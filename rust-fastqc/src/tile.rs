//! Per-tile mean quality deviations for Illumina FASTQ headers.
//!
//! Uses the first 10,000 nonempty reads and every tenth read thereafter.
//! Counts and quality sums are sufficient; per-quality histograms are unnecessary.
//! Memory is O(sum of the longest sampled read length for each detected tile).

use crate::grouping::base_groups;
use crate::modules::{ModuleResult, Status};
use crate::stats::java_double;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
struct Position {
    count: u64,
    sum: u64,
}

/// Per-file tile detection and sampled position statistics.
#[derive(Clone, Debug, Default)]
pub struct TileQuality {
    tiles: BTreeMap<i32, Vec<Position>>,
    records: u64,
    split_position: Option<usize>,
    disabled: bool,
    length: usize,
}

/// Match Java's signed i32 parsing, including its BMP decimal-digit blocks.
/// Supplementary digits use surrogate pairs in Java and are not accepted there.
fn tile_id(field: &[u8]) -> Option<i32> {
    let text = std::str::from_utf8(field).ok()?;
    if text.is_ascii() {
        return text.parse().ok();
    }
    const ZEROS: [u32; 37] = [
        0x0030, 0x0660, 0x06f0, 0x07c0, 0x0966, 0x09e6, 0x0a66, 0x0ae6, 0x0b66, 0x0be6, 0x0c66,
        0x0ce6, 0x0d66, 0x0de6, 0x0e50, 0x0ed0, 0x0f20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946,
        0x19d0, 0x1a80, 0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0,
        0xa9f0, 0xaa50, 0xabf0, 0xff10,
    ];
    let mut normalized = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '+' || ch == '-' {
            normalized.push(ch);
            continue;
        }
        let digit = ZEROS
            .iter()
            .find_map(|zero| u32::from(ch).checked_sub(*zero).filter(|digit| *digit < 10))?;
        normalized.push(char::from_digit(digit, 10)?);
    }
    normalized.parse().ok()
}

impl TileQuality {
    /// Observe an eligible read after length filtering, using raw quality bytes.
    ///
    /// Malformed sampled headers disable this optional module for the file, as
    /// in FastQC. More than 2501 distinct tile IDs triggers its parsing heuristic
    /// and returns the upstream warning once. Unsampled headers are not parsed.
    pub fn add_read(&mut self, header: &[u8], quality: &[u8], offset: u8) -> Option<&'static str> {
        if self.disabled || quality.is_empty() {
            return None;
        }
        self.records += 1;
        if self.records > 10_000 && !self.records.is_multiple_of(10) {
            return None;
        }
        let mut fields: Vec<_> = header.split(|byte| *byte == b':').collect();
        while fields.last().is_some_and(|field| field.is_empty()) {
            fields.pop();
        }
        let index = self.split_position.or({
            if fields.len() >= 7 {
                Some(4)
            } else if fields.len() >= 5 {
                Some(2)
            } else {
                None
            }
        });
        let tile = index
            .and_then(|i| fields.get(i))
            .and_then(|field| tile_id(field));
        let Some(tile) = tile else {
            self.disabled = true;
            self.tiles.clear();
            return None;
        };
        self.split_position = index;
        if !self.tiles.contains_key(&tile) && self.tiles.len() > 2500 {
            self.disabled = true;
            self.tiles.clear();
            return Some("Too many tiles (>2500) so giving up trying to do per-tile qualities since we're probably parsing the file wrongly");
        }
        self.length = self.length.max(quality.len());
        let positions = self.tiles.entry(tile).or_default();
        positions.resize_with(positions.len().max(quality.len()), Position::default);
        for (position, byte) in positions.iter_mut().zip(quality) {
            position.count += 1;
            position.sum += u64::from(byte.saturating_sub(offset).min(93));
        }
        None
    }

    /// Render observed tile deviations; absent observations remain `NaN`.
    ///
    /// Each position has equal weight within a group; each observed tile has
    /// equal weight in that group's reference mean. An unobserved tile/group
    /// contributes neither an invented zero quality nor a reference weight.
    pub fn report(&self, nogroup: bool, warn: f64, error: f64) -> Option<ModuleResult> {
        if self.disabled || self.length == 0 {
            return None;
        }
        let groups = base_groups(self.length, nogroup);
        let mut means: Vec<Vec<f64>> = Vec::with_capacity(self.tiles.len());
        for positions in self.tiles.values() {
            let row = groups
                .iter()
                .map(|group| {
                    let mut count = 0;
                    let mut sum = 0.0;
                    for i in group.start - 1..group.end {
                        if let Some(position) = positions.get(i).filter(|p| p.count > 0) {
                            sum += position.sum as f64 / position.count as f64;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        f64::NAN
                    } else {
                        sum / f64::from(count)
                    }
                })
                .collect();
            means.push(row);
        }
        let mut worst = 0.0f64;
        for i in 0..groups.len() {
            let mut sum = 0.0;
            let mut count = 0;
            for row in &means {
                if row[i].is_finite() {
                    sum += row[i];
                    count += 1;
                }
            }
            let average = if count == 0 {
                f64::NAN
            } else {
                sum / f64::from(count)
            };
            for row in &mut means {
                row[i] -= average;
                if row[i].is_finite() {
                    worst = worst.max(row[i].abs());
                }
            }
        }
        let mut lines = vec!["#Tile\tBase\tMean".into()];
        for (tile, row) in self.tiles.keys().zip(means) {
            for (group, mean) in groups.iter().zip(row) {
                lines.push(format!("{tile}\t{}\t{}", group.label(), java_double(mean)));
            }
        }
        Some(ModuleResult {
            name: "Per tile sequence quality",
            status: if worst > error {
                Status::Fail
            } else if worst > warn {
                Status::Warn
            } else {
                Status::Pass
            },
            lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(stats: &TileQuality) -> ModuleResult {
        stats.report(false, 5.0, 10.0).unwrap()
    }

    #[test]
    fn unicode_tile_ids_preserve_java_integer_bounds() {
        assert_eq!(tile_id("-٢١٤٧٤٨٣٦٤٨".as_bytes()), Some(i32::MIN));
        assert_eq!(tile_id("２１４７４８３６４７".as_bytes()), Some(i32::MAX));
        assert_eq!(tile_id("٢١٤٧٤٨٣٦٤٨".as_bytes()), None);
        assert_eq!(tile_id("𝟙".as_bytes()), None);
        assert_eq!(tile_id("－1".as_bytes()), None);
    }

    #[test]
    fn no_observation_is_not_zero_quality() {
        let mut stats = TileQuality::default();
        stats.add_read(b"I:1:F:1:1:0:0", b"5555", 33);
        stats.add_read(b"I:1:F:1:2:0:0", b"55555555", 33);
        let result = report(&stats);
        assert_eq!(result.status, Status::Pass);
        assert_eq!(result.lines[5], "1\t5\tNaN");
        assert_eq!(result.lines[13], "2\t5\t0.0");
    }

    #[test]
    fn an_observed_zero_contributes_to_the_reference_mean() {
        let mut stats = TileQuality::default();
        stats.add_read(b"I:1:F:1:2:0:0", b"!!!!", 33);
        stats.add_read(b"I:1:F:1:1:0:0", b"7777", 33);
        let result = report(&stats);
        assert_eq!(result.status, Status::Fail);
        assert_eq!(result.lines[1], "1\t1\t11.0");
        assert_eq!(result.lines[5], "2\t1\t-11.0");
    }

    #[test]
    fn sampling_skips_headers_and_quality_together() {
        let mut stats = TileQuality::default();
        for _ in 0..10_000 {
            stats.add_read(b"I:1:F:1:1:0:0", b"5", 33);
        }
        for _ in 0..9 {
            stats.add_read(b"invalid", b"!", 33);
        }
        assert!(stats.report(false, 5.0, 10.0).is_some());
        stats.add_read(b"invalid", b"!", 33);
        assert!(stats.report(false, 5.0, 10.0).is_none());
    }

    #[test]
    fn reporting_is_repeatable_and_does_not_destroy_counts() {
        let mut stats = TileQuality::default();
        stats.add_read(b"I:1:1:0:0", b"55", 33);
        assert_eq!(report(&stats), report(&stats));
        stats.add_read(b"I:1:2:0:0", b"!!", 33);
        assert_eq!(report(&stats).status, Status::Warn);
    }
}
