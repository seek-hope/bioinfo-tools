//! FastQC's base grouping for reads longer than 75bp.
//!
//! For a long read FastQC does not print one row per base position; it prints one
//! row per *group* of positions, labelled `10-11`, `12-13` and so on. Reproducing
//! the grouping is required for the report to match on long reads: without it the
//! tables are longer and every position row disagrees.
//!
//! ## The interval rule
//!
//! Up to 75 positions remain ungrouped. Above that, `getLinearInterval(length)`
//! picks the first candidate from `2, 5, 10`, then `20, 50, 100`, and so on,
//! whose resulting group count is under 75:
//!
//! ```text
//! interval = candidate * multiplier
//! groups   = 9 + (length - 9) / interval
//! if ((length - 9) % interval != 0) groups += 1
//! if (groups < 75) return interval
//! ```
//!
//! ## The grouping rule
//!
//! Positions 1..=9 are always their own single-base groups (the first 9 bases are
//! never merged, because quality is most variable there). From position 10 the
//! remaining positions use `interval`, with the final group truncated at the
//! read length. For intervals above ten, the first merged group is 10..interval-1
//! so all subsequent groups start at interval multiples.
//!
//! Pinned FastQC 0.13.0 execution confirms 75 individual rows at length 75,
//! and `10-11`, `12-13`, `14-15`, ... at lengths 76 and 80.
//! See `tests/oracle/fastqc_tile_diff.py` for direct boundary checks.

/// A group of base positions, 1-based and inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseGroup {
    /// First position in the group, 1-based.
    pub start: usize,
    /// Last position in the group, 1-based and inclusive.
    pub end: usize,
}

impl BaseGroup {
    /// The label FastQC prints, e.g. `10-11` or `12` for a single position.
    pub fn label(&self) -> String {
        if self.start == self.end {
            format!("{}", self.start)
        } else {
            format!("{}-{}", self.start, self.end)
        }
    }
}

/// FastQC's `getLinearInterval`.
pub fn linear_interval(read_length: usize) -> usize {
    if read_length <= 75 {
        return 1;
    }
    let mut multiplier: usize = 1;
    loop {
        for candidate in [2usize, 5, 10] {
            let interval = candidate * multiplier;
            let span = read_length.saturating_sub(9);
            let mut groups = 9 + span / interval;
            if !span.is_multiple_of(interval) {
                groups += 1;
            }
            if groups < 75 {
                return interval;
            }
        }
        multiplier *= 10;
        if multiplier == 10_000_000 {
            return read_length.max(1);
        }
    }
}

/// FastQC's `makeBaseGroups`: one group per position up to 9, then chunks.
pub fn base_groups(read_length: usize, nogroup: bool) -> Vec<BaseGroup> {
    if nogroup {
        return (1..=read_length)
            .map(|position| BaseGroup {
                start: position,
                end: position,
            })
            .collect();
    }

    let interval = linear_interval(read_length);
    let mut groups = Vec::new();
    let mut start = 1usize;
    while start <= read_length {
        // Positions 1..=9 stay unmerged.
        let width = if start <= 9 { 1 } else { interval };
        let end = if start == 10 && interval > 10 {
            interval - 1
        } else {
            (start + width - 1).min(read_length)
        };
        groups.push(BaseGroup { start, end });
        start = end + 1;
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_up_to_75_bases_are_never_grouped() {
        for length in [1usize, 9, 20, 50, 74, 75] {
            let groups = base_groups(length, false);
            assert!(
                groups.iter().all(|group| group.start == group.end),
                "length {length} should not group"
            );
            assert_eq!(groups.len(), length);
        }
    }

    #[test]
    fn grouping_starts_after_75_bases() {
        // Direct pinned executable comparison corrects the former test's
        // mistaken claim that 75 positions were already grouped.
        assert_eq!(linear_interval(75), 1);
        assert_eq!(base_groups(75, false).len(), 75);
        assert_eq!(linear_interval(76), 2);
        assert!(base_groups(76, false).len() < 75);
    }

    #[test]
    fn an_eighty_base_read_matches_fastqc() {
        // 9 individual positions followed by 10-11, 12-13, 14-15, ...
        let groups = base_groups(80, false);
        let labels: Vec<String> = groups.iter().map(|group| group.label()).collect();
        assert_eq!(labels[0], "1");
        assert_eq!(labels[8], "9");
        assert_eq!(labels[9], "10-11");
        assert_eq!(labels[10], "12-13");
        assert_eq!(labels[11], "14-15");
        // Groups must tile the read exactly.
        assert_eq!(groups.first().unwrap().start, 1);
        assert_eq!(groups.last().unwrap().end, 80);
        for pair in groups.windows(2) {
            assert_eq!(pair[0].end + 1, pair[1].start, "gap between groups");
        }
    }

    #[test]
    fn large_intervals_align_to_round_position_boundaries() {
        for (length, interval) in [(660, 20), (1400, 50), (4000, 100)] {
            let groups = base_groups(length, false);
            assert_eq!(
                groups[9],
                BaseGroup {
                    start: 10,
                    end: interval - 1
                }
            );
            assert_eq!(
                groups[10],
                BaseGroup {
                    start: interval,
                    end: 2 * interval - 1
                }
            );
            assert_eq!(groups.last().unwrap().end, length);
        }
    }

    #[test]
    fn interval_selection_keeps_group_count_under_75() {
        for length in [10usize, 50, 80, 150, 300, 1000, 10_000] {
            let interval = linear_interval(length);
            assert!(interval >= 1, "interval for {length}");
            let groups = base_groups(length, false);
            assert!(
                groups.len() <= 75,
                "{length} produced {} groups",
                groups.len()
            );
        }
    }

    #[test]
    fn nogroup_reports_every_position() {
        let groups = base_groups(80, true);
        assert_eq!(groups.len(), 80);
        assert_eq!(groups[9].label(), "10");
    }

    #[test]
    fn groups_tile_the_read_for_many_lengths() {
        for length in 1..=200 {
            let groups = base_groups(length, false);
            assert_eq!(groups[0].start, 1, "length {length}");
            assert_eq!(groups.last().unwrap().end, length, "length {length}");
            for pair in groups.windows(2) {
                assert_eq!(pair[0].end + 1, pair[1].start, "length {length}");
            }
        }
    }
}
