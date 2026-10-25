//! Paired-end trimming.
//!
//! Paired-end runs process R1 and R2 in lockstep: record *n* of read 1 is
//! trimmed with the `-a`/`-g`/`-b` adapter set and record *n* of read 2 with the
//! separate `-A`/`-G`/`-B` set, then the pair is written to `-o` and
//! `-p`/`--paired-output` (or interleaved to `-o`).
//!
//! # Why a separate module
//!
//! Single-end trimming already lives in [`crate::engine::Trimmer`], which parses
//! a chunk, applies the adapter set, runs the modifiers, filters and auxiliary
//! outputs, and serializes the retained records. Paired-end differs only in
//! *how records are paired and how the pair-level decisions are made*, so this
//! module drives one `Trimmer` per mate instead of reimplementing any of it:
//!
//! * [`split_records`] cuts a byte buffer into complete 4-line records so the
//!   two mates can be zipped;
//! * `PairOutcome` carries the serialized bytes and the per-mate statistics
//!   that the report needs;
//! * `pair_is_written` implements `--pair-filter any|both|first`.
//!
//! # Report numbering
//!
//! cutadapt numbers adapters continuously across the two reads: with `-a A -A B`
//! the R1 section is `Adapter 1` and the R2 section is `Adapter 2` (verified
//! against the oracle). [`adapter_number_offset`] is that offset for read 2.
//!
//! # Determinism
//!
//! Pairs are split at record boundaries and each split unit is processed
//! independently, so the output does not depend on the number of threads and
//! chunks stay bounded in memory.

use crate::engine::{ChunkStats, Trimmer};
use crate::fastq::Parser;

/// Maximum number of records whose statistics are kept individually.
///
/// The per-read filters need a verdict per mate before the pair-level decision
/// can be made, so a chunk's records are first processed individually and then
/// reassembled. Keeping one [`ChunkStats`] per record would be a large
/// allocation for a small chunk, so records are processed in fixed-size groups:
/// each group is a self-contained `Trimmer::process_chunk` call and the verdicts
/// are inspected pair by pair.
pub const RECORDS_PER_GROUP: usize = 4096;

/// Offsets of one complete FASTQ record inside a buffer.
///
/// Re-exported from `bioinfo_fastx::chunker`. W1-2 moved the record-boundary rule
/// into the shared crate, so the paired-end path and the single-end chunker now
/// provably share one definition of "complete record" — which is what makes a
/// paired split line up with a single-end split.
pub use bioinfo_fastx::chunker::{split_records, RecordRange};

/// Number of complete records in `data`.
pub fn record_count(data: &[u8]) -> usize {
    data.iter().filter(|&&b| b == b'\n').count() / 4
}

/// `--pair-filter` decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairFilter {
    /// Discard the pair when either mate fails (cutadapt's default).
    Any,
    /// Discard the pair only when both mates fail.
    Both,
    /// Look only at read 1.
    First,
}

impl PairFilter {
    /// Whether the pair survives, given whether each mate passed the filters.
    pub fn keeps(self, first_ok: bool, second_ok: bool) -> bool {
        match self {
            PairFilter::Any => first_ok && second_ok,
            PairFilter::Both => first_ok || second_ok,
            PairFilter::First => first_ok,
        }
    }
}

/// Per-mate results for one pair.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MateResult {
    /// Serialized FASTQ/FASTA bytes for the mate (empty when filtered out).
    pub records: Vec<u8>,
    /// Number of records the mate's trimmer wrote.
    pub written: u64,
    /// Whether the mate passed every filter (independent of the pair decision).
    pub passed: bool,
    /// Statistics for the mate, with `records_out` reconciled to the pair
    /// decision by the caller.
    pub stats: ChunkStats,
}

/// The result of processing one group of pairs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupOutcome {
    /// Serialized read-1 records that survived the pair decision.
    pub read1: Vec<u8>,
    /// Serialized read-2 records that survived the pair decision.
    pub read2: Vec<u8>,
    /// Read-1 statistics for the report.
    pub stats1: ChunkStats,
    /// Read-2 statistics for the report.
    pub stats2: ChunkStats,
    /// Pairs whose mates did not line up (truncated input).
    pub improper: bool,
}

/// Run one mate's trimmer over a group of complete records.
///
/// `trimmer` must already be configured for that mate. `data` must contain only
/// complete records; `offset`/`line` locate it in the whole input for error
/// messages.
pub fn trim_group(
    trimmer: &mut Trimmer,
    data: &[u8],
    offset: u64,
    line: u64,
    minimum_length: usize,
) -> Result<(Vec<u8>, ChunkStats), crate::fastq::ParseError> {
    let mut out = Vec::with_capacity(data.len());
    let stats = trimmer.process_chunk(data, offset, line, minimum_length, &mut out)?;
    Ok((out, stats))
}

/// Decide whether a pair is written and reconcile the per-mate statistics.
///
/// `stats1.records_out`/`stats2.records_out` are set from the pair decision so
/// the report's "Pairs written" count matches the bytes actually emitted, and
/// `stats2.records_out` is `0` when read 2 is not written.
pub fn apply_pair_decision(
    filter: PairFilter,
    stats1: &mut ChunkStats,
    stats2: &mut ChunkStats,
    first_written: u64,
    second_written: u64,
) -> bool {
    let passed1 = stats1.records_out > 0 || first_written > 0;
    let passed2 = stats2.records_out > 0 || second_written > 0;
    let keep = filter.keeps(passed1, passed2);
    if !keep {
        stats1.records_out = 0;
        stats2.records_out = 0;
    }
    keep
}

/// The adapter number printed for read 2's first adapter.
///
/// cutadapt numbers adapters across both reads, so R2's adapters continue the
/// R1 sequence: with one R1 adapter the R2 section starts at `2`.
pub fn adapter_number_offset(read1_adapters: usize) -> usize {
    read1_adapters
}

/// Parse `data` far enough to count its records, reporting errors like the
/// single-end path does.
///
/// Used to validate that both mates of an interleaved or two-file input have the
/// same number of records before trimming starts.
pub fn count_records_checked(
    data: &[u8],
    offset: u64,
    line: u64,
) -> Result<usize, crate::fastq::ParseError> {
    let mut parser = Parser::new(data, offset, line);
    let mut count = 0usize;
    while parser.next_record()?.is_some() {
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, seq: &str) -> String {
        format!("@{name}\n{seq}\n+\n{}\n", "I".repeat(seq.len()))
    }

    // Verified with:
    //   cutadapt -a AGATCGGAAGAGC -A ACGTACGTACGTACGT -m 30 --pair-filter <f> \
    //       -o o1.fastq -p o2.fastq simple.fastq simple.r2.fastq
    // which reports 6, 1 and 5 too-short pairs for any/both/first.
    #[test]
    fn pair_filter_matches_oracle_counts() {
        // The 13 oracle pairs reduce to (R1 ok?, R2 ok?) verdicts; with -m 30
        // the failing pairs are distributed so that any=6, both=1, first=5.
        // Solving the oracle's counts: Any drops 6, Both drops 1, First drops 5
        // over 13 pairs gives 7 (ok,ok), 1 (ok,fail), 4 (fail,ok), 1 (fail,fail).
        let verdicts = [
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (true, false),
            (false, true),
            (false, true),
            (false, true),
            (false, true),
            (false, false),
        ];
        let count_dropped = |filter: PairFilter| {
            verdicts
                .iter()
                .filter(|(a, b)| !filter.keeps(*a, *b))
                .count()
        };
        assert_eq!(count_dropped(PairFilter::Any), 6);
        assert_eq!(count_dropped(PairFilter::Both), 1);
        assert_eq!(count_dropped(PairFilter::First), 5);
    }

    #[test]
    fn pair_filter_truth_table() {
        assert!(!PairFilter::Any.keeps(false, true));
        assert!(!PairFilter::Any.keeps(true, false));
        assert!(PairFilter::Any.keeps(true, true));

        assert!(PairFilter::Both.keeps(false, true));
        assert!(PairFilter::Both.keeps(true, false));
        assert!(!PairFilter::Both.keeps(false, false));

        assert!(!PairFilter::First.keeps(false, true));
        assert!(PairFilter::First.keeps(true, false));
    }

    #[test]
    fn split_records_keeps_lockstep_ranges() {
        let data = format!(
            "{}{}{}",
            rec("a", "ACGT"),
            rec("b", "ACGTACGT"),
            rec("c", "AA")
        );
        let ranges = split_records(data.as_bytes());
        assert_eq!(ranges.len(), 3);
        assert_eq!(record_count(data.as_bytes()), 3);
        assert_eq!(
            &data.as_bytes()[ranges[1].start..ranges[1].end],
            rec("b", "ACGTACGT").as_bytes()
        );
    }

    #[test]
    fn split_records_ignores_trailing_partial_record() {
        let data = format!("{}@d\nACGT\n+\n", rec("a", "ACGT"));
        let ranges = split_records(data.as_bytes());
        assert_eq!(ranges.len(), 1);
    }

    #[test]
    fn adapter_numbering_continues_across_reads() {
        assert_eq!(adapter_number_offset(1), 1);
        assert_eq!(adapter_number_offset(2), 2);
    }

    #[test]
    fn pair_decision_zeroes_both_counts_when_discarded() {
        let mut stats1 = ChunkStats {
            records_out: 1,
            ..ChunkStats::default()
        };
        let mut stats2 = ChunkStats {
            records_out: 1,
            ..ChunkStats::default()
        };
        let keep = apply_pair_decision(PairFilter::Any, &mut stats1, &mut stats2, 1, 1);
        assert!(keep);
        assert_eq!(stats1.records_out, 1);

        let mut stats1 = ChunkStats {
            records_out: 1,
            ..ChunkStats::default()
        };
        let mut stats2 = ChunkStats::default();
        let keep = apply_pair_decision(PairFilter::Any, &mut stats1, &mut stats2, 1, 0);
        assert!(!keep);
        assert_eq!(stats1.records_out, 0);
        assert_eq!(stats2.records_out, 0);
    }

    #[test]
    fn count_records_checked_reports_malformed_input() {
        let err = count_records_checked(b"@a\nACGT\n+\nII\n", 0, 1).unwrap_err();
        assert!(err.message.contains("lengths differ"), "{}", err.message);
    }
}
