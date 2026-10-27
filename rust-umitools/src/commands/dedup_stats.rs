//! `dedup --output-stats`: observed UMI/count summaries and a seeded null model.
//!
//! The observed columns follow UMI-tools 1.1.6's three TSV layouts. Null columns
//! reproduce NumPy RandomState weighted choice, including singleton draws and the
//! `unique` method's duplicate-dict-key overwrite of pre-cluster values.

use crate::core::umi::Umi;
use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// One bundle's pre/post UMI observations.
pub struct BundleStatsRow {
    pub pre: Vec<(Umi, u64)>,
    pub post: Vec<(Umi, u64)>,
    pub pre_size: usize,
    pub post_size: usize,
}

struct NumpyRandom {
    mt: [u32; 624],
    index: usize,
    random_fill_size: usize,
    random_umis: Vec<Umi>,
    random_ix: usize,
}

impl NumpyRandom {
    fn new(seed: i64) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed as u32;
        for index in 1..624 {
            mt[index] = 1_812_433_253u32
                .wrapping_mul(mt[index - 1] ^ (mt[index - 1] >> 30))
                .wrapping_add(index as u32);
        }
        Self {
            mt,
            index: 624,
            random_fill_size: 100_000,
            random_umis: Vec::new(),
            random_ix: 100_000,
        }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for i in 0..624 {
                let y = (self.mt[i] & 0x8000_0000) | (self.mt[(i + 1) % 624] & 0x7fff_ffff);
                self.mt[i] =
                    self.mt[(i + 397) % 624] ^ (y >> 1) ^ if y & 1 == 1 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    fn next_f64(&mut self) -> f64 {
        let a = u64::from(self.next_u32() >> 5);
        let b = u64::from(self.next_u32() >> 6);
        (a as f64 * 67_108_864.0 + b as f64) / 9_007_199_254_740_992.0
    }

    fn refill(&mut self, pool: &[(Umi, u64)]) {
        let total: u64 = pool.iter().map(|(_, count)| *count).sum();
        let mut cumulative = Vec::with_capacity(pool.len());
        let mut sum = 0u64;
        for (_, count) in pool {
            sum += count;
            cumulative.push(sum as f64 / total as f64);
        }
        self.random_umis.clear();
        self.random_umis.reserve(self.random_fill_size);
        for _ in 0..self.random_fill_size {
            let value = self.next_f64();
            let index = cumulative
                .iter()
                .position(|threshold| value < *threshold)
                .unwrap_or(pool.len() - 1);
            self.random_umis.push(pool[index].0.clone());
        }
        self.random_ix = 0;
    }

    fn get_umis(&mut self, n: usize, pool: &[(Umi, u64)]) -> Vec<Umi> {
        if n >= self.random_fill_size - self.random_ix {
            if n > self.random_fill_size {
                self.random_fill_size = n * 2;
            }
            self.refill(pool);
        }
        let result = self.random_umis[self.random_ix..self.random_ix + n].to_vec();
        self.random_ix += n;
        result
    }
}

/// Accumulates rows and writes the three upstream TSV files.
pub struct StatsCollector {
    rows: Vec<BundleStatsRow>,
    pool: Vec<(Umi, u64)>,
    rng: NumpyRandom,
}

impl StatsCollector {
    pub fn new(seed: i64) -> StatsCollector {
        StatsCollector {
            rows: Vec::new(),
            pool: Vec::new(),
            rng: NumpyRandom::new(seed),
        }
    }

    pub fn push(&mut self, row: BundleStatsRow) {
        for (umi, count) in &row.pre {
            if let Some((_, existing)) = self.pool.iter_mut().find(|(other, _)| other == umi) {
                *existing += count;
            } else {
                self.pool.push((umi.clone(), *count));
            }
        }
        self.rows.push(row);
    }

    fn null_average(&mut self, size: usize) -> f64 {
        if self.pool.is_empty() {
            return -1.0;
        }
        let sample = self.rng.get_umis(size, &self.pool);
        average_distance(&sample)
    }

    /// Write `<prefix>_per_umi_per_position.tsv`, `<prefix>_per_umi.tsv` and
    /// `<prefix>_edit_distance.tsv`.
    pub fn write(mut self, prefix: &str, method: &str) -> Result<()> {
        let mut positions: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
        let mut per_umi: BTreeMap<Vec<u8>, (Vec<u64>, Vec<u64>)> = BTreeMap::new();
        for row in &self.rows {
            for (_, count) in &row.pre {
                positions.entry(*count).or_default().0 += 1;
            }
            for (_, count) in &row.post {
                positions.entry(*count).or_default().1 += 1;
            }
            for (umi, count) in &row.pre {
                per_umi.entry(umi.to_bytes()).or_default().0.push(*count);
            }
            for (umi, count) in &row.post {
                per_umi.entry(umi.to_bytes()).or_default().1.push(*count);
            }
        }

        let mut position_text = String::from("counts\tinstances_pre\tinstances_post\n");
        for (count, (pre, post)) in &positions {
            position_text.push_str(&format!("{count}\t{pre}\t{post}\n"));
        }
        write_file(
            &format!("{prefix}_per_umi_per_position.tsv"),
            &position_text,
        )?;

        let mut umi_text = String::from(
            "UMI\tmedian_counts_pre\ttimes_observed_pre\ttotal_counts_pre\t\
             median_counts_post\ttimes_observed_post\ttotal_counts_post\n",
        );
        for (umi, (pre, post)) in &per_umi {
            umi_text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                String::from_utf8_lossy(umi),
                median(pre),
                pre.len(),
                pre.iter().sum::<u64>(),
                median(post),
                post.len(),
                post.iter().sum::<u64>(),
            ));
        }
        write_file(&format!("{prefix}_per_umi.tsv"), &umi_text)?;

        let rows: Vec<(Vec<Umi>, Vec<Umi>, usize, usize)> = self
            .rows
            .iter()
            .map(|row| {
                (
                    row.pre.iter().map(|(umi, _)| umi.clone()).collect(),
                    row.post.iter().map(|(umi, _)| umi.clone()).collect(),
                    row.pre_size,
                    row.post_size,
                )
            })
            .collect();
        let mut edit_rows = Vec::with_capacity(rows.len());
        for (pre_values, post_values, pre_size, post_size) in rows {
            let pre = average_distance(&pre_values);
            let post = average_distance(&post_values);
            let pre_null = self.null_average(pre_size);
            let post_null = self.null_average(post_size);
            edit_rows.push((pre, post, pre_null, post_null));
        }
        let max_ed = edit_rows
            .iter()
            .flat_map(|(a, b, c, d)| [*a, *b, *c, *d])
            .fold(0.0f64, f64::max)
            .max(0.0) as i64;
        let width = (max_ed + 3) as usize;
        let mut unique = vec![0u64; width];
        let mut unique_null = vec![0u64; width];
        let mut post = vec![0u64; width];
        let mut post_null = vec![0u64; width];
        for (pre, post_value, pre_null, post_null_value) in edit_rows {
            if method == "unique" {
                // UMI-tools builds a dict with duplicate `unique` and
                // `unique_null` keys for this method. The post-cluster values
                // overwrite the pre-cluster values in both columns.
                unique[bin(post_value, max_ed)] += 1;
                unique_null[bin(post_null_value, max_ed)] += 1;
            } else {
                unique[bin(pre, max_ed)] += 1;
                unique_null[bin(pre_null, max_ed)] += 1;
            }
            post[bin(post_value, max_ed)] += 1;
            post_null[bin(post_null_value, max_ed)] += 1;
        }
        let mut edit_text =
            format!("unique\tunique_null\t{method}\t{method}_null\tedit_distance\n");
        for index in 0..width {
            let label = if index == 0 {
                "Single_UMI".to_string()
            } else {
                (index as i64 - 1).to_string()
            };
            edit_text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                unique[index], unique_null[index], post[index], post_null[index], label
            ));
        }
        write_file(&format!("{prefix}_edit_distance.tsv"), &edit_text)
    }
}

fn bin(value: f64, max_ed: i64) -> usize {
    if value < 0.0 {
        0
    } else {
        ((value.ceil() as i64) + 1).clamp(1, max_ed + 2) as usize
    }
}

fn median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2
    } else {
        sorted[middle]
    }
}

fn average_distance(umis: &[Umi]) -> f64 {
    if umis.len() <= 1 {
        return -1.0;
    }
    let mut sum = 0u64;
    let mut pairs = 0u64;
    for left in 0..umis.len() {
        let left_bytes = umis[left].to_bytes();
        for right in umis.iter().skip(left + 1) {
            let right = right.to_bytes();
            sum += left_bytes
                .iter()
                .zip(right.iter())
                .filter(|(a, b)| a != b)
                .count() as u64;
            pairs += 1;
        }
    }
    sum as f64 / pairs as f64
}

fn write_file(path: &str, text: &str) -> Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
    }
    std::fs::write(path, text).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numpy_seed_and_weighted_choice_match_reference() {
        let mut rng = NumpyRandom::new(1);
        let expected = [
            0.417_022_004_702_574,
            0.720_324_493_442_158_1,
            0.000_114_374_817_344_89,
            0.302_332_572_631_839_77,
        ];
        for value in expected {
            assert!((rng.next_f64() - value).abs() < 1e-15);
        }
        let pool = vec![
            (Umi::from_bytes(b"AAAA"), 2),
            (Umi::from_bytes(b"AAAT"), 1),
            (Umi::from_bytes(b"AAAC"), 1),
            (Umi::from_bytes(b"GGGG"), 1),
        ];
        let mut rng = NumpyRandom::new(1);
        let first = rng.get_umis(4, &pool);
        let second = rng.get_umis(4, &pool);
        assert_eq!(
            first.iter().map(|umi| umi.to_bytes()).collect::<Vec<_>>(),
            [
                b"AAAT".to_vec(),
                b"AAAC".to_vec(),
                b"AAAA".to_vec(),
                b"AAAA".to_vec()
            ]
        );
        assert_eq!(
            second.iter().map(|umi| umi.to_bytes()).collect::<Vec<_>>(),
            vec![
                b"AAAA".to_vec(),
                b"AAAA".to_vec(),
                b"AAAA".to_vec(),
                b"AAAA".to_vec(),
            ]
        );
    }

    #[test]
    fn edit_distance_bins_use_right_closed_digitize_semantics() {
        assert_eq!(bin(-1.0, 4), 0);
        assert_eq!(bin(0.5, 4), 2);
        assert_eq!(bin(1.0, 4), 2);
        assert_eq!(bin(1.5, 4), 3);
        assert_eq!(bin(2.5, 4), 4);
        assert_eq!(bin(3.0, 4), 4);
        assert_eq!(bin(4.0, 4), 5);
    }
}
