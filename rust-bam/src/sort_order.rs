//! Sort order: coordinates, tie-breaking and the documented ordering contract.
//!
//! ## Documented sort order
//!
//! `rust-bam sort` implements **coordinate sort** with the same total order
//! `samtools sort` uses, so that a sorted file compares equal record-for-record:
//!
//! 1. **Unmapped records** (flag 0x4 set, or no `RNAME`, or no `POS`) sort
//!    **last**, after every placed record.
//! 2. Among placed records, by **reference sequence id** in `@SQ` header order
//!    (so `chr1` before `chr2` follows the header, not alphabetical order).
//! 3. Then by **leftmost alignment start**, ascending, 1-based.
//! 4. Then by **reverse strand** — forward (0x10 clear) before reverse (0x10
//!    set). This is `samtools`'s tie-break and it matters because two alignments
//!    can share a start position.
//! 5. Then by **name**, as a final deterministic tie-break.
//! 6. Then by the **original input index**, so the order is total: two records
//!    identical in every compared field still come out in input order, which is
//!    what makes the output reproducible run to run.
//!
//! Steps 1–4 are what `samtools sort` does. Step 5 is not in samtools' contract
//! (its heap ordering leaves such ties unspecified); adding name and then input
//! index makes `rust-bam`'s output **stable and reproducible**, which the plan
//! requires ("sort order documented and stable"). This is recorded as
//! `DR-W1-4-3`.
//!
//! ## Stability
//!
//! The comparator is a total order over records that carry distinct input
//! indices, so `sort_by` cannot produce run-to-run variation. There is no
//! parallelism and no hash iteration anywhere in the sort path.

use noodles::sam::alignment::record::Flags;

/// The unmapped flag bit, 0x4.
pub const UNMAPPED: u16 = 0x4;
/// The reverse-strand flag bit, 0x10.
pub const REVERSE: u16 = 0x10;

/// The sort key for one record.
///
/// Deliberately a plain struct of scalars so it can be compared without touching
/// the record again, and so the ordering is inspectable in a unit test.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SortKey {
    /// 0 for a placed record, 1 for an unmapped one. Unmapped sorts last.
    pub unmapped_rank: u8,
    /// Reference sequence id, in `@SQ` order. 0 when unmapped.
    pub reference_id: usize,
    /// 1-based leftmost alignment start. 0 when unmapped.
    pub start: usize,
    /// 0 for forward, 1 for reverse.
    pub reverse_rank: u8,
    /// Record name, for a deterministic tie-break.
    pub name: Vec<u8>,
    /// Position in the input, so the order is total.
    pub input_index: usize,
}

impl SortKey {
    /// Build a key from the fields that affect ordering.
    pub fn new(
        flags: Flags,
        reference_id: Option<usize>,
        start: Option<usize>,
        name: &[u8],
        input_index: usize,
    ) -> SortKey {
        let raw = flags.bits();
        // A record is "placed" only when it is mapped AND has both a reference and
        // a position. samtools treats a record missing either as unplaced.
        let placed = raw & UNMAPPED == 0 && reference_id.is_some() && start.is_some();
        SortKey {
            unmapped_rank: if placed { 0 } else { 1 },
            reference_id: if placed { reference_id.unwrap_or(0) } else { 0 },
            start: if placed { start.unwrap_or(0) } else { 0 },
            reverse_rank: if raw & REVERSE != 0 { 1 } else { 0 },
            name: name.to_vec(),
            input_index,
        }
    }

    /// True when this record sorts after every placed record.
    pub fn is_unplaced(&self) -> bool {
        self.unmapped_rank == 1
    }
}

/// Sort records by their keys, returning them in documented order.
///
/// `records` is consumed and returned reordered. This is a stable sort over a
/// total order, so the result is deterministic.
pub fn sort_by_key<T>(mut records: Vec<(SortKey, T)>) -> Vec<T> {
    records.sort_by(|a, b| a.0.cmp(&b.0));
    records.into_iter().map(|(_, item)| item).collect()
}

/// Check whether a sequence of keys is already sorted.
///
/// Used by `index` to decide whether a file satisfies the precondition for
/// indexing, mirroring `samtools index`, which refuses an unsorted file.
pub fn is_sorted(keys: &[SortKey]) -> bool {
    keys.windows(2).all(|pair| pair[0] <= pair[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(
        flags: u16,
        reference_id: Option<usize>,
        start: Option<usize>,
        name: &str,
        index: usize,
    ) -> SortKey {
        SortKey::new(
            Flags::from_bits_truncate(flags),
            reference_id,
            start,
            name.as_bytes(),
            index,
        )
    }

    #[test]
    fn placed_records_come_before_unmapped_ones() {
        let unmapped = key(UNMAPPED, None, None, "z", 0);
        let placed = key(0, Some(0), Some(100), "a", 1);
        assert!(placed < unmapped);
        assert!(unmapped.is_unplaced());
        assert!(!placed.is_unplaced());
    }

    #[test]
    fn a_record_missing_a_position_is_unplaced_even_without_the_flag() {
        // Not the unmapped flag, but no POS: samtools treats it as unplaced.
        let no_position = key(0, Some(0), None, "a", 0);
        assert!(no_position.is_unplaced());
    }

    #[test]
    fn a_record_missing_a_reference_is_unplaced() {
        let no_reference = key(0, None, Some(10), "a", 0);
        assert!(no_reference.is_unplaced());
    }

    #[test]
    fn ordering_is_by_reference_then_start() {
        let mut records = [
            (key(0, Some(1), Some(10), "c", 0), "chr2:10"),
            (key(0, Some(0), Some(200), "b", 1), "chr1:200"),
            (key(0, Some(0), Some(100), "a", 2), "chr1:100"),
        ];
        // Rearranged by the sort, not by the initial order.
        records.sort_by(|a, b| a.0.cmp(&b.0));
        let order: Vec<&str> = records.iter().map(|(_, label)| *label).collect();
        assert_eq!(order, ["chr1:100", "chr1:200", "chr2:10"]);
    }

    #[test]
    fn reference_order_follows_the_header_not_the_name() {
        // id 0 is "chr2", id 1 is "chr10" -- header order decides.
        let mut records = [
            (key(0, Some(1), Some(5), "b", 0), "id1"),
            (key(0, Some(0), Some(5), "a", 1), "id0"),
        ];
        records.sort_by(|a, b| a.0.cmp(&b.0));
        let order: Vec<&str> = records.iter().map(|(_, label)| *label).collect();
        assert_eq!(order, ["id0", "id1"]);
    }

    #[test]
    fn forward_strand_sorts_before_reverse_at_the_same_position() {
        let forward = key(0, Some(0), Some(100), "same", 0);
        let reverse = key(REVERSE, Some(0), Some(100), "same", 1);
        assert!(forward < reverse);
    }

    #[test]
    fn the_order_is_total_so_output_is_reproducible() {
        // Two records identical in every compared field still order by input index.
        let first = key(0, Some(0), Some(100), "same", 0);
        let second = key(0, Some(0), Some(100), "same", 1);
        assert!(first < second);
        assert!(second > first);
        assert_ne!(first, second);
    }

    #[test]
    fn sorting_is_stable_across_repeated_runs() {
        let build = || {
            vec![
                (key(UNMAPPED, None, None, "u1", 0), "u1"),
                (key(0, Some(0), Some(50), "a", 1), "a"),
                (key(REVERSE, Some(0), Some(50), "b", 2), "b"),
                (key(0, Some(0), Some(10), "c", 3), "c"),
            ]
        };
        let once = sort_by_key(build());
        for _ in 0..5 {
            assert_eq!(sort_by_key(build()), once);
        }
        assert_eq!(once, ["c", "a", "b", "u1"]);
    }

    #[test]
    fn is_sorted_detects_a_permutation() {
        let sorted = vec![
            key(0, Some(0), Some(10), "a", 0),
            key(0, Some(0), Some(20), "b", 1),
            key(UNMAPPED, None, None, "c", 2),
        ];
        assert!(is_sorted(&sorted));

        let unsorted = vec![
            key(0, Some(0), Some(20), "b", 0),
            key(0, Some(0), Some(10), "a", 1),
        ];
        assert!(!is_sorted(&unsorted));
    }

    #[test]
    fn a_single_record_and_an_empty_list_are_sorted() {
        assert!(is_sorted(&[]));
        assert!(is_sorted(&[key(0, Some(0), Some(1), "a", 0)]));
    }
}
