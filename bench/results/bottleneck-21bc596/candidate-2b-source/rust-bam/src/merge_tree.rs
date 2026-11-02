//! Index-only selection for a multiway merge with fixed run heads.
//!
//! Building costs O(k) and replacing one leaf costs O(log k), with at most one
//! key comparison per level. Only indices move through the tree; records and
//! their allocations stay in the caller's slots. Storage is less than 4k indices
//! for nonempty inputs. Equal keys select the earlier run.

use std::cmp::Ordering;

const EMPTY: usize = usize::MAX;

pub(crate) struct MergeTree {
    leaves: usize,
    winners: Vec<usize>,
}

impl MergeTree {
    pub(crate) fn new(
        len: usize,
        active: impl Fn(usize) -> bool,
        mut compare: impl FnMut(usize, usize) -> Ordering,
    ) -> Self {
        let leaves = len.max(1).next_power_of_two();
        let mut winners = vec![EMPTY; 2 * leaves];
        for index in 0..len {
            if active(index) {
                winners[leaves + index] = index;
            }
        }
        for node in (1..leaves).rev() {
            winners[node] = Self::choose(winners[2 * node], winners[2 * node + 1], &mut compare);
        }
        Self { leaves, winners }
    }

    fn choose(
        left: usize,
        right: usize,
        compare: &mut impl FnMut(usize, usize) -> Ordering,
    ) -> usize {
        if left == EMPTY {
            right
        } else if right == EMPTY || compare(left, right).then_with(|| left.cmp(&right)).is_le() {
            left
        } else {
            right
        }
    }

    pub(crate) fn winner(&self) -> Option<usize> {
        (self.winners[1] != EMPTY).then_some(self.winners[1])
    }

    pub(crate) fn replace(
        &mut self,
        index: usize,
        active: bool,
        mut compare: impl FnMut(usize, usize) -> Ordering,
    ) {
        let mut node = self.leaves + index;
        self.winners[node] = if active { index } else { EMPTY };
        while node > 1 {
            node /= 2;
            self.winners[node] = Self::choose(
                self.winners[2 * node],
                self.winners[2 * node + 1],
                &mut compare,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_matches_total_sort_with_ties_empty_runs_and_unequal_lengths() {
        for count in [0usize, 1, 2, 3, 7, 8, 9, 31, 64, 65] {
            let runs: Vec<Vec<usize>> = (0..count)
                .map(|run| (0..(run * 7 % 19)).map(|i| (i / 3) * 5 + run % 4).collect())
                .collect();
            let mut offsets = vec![0; count];
            let mut tree = MergeTree::new(
                count,
                |i| !runs[i].is_empty(),
                |a, b| runs[a][0].cmp(&runs[b][0]),
            );
            let mut actual = Vec::new();
            while let Some(run) = tree.winner() {
                actual.push((runs[run][offsets[run]], run, offsets[run]));
                offsets[run] += 1;
                tree.replace(run, offsets[run] < runs[run].len(), |a, b| {
                    runs[a][offsets[a]].cmp(&runs[b][offsets[b]])
                });
            }
            let mut expected: Vec<_> = runs
                .iter()
                .enumerate()
                .flat_map(|(run, values)| {
                    values
                        .iter()
                        .enumerate()
                        .map(move |(i, &value)| (value, run, i))
                })
                .collect();
            expected.sort();
            assert_eq!(actual, expected, "runs={count}");
        }
    }

    #[test]
    fn replacement_has_one_comparison_per_level_and_can_reactivate_a_slot() {
        for len in [1usize, 2, 3, 8, 17, 64, 65] {
            let mut keys: Vec<_> = (0..len).collect();
            let mut tree = MergeTree::new(len, |_| true, |a, b| keys[a].cmp(&keys[b]));
            let depth = len.next_power_of_two().trailing_zeros();
            let capacity = tree.winners.capacity();
            for step in 0..len * 3 {
                let run = tree.winner().unwrap();
                keys[run] = len + step;
                let mut comparisons = 0;
                tree.replace(run, true, |a, b| {
                    comparisons += 1;
                    keys[a].cmp(&keys[b])
                });
                assert!(comparisons <= depth);
                assert_eq!(tree.winner(), (0..len).min_by_key(|&i| (keys[i], i)));
                assert_eq!(tree.winners.capacity(), capacity);
            }
            for i in 0..len {
                tree.replace(i, false, |a, b| keys[a].cmp(&keys[b]));
            }
            assert_eq!(tree.winner(), None);
            tree.replace(len - 1, true, |a, b| keys[a].cmp(&keys[b]));
            assert_eq!(tree.winner(), Some(len - 1));
        }
    }
}
