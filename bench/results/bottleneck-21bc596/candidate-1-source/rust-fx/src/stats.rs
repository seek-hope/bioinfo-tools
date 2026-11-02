//! Per-record sequence statistics: length, GC, quality metrics, and the numbers
//! `stats`, `fx2tab` and `seq` need.
//!
//! # Definitions, with their source
//!
//! Definitions below are checked against pinned SeqKit 2.14.0. In particular,
//! its Q20/Q30 help says "greater than", but its executable and `stat.go` include
//! scores exactly at the threshold. Boundary regressions preserve that behavior.
//!
//! * **GC(%)** — `stats` uses total sequence length; `fx2tab -g` excludes
//!   ambiguous bases from the denominator.
//! * **AvgQual** — explicitly *not* the arithmetic mean of the quality scores.
//!   The documented procedure is: scores → error probabilities, mean, → back to a
//!   score (`seqkit` issue #448).
//! * **Q20(%)/Q30(%)** — percentage of bases with quality score at least 20/30.
//! * **sum_n** — count of `N`, `n`, `X`, `x`.
//! * **sum_gap** — count of the gap characters, default `- .` plus space and tab
//!   (`seqkit seq -g` documents `-G` default `"- \t."`). Disjoint from `sum_n`.
//! * **Q1/Q2/Q3** — quartiles of sequence length. The interpolation method is
//!   not documented; DR-3 records the choice made here.

use std::sync::LazyLock;

// Scores are bytes after saturating offset subtraction. Cache the exact same
// powf result for all 256 possibilities; keep additions in input order so the
// mean-error round trip retains its original floating-point rounding.
static ERROR_PROBABILITIES: LazyLock<[f64; 256]> =
    LazyLock::new(|| std::array::from_fn(|score| 10f64.powf(-(score as f64) / 10.0)));

/// Characters SeqKit counts as gaps by default (`-G` default `"- \t."`).
pub const DEFAULT_GAP_LETTERS: &[u8] = b"- \t.";

/// Characters counted by `sum_n`.
pub const AMBIGUOUS_LETTERS: &[u8] = b"NnXx";

/// The four DNA/RNA bases whose count forms the GC denominator.
fn is_gc_base(byte: u8) -> bool {
    matches!(byte.to_ascii_uppercase(), b'G' | b'C')
}

fn is_at_base(byte: u8) -> bool {
    matches!(byte.to_ascii_uppercase(), b'A' | b'T' | b'U')
}

/// Statistics accumulated over one sequence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeqStats {
    /// Number of bases, gaps included, as SeqKit counts them.
    pub length: u64,
    /// Bases that are `G` or `C` (case-insensitive).
    pub gc: u64,
    /// Bases that are `A`, `T` or `U` (case-insensitive).
    pub at: u64,
    /// Ambiguous letters (`N`, `n`, `X`, `x`).
    pub n_count: u64,
    /// Gap characters (`-`, space, tab, `.` by default).
    pub gap_count: u64,
}

impl SeqStats {
    /// Accumulate one sequence.
    pub fn from_sequence(sequence: &[u8], gap_letters: &[u8]) -> SeqStats {
        let mut gaps = [false; 256];
        for &byte in gap_letters {
            gaps[byte as usize] = true;
        }
        Self::from_gap_table(sequence, &gaps)
    }

    pub(crate) fn from_gap_table(sequence: &[u8], gaps: &[bool; 256]) -> SeqStats {
        let mut stats = SeqStats {
            length: sequence.len() as u64,
            ..SeqStats::default()
        };
        for &byte in sequence {
            // Boolean counters avoid unpredictable branches on mixed bases.
            // The byte table keeps arbitrary (including repeated) gap symbols
            // exact without searching the configured slice for every base.
            stats.gap_count += u64::from(gaps[byte as usize]);
            stats.n_count += u64::from(matches!(byte, b'N' | b'n' | b'X' | b'x'));
            stats.gc += u64::from(is_gc_base(byte));
            stats.at += u64::from(is_at_base(byte));
        }
        stats
    }

    /// `stats` `GC(%)`: `(G+C)/length`, or `None` when the length is zero.
    ///
    /// The denominator is the **total sequence length**, so `N`s and gap
    /// characters dilute the percentage. Verified against SeqKit 2.14.0:
    ///
    ///   GCGCAAAA      -> 50.00   (4/8)
    ///   GCGCAAAANN    -> 40.00   (4/10 -- the two Ns count in the denominator)
    ///   GCGCAAAA----  -> 33.33   (4/12 -- the gaps count too)
    ///   NNNN          ->  0.00   (no GC, but still defined)
    ///   ----          ->  0.00
    ///   (empty)       -> NaN
    ///
    /// Note this differs from `fx2tab -g`, which uses `(G+C)/(G+C+A+T)`; see
    /// `gc_percent_unambiguous`. Using one denominator for both was the bug that
    /// made `stats -a` report 51.72 where the oracle said 48.39.
    pub fn gc_percent(&self) -> Option<f64> {
        if self.length == 0 {
            None
        } else {
            Some(self.gc as f64 * 100.0 / self.length as f64)
        }
    }

    /// `fx2tab -g` GC: `(G+C)/(G+C+A+T)`, or `None` when there are no
    /// unambiguous bases.
    ///
    /// SeqKit prints `NaN` here for a sequence made only of `N`s or gaps, which
    /// is a different contract from `stats`'s `0.00` for the same input:
    ///
    ///   GCGCAAAANN    -> 50.00   (4/8, the Ns do NOT count)
    ///   NNNN          -> NaN
    ///   ----          -> NaN
    pub fn gc_percent_unambiguous(&self) -> Option<f64> {
        let denominator = self.gc + self.at;
        if denominator == 0 {
            None
        } else {
            Some(self.gc as f64 * 100.0 / denominator as f64)
        }
    }
}

// A lane contributes at most one per byte. Flush before any 16-bit lane can
// carry into its neighbour, including for arbitrarily long wrapped records.
const COUNT_CHUNK: usize = u16::MAX as usize;

/// Per-file integer contributions, packed into four independent 16-bit lanes.
/// Classification and arbitrary gap membership are computed once per byte value.
pub(crate) struct SequenceCounter {
    contributions: [u64; 256],
}

impl SequenceCounter {
    pub(crate) fn new(gap_letters: &[u8]) -> Self {
        let mut gaps = [false; 256];
        for &byte in gap_letters {
            gaps[byte as usize] = true;
        }
        Self {
            contributions: std::array::from_fn(|i| {
                let byte = i as u8;
                u64::from(is_gc_base(byte))
                    | (u64::from(is_at_base(byte)) << 16)
                    | (u64::from(matches!(byte, b'N' | b'n' | b'X' | b'x')) << 32)
                    | (u64::from(gaps[i]) << 48)
            }),
        }
    }

    pub(crate) fn stats(&self, sequence: &[u8]) -> SeqStats {
        let mut stats = SeqStats {
            length: sequence.len() as u64,
            ..SeqStats::default()
        };
        for chunk in sequence.chunks(COUNT_CHUNK) {
            let counts = chunk
                .iter()
                .fold(0u64, |sum, &byte| sum + self.contributions[byte as usize]);
            stats.gc += counts & 0xffff;
            stats.at += (counts >> 16) & 0xffff;
            stats.n_count += (counts >> 32) & 0xffff;
            stats.gap_count += counts >> 48;
        }
        stats
    }
}

/// Separate G and C counts, needed for GC-Skew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcSkewCounts {
    /// Number of `G`/`g`.
    pub g: u64,
    /// Number of `C`/`c`.
    pub c: u64,
}

impl GcSkewCounts {
    /// Count `G` and `C` in `sequence`.
    pub fn from_sequence(sequence: &[u8]) -> GcSkewCounts {
        let mut counts = GcSkewCounts::default();
        for &byte in sequence {
            match byte.to_ascii_uppercase() {
                b'G' => counts.g += 1,
                b'C' => counts.c += 1,
                _ => {}
            }
        }
        counts
    }

    /// `(G-C)/(G+C)`, or `None` when `G+C` is zero.
    pub fn skew(&self) -> Option<f64> {
        let total = self.g + self.c;
        if total == 0 {
            None
        } else {
            Some((self.g as f64 - self.c as f64) / total as f64)
        }
    }
}

/// Quality-based statistics for one read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QualStats {
    /// Total bases scored.
    pub bases: u64,
    /// Bases with a score at least 20.
    pub q20: u64,
    /// Bases with a score at least 30.
    pub q30: u64,
    /// Sum of error probabilities, for [`QualStats::average_quality`].
    error_sum: f64,
}

/// Per-file quality decoding: offset subtraction and probability lookup are
/// prepared once. Integer counts can be reduced separately; floating-point
/// additions still occur in their original per-base, per-record order.
pub(crate) struct QualityDecoder {
    probabilities: [f64; 256],
    counts: [u32; 256],
}

impl QualityDecoder {
    pub(crate) fn new(base: u8) -> Self {
        let probabilities = &*ERROR_PROBABILITIES;
        Self {
            probabilities: std::array::from_fn(|byte| {
                probabilities[(byte as u8).saturating_sub(base) as usize]
            }),
            counts: std::array::from_fn(|byte| {
                u32::from(byte as u16 >= base as u16 + 20)
                    | (u32::from(byte as u16 >= base as u16 + 30) << 16)
            }),
        }
    }

    pub(crate) fn stats(&self, qualities: &[u8]) -> QualStats {
        let mut stats = QualStats {
            bases: qualities.len() as u64,
            ..QualStats::default()
        };
        for chunk in qualities.chunks(COUNT_CHUNK) {
            let mut counts = 0u32;
            for &byte in chunk {
                counts += self.counts[byte as usize];
                // Keep a single accumulator across chunks: regrouping this sum
                // (including using histograms or multiplication) changes bits.
                stats.error_sum += self.probabilities[byte as usize];
            }
            stats.q20 += u64::from(counts & 0xffff);
            stats.q30 += u64::from(counts >> 16);
        }
        stats
    }
}

impl QualStats {
    /// Accumulate one quality string. `base` is the ASCII offset (33 for
    /// Phred+33).
    pub fn from_qualities(qualities: &[u8], base: u8) -> QualStats {
        let probabilities = &*ERROR_PROBABILITIES;
        let mut stats = QualStats {
            bases: qualities.len() as u64,
            ..QualStats::default()
        };
        for &byte in qualities {
            let score = byte.saturating_sub(base);
            if score >= 20 {
                stats.q20 += 1;
            }
            if score >= 30 {
                stats.q30 += 1;
            }
            // Convert the score back to an error probability, as SeqKit's
            // documented AvgQual procedure requires.
            stats.error_sum += probabilities[score as usize];
        }
        stats
    }

    /// `Q20(%)`.
    pub fn q20_percent(&self) -> Option<f64> {
        if self.bases == 0 {
            None
        } else {
            Some(self.q20 as f64 * 100.0 / self.bases as f64)
        }
    }

    /// `Q30(%)`.
    pub fn q30_percent(&self) -> Option<f64> {
        if self.bases == 0 {
            None
        } else {
            Some(self.q30 as f64 * 100.0 / self.bases as f64)
        }
    }

    /// `AvgQual`, as the documented mean-error round trip.
    ///
    /// `-10 * log10(mean(10^(-q/10)))`. Computed and stored incrementally so a
    /// whole file needs no per-read vector.
    pub fn average_quality(&self) -> Option<f64> {
        if self.bases == 0 {
            return None;
        }
        let mean_error = self.error_sum / self.bases as f64;
        Some(-10.0 * mean_error.log10())
    }
}

/// Merge another accumulator into this one (used to fold per-chunk results).
pub trait Merge {
    /// Absorb `other`.
    fn merge(&mut self, other: &Self);
}

impl Merge for SeqStats {
    fn merge(&mut self, other: &Self) {
        self.length += other.length;
        self.gc += other.gc;
        self.at += other.at;
        self.n_count += other.n_count;
        self.gap_count += other.gap_count;
    }
}

impl Merge for QualStats {
    fn merge(&mut self, other: &Self) {
        self.bases += other.bases;
        self.q20 += other.q20;
        self.q30 += other.q30;
        self.error_sum += other.error_sum;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_sequence_counts_match_scalar_across_lane_boundaries() {
        let all_bytes: Vec<u8> = (0..=255).collect();
        for gaps in [DEFAULT_GAP_LETTERS, b"GgNnUU", b"\0\xffGG", &all_bytes] {
            let counter = SequenceCounter::new(gaps);
            for length in [
                0,
                1,
                255,
                COUNT_CHUNK - 1,
                COUNT_CHUNK,
                COUNT_CHUNK + 1,
                2 * COUNT_CHUNK + 1,
            ] {
                let mixed: Vec<_> = (0..length).map(|i| i as u8).collect();
                assert_eq!(counter.stats(&mixed), SeqStats::from_sequence(&mixed, gaps));
                for byte in [b'G', b'n', b'U', 255] {
                    let repeated = vec![byte; length];
                    assert_eq!(
                        counter.stats(&repeated),
                        SeqStats::from_sequence(&repeated, gaps)
                    );
                }
            }
        }
    }

    #[test]
    fn packed_quality_counts_preserve_sum_bits_across_lane_boundaries() {
        for base in [0, 33, 64, 226, 236, 255] {
            let decoder = QualityDecoder::new(base);
            for length in [
                COUNT_CHUNK - 1,
                COUNT_CHUNK,
                COUNT_CHUNK + 1,
                2 * COUNT_CHUNK + 1,
            ] {
                for qualities in [
                    (0..length).map(|i| i as u8).collect::<Vec<_>>(),
                    vec![63; length],
                    vec![255; length],
                ] {
                    let expected = QualStats::from_qualities(&qualities, base);
                    let actual = decoder.stats(&qualities);
                    assert_eq!(
                        (actual.bases, actual.q20, actual.q30),
                        (expected.bases, expected.q20, expected.q30)
                    );
                    assert_eq!(actual.error_sum.to_bits(), expected.error_sum.to_bits());
                }
            }
        }
    }

    #[test]
    fn sequence_counts_cover_all_bytes_and_overlapping_gap_symbols() {
        let sequence: Vec<u8> = (0..=255).collect();
        for gaps in [DEFAULT_GAP_LETTERS, b"", b"AACCGNnx", &sequence[..]] {
            let stats = SeqStats::from_sequence(&sequence, gaps);
            assert_eq!(stats.length, 256);
            assert_eq!(stats.gc, 4);
            assert_eq!(stats.at, 6);
            assert_eq!(stats.n_count, 4);
            let distinct: std::collections::BTreeSet<u8> = gaps.iter().copied().collect();
            assert_eq!(stats.gap_count, distinct.len() as u64);
        }
    }

    #[test]
    fn prepared_quality_decoder_is_bitwise_identical_for_every_offset() {
        let ascending: Vec<u8> = (0..=255).collect();
        let descending: Vec<u8> = ascending.iter().rev().copied().collect();
        for base in 0..=255 {
            let decoder = QualityDecoder::new(base);
            for quality in [
                ascending.as_slice(),
                descending.as_slice(),
                b"",
                b"IIIIIIII55555555",
            ] {
                let expected = QualStats::from_qualities(quality, base);
                let actual = decoder.stats(quality);
                assert_eq!(
                    (actual.bases, actual.q20, actual.q30),
                    (expected.bases, expected.q20, expected.q30)
                );
                assert_eq!(actual.error_sum.to_bits(), expected.error_sum.to_bits());
            }
        }
    }

    #[test]
    fn cached_error_probabilities_preserve_every_score_and_sum_bitwise() {
        let ascending: Vec<u8> = (0..=255).collect();
        let descending: Vec<u8> = ascending.iter().copied().rev().collect();
        for base in [0, 33, 64, 255] {
            for qualities in [&ascending, &descending] {
                let expected: f64 = qualities
                    .iter()
                    .map(|byte| 10f64.powf(-(byte.saturating_sub(base) as f64) / 10.0))
                    .sum();
                let stats = QualStats::from_qualities(qualities, base);
                assert_eq!(stats.error_sum.to_bits(), expected.to_bits());
            }
            for byte in 0u8..=255 {
                let expected = 10f64.powf(-(byte.saturating_sub(base) as f64) / 10.0);
                let stats = QualStats::from_qualities(&[byte], base);
                assert_eq!(stats.error_sum.to_bits(), expected.to_bits());
            }
        }
    }

    #[test]
    fn stats_gc_uses_the_length_denominator() {
        // `stats` GC(%) is (G+C)/length, so N dilutes it: 2/10 = 20%.
        //
        // Verified against SeqKit 2.14.0, which prints 40.00 for GCGCAAAANN
        // (4/10). The earlier (G+C)/(G+C+A+T) formula gave 50% here and made
        // `stats -a` disagree with the oracle on every FASTQ containing an N.
        let stats = SeqStats::from_sequence(b"GCATNNNNNN", DEFAULT_GAP_LETTERS);
        assert_eq!(stats.gc, 2);
        assert_eq!(stats.at, 2);
        assert_eq!(stats.n_count, 6);
        assert_eq!(stats.gc_percent(), Some(20.0));
        assert_eq!(
            stats.gc_percent_unambiguous(),
            Some(50.0),
            "fx2tab -g keeps the unambiguous denominator"
        );
    }

    #[test]
    fn fx2tab_gc_uses_the_unambiguous_denominator() {
        // (G+C)/(G+C+A+T) = 4/8 = 50%, unlike stats' 4/10 = 40%.
        let stats = SeqStats::from_sequence(b"GCGCAAAANN", DEFAULT_GAP_LETTERS);
        assert_eq!(stats.gc_percent(), Some(40.0));
        assert_eq!(stats.gc_percent_unambiguous(), Some(50.0));
    }

    #[test]
    fn gc_is_case_insensitive() {
        assert_eq!(
            SeqStats::from_sequence(b"gcgc", DEFAULT_GAP_LETTERS).gc_percent(),
            Some(100.0)
        );
        assert_eq!(
            SeqStats::from_sequence(b"GCGC", DEFAULT_GAP_LETTERS).gc_percent(),
            Some(100.0)
        );
        assert_eq!(
            SeqStats::from_sequence(b"gCgC", DEFAULT_GAP_LETTERS).gc_percent(),
            Some(100.0)
        );
    }

    #[test]
    fn rna_uracil_counts_toward_the_denominator() {
        // U counts as an AT-class base so an RNA sequence reports a sensible GC.
        let stats = SeqStats::from_sequence(b"GCUU", DEFAULT_GAP_LETTERS);
        assert_eq!(stats.at, 2);
        assert_eq!(stats.gc_percent(), Some(50.0));
    }

    #[test]
    fn stats_gc_is_defined_without_unambiguous_bases_but_fx2tab_gc_is_not() {
        // SeqKit prints 0.00 from `stats` for an all-N sequence, but NaN from
        // `fx2tab -g`. Both behaviours are pinned here.
        let all_n = SeqStats::from_sequence(b"NNNN", DEFAULT_GAP_LETTERS);
        assert_eq!(all_n.gc_percent(), Some(0.0));
        assert_eq!(all_n.gc_percent_unambiguous(), None);

        // An empty sequence has no length denominator at all, so stats is NaN.
        let empty = SeqStats::from_sequence(b"", DEFAULT_GAP_LETTERS);
        assert_eq!(empty.gc_percent(), None);
        assert_eq!(empty.gc_percent_unambiguous(), None);
    }

    #[test]
    fn gaps_and_ambiguous_bases_are_disjoint() {
        let stats = SeqStats::from_sequence(b"AC-G TN.", DEFAULT_GAP_LETTERS);
        assert_eq!(stats.gap_count, 3, "dash, space, dot");
        assert_eq!(stats.n_count, 1, "N only");
        assert_eq!(stats.length, 8, "length counts gaps");
    }

    #[test]
    fn gap_letters_are_configurable() {
        // `-G Nn` makes N a gap. The two counts then both include it, which is
        // what SeqKit does when the user adds N to the gap set.
        let stats = SeqStats::from_sequence(b"ACNN-", b"Nn-");
        assert_eq!(stats.gap_count, 3);
        assert_eq!(stats.n_count, 2);
    }

    #[test]
    fn length_counts_every_byte_including_gaps() {
        let stats = SeqStats::from_sequence(b"AC--GT", DEFAULT_GAP_LETTERS);
        assert_eq!(stats.length, 6);
        assert_eq!(stats.gap_count, 2);
    }

    #[test]
    fn gc_skew_is_g_minus_c_over_g_plus_c() {
        let counts = GcSkewCounts::from_sequence(b"GGGC");
        assert_eq!(counts.g, 3);
        assert_eq!(counts.c, 1);
        assert_eq!(counts.skew(), Some(0.5));

        let balanced = GcSkewCounts::from_sequence(b"GGCC");
        assert_eq!(balanced.skew(), Some(0.0));

        let none = GcSkewCounts::from_sequence(b"ATAT");
        assert_eq!(none.skew(), None);
    }

    #[test]
    fn quality_percentages_include_threshold_scores() {
        // SeqKit 2.14.0 includes Q20 ('5') and Q30 ('?') at their thresholds.
        let stats = QualStats::from_qualities(b"II5?", 33);
        assert_eq!(stats.bases, 4);
        assert_eq!(stats.q20, 4);
        assert_eq!(stats.q30, 3);
        assert_eq!(stats.q20_percent(), Some(100.0));
        assert_eq!(stats.q30_percent(), Some(75.0));
    }

    #[test]
    fn average_quality_is_the_mean_error_round_trip() {
        // Uniform Q40: mean error = 1e-4, so AvgQual is exactly 40.
        let stats = QualStats::from_qualities(b"IIII", 33);
        let avg = stats.average_quality().unwrap();
        assert!((avg - 40.0).abs() < 1e-9, "got {avg}");

        // A mixture is NOT the arithmetic mean of the scores. Q0 and Q40 average
        // to 20 arithmetically, but the documented procedure gives ~3.01.
        let mixed = QualStats::from_qualities(b"!I", 33);
        let avg = mixed.average_quality().unwrap();
        let arithmetic = (0.0 + 40.0) / 2.0;
        assert!(
            (avg - arithmetic).abs() > 1.0,
            "AvgQual must not be the arithmetic mean: got {avg}, arithmetic {arithmetic}"
        );
        let expected = -10.0 * ((1.0f64 + 1e-4) / 2.0).log10();
        assert!(
            (avg - expected).abs() < 1e-9,
            "got {avg}, expected {expected}"
        );
    }

    #[test]
    fn empty_qualities_have_no_defined_averages() {
        let stats = QualStats::from_qualities(b"", 33);
        assert_eq!(stats.bases, 0);
        assert_eq!(stats.average_quality(), None);
        assert_eq!(stats.q20_percent(), None);
        assert_eq!(stats.q30_percent(), None);
    }

    #[test]
    fn phred64_shifts_the_scores() {
        // 'h' is 104; with base 64 the score is 40.
        let stats = QualStats::from_qualities(b"hhhh", 64);
        assert_eq!(stats.q30, 4);
        assert_eq!(stats.q20, 4);
    }

    #[test]
    fn scores_below_the_base_do_not_underflow() {
        // A quality byte below the base is invalid input; it must not panic.
        let stats = QualStats::from_qualities(b"\x00", 33);
        assert_eq!(stats.bases, 1);
        assert_eq!(stats.q20, 0);
    }

    #[test]
    fn merging_is_associative_for_both_accumulators() {
        let a = SeqStats::from_sequence(b"GCAT", DEFAULT_GAP_LETTERS);
        let b = SeqStats::from_sequence(b"NN--", DEFAULT_GAP_LETTERS);
        let mut merged = a.clone();
        merged.merge(&b);
        assert_eq!(merged.length, 8);
        assert_eq!(merged.gc, 2);
        assert_eq!(merged.at, 2);
        assert_eq!(merged.n_count, 2);
        assert_eq!(merged.gap_count, 2);

        let qa = QualStats::from_qualities(b"II", 33);
        let qb = QualStats::from_qualities(b"!!", 33);
        let mut qm = qa.clone();
        qm.merge(&qb);
        assert_eq!(qm.bases, 4);
        assert_eq!(qm.q30, 2);
        // Merging must give the same AvgQual as hashing all four at once.
        assert_eq!(
            qm.average_quality(),
            QualStats::from_qualities(b"II!!", 33).average_quality()
        );
    }

    // A pinned example so a refactor cannot silently change reported numbers.
    #[test]
    fn pinned_example_matches_a_hand_computation() {
        let sequence = b"AGATCGGAAGAGCACACGTCTGAACTCCAGTCAC";
        let stats = SeqStats::from_sequence(sequence, DEFAULT_GAP_LETTERS);
        assert_eq!(stats.length, 34);
        // A=11, C=10, G=8, T=5 -> GC = 18, GC+AT = 34
        assert_eq!(stats.gc, 18);
        assert_eq!(stats.at, 16);
        let gc = stats.gc_percent().unwrap();
        assert!((gc - 18.0 * 100.0 / 34.0).abs() < 1e-12, "got {gc}");
        assert_eq!(stats.n_count, 0);
        assert_eq!(stats.gap_count, 0);
    }
}
