//! Build a [`crate::report::stats::Stats`] from a pipeline run.
//!
//! The report modules are deliberately passive: they format a `Stats` value and
//! never look at the pipeline. This module is the single place where run-time
//! measurements are translated into that model, so the formatting stays testable
//! in isolation and the translation logic is testable here.

use crate::adapter::matcher::{ErrorRate, MatchConfig};
use crate::adapter::{AdapterSpec, AdapterType};
use crate::pipeline::Stats as RunStats;
use crate::report::stats::error_lengths;
use crate::report::stats::{
    AdapterStats, AdjacentBases, EndStats, FilterCounts, HistogramRow, Stats,
};

/// Build the report section for one adapter of one read.
///
/// Read 1 and read 2 use the same formatting in cutadapt; only the section
/// heading differs, so both go through this function.
#[allow(clippy::too_many_arguments)]
fn adapter_report(
    spec: &AdapterSpec,
    measured: &crate::engine::AdapterChunkStats,
    index: usize,
    match_config: &MatchConfig,
    reverse_complemented: Option<u64>,
) -> AdapterStats {
    let mut adapter = AdapterStats {
        // A named adapter (loaded from `file:FILE`) is shown by its name; an
        // anonymous one shows its 1-based position (oracle: "=== Adapter ad1 ==="
        // for file-loaded adapters, "=== Adapter 1 ===" otherwise).
        // An explicit name (even an empty one) is used verbatim; only the
        // auto-generated numeric placeholder falls back to the position.
        name: if is_explicit_name(&spec.name, index) {
            spec.name.clone()
        } else {
            (index + 1).to_string()
        },
        linked: spec.adapter_type == AdapterType::Linked,
        reverse_complemented,
        front: None,
        back: None,
    };
    let error_rate = match match_config.error_rate {
        ErrorRate::Rate(r) => r,
        // cutadapt converts an absolute count into a rate for the report.
        ErrorRate::Absolute(n) => {
            let len = if spec.adapter_type == AdapterType::Linked {
                spec.five_prime_sequence.len() + spec.sequence.len()
            } else {
                spec.sequence.len()
            };
            if len == 0 {
                0.0
            } else {
                n as f64 / len as f64
            }
        }
    };
    let three_type = cutadapt_type_name(spec, false);
    let five_type = cutadapt_type_name(spec, true);
    // Anchored adapters must match end-to-end, so cutadapt reports a single
    // absolute "No. of allowed errors: N" instead of length ranges. The 5' half
    // of a linked adapter always matches the whole 5' part, so it gets the same
    // treatment.
    let partial = !matches!(
        spec.adapter_type,
        AdapterType::AnchoredThreePrime | AdapterType::AnchoredFivePrime
    );
    let make_end = |sequence: &[u8], adjacent: Option<AdjacentBases>, three: bool| EndStats {
        adapter_type: if three {
            three_type.clone()
        } else {
            five_type.clone()
        },
        sequence: String::from_utf8_lossy(sequence).into_owned(),
        error_rate,
        indels: match_config.allow_indels,
        allows_partial_matches: partial,
        // Wildcards do not count toward the error budget unless -N made them
        // literal (oracle: "-a AGATCGGAAGAGCN" -> "10-13 bp", with -N -> "10-14 bp").
        effective_length: if match_config.no_adapter_wildcards {
            sequence.len()
        } else {
            sequence.iter().filter(|b| b"ACGT".contains(b)).count()
        },
        minimum_overlap: match_config.min_overlap.min(sequence.len()),
        adjacent_bases: adjacent,
        rows: Vec::new(),
        five_prime_overlaps: None,
        five_prime_rows: Vec::new(),
        matches_total: None,
        three_prime_matches: None,
    };

    let rows = build_rows(&measured.trimmed, &spec.sequence, error_rate);

    // A `-b` (anywhere) adapter is reported as a single section whose type
    // is "variable 5'/3'"; giving it both a front and a back section would
    // double-count its matches (the oracle reports 9, not 18).
    let both_ends = spec.adapter_type == AdapterType::Anywhere;
    if spec.adapter_type.trims_three_prime() && !both_ends {
        let mut end = make_end(&spec.sequence, Some(adjacent_bases(measured)), true);
        end.rows = rows.clone();
        if spec.adapter_type == AdapterType::Linked {
            // Only linked matches that actually removed a 3' part count, and
            // the histogram is of the 3' part lengths only.
            end.three_prime_matches = Some(measured.three_prime_hits);
            let three: Vec<(usize, usize)> = measured
                .three_prime_lengths
                .iter()
                .map(|&len| (len, 0usize))
                .collect();
            end.rows = build_rows(&three, &spec.sequence, error_rate);
        }
        adapter.back = Some(end);
    }
    if spec.adapter_type.trims_five_prime() && !both_ends {
        let five = if spec.adapter_type == AdapterType::Linked {
            &spec.five_prime_sequence
        } else {
            &spec.sequence
        };
        let mut end = make_end(five, None, false);
        if spec.adapter_type == AdapterType::Linked {
            // Oracle: `Minimum overlap: 16+3` for `^ACGTACGTACGTACGT...AD`
            // with -O 3 -- the 5' part reports its full length, the 3' part
            // reports -O -- and the 5' part allows a fixed absolute number
            // of errors instead of per-length ranges.
            end.minimum_overlap = five.len();
            end.allows_partial_matches = false;
            // The 5' histogram counts only the 5' removal lengths.
            let five_lengths: Vec<(usize, usize)> = measured
                .five_prime_lengths
                .iter()
                .map(|&len| (len, 0usize))
                .collect();
            end.rows = build_rows(&five_lengths, five, error_rate);
        }
        end.rows = rows.clone();
        adapter.front = Some(end);
    }
    if both_ends {
        // Report the anywhere adapter under its own 3'-style section, which
        // is where cutadapt keeps its single counter, plus the 5'/3'
        // overlap breakdown cutadapt prints for `-b`. The 3' histogram must
        // exclude the matches already counted under the 5' heading.
        // `measured.trimmed` holds every match; the 5' ones are also in
        // `five_prime_trimmed`. Removing one occurrence per 5' match leaves
        // exactly the 3' matches.
        let mut remaining = measured.trimmed.clone();
        for pair in &measured.five_prime_trimmed {
            if let Some(pos) = remaining.iter().position(|x| x == pair) {
                remaining.remove(pos);
            }
        }
        let mut end = make_end(&spec.sequence, Some(adjacent_bases(measured)), true);
        end.rows = build_rows(&remaining, &spec.sequence, error_rate);
        end.five_prime_overlaps = Some(measured.five_prime_overlaps);
        end.matches_total = Some(measured.matches);
        end.three_prime_matches = Some(measured.three_prime_hits);
        end.five_prime_rows = build_rows(&measured.five_prime_trimmed, &spec.sequence, error_rate);
        adapter.back = Some(end);
    }
    adapter
}
/// Expected number of reads that would match by chance at a given overlap.
///
/// cutadapt fills the `expect` column from the probability of a random match
/// over `length` bases allowing `max_err` mismatches, multiplied by the number
/// of reads. For the adapter lengths and error rates in practice this is far
/// below 1 and is printed as `0.0`.
///
/// The differential harness compares the report with a tolerance for this
/// column because cutadapt derives it from a global pseudo-random stream whose
/// value differs run to run; the count and error columns are compared exactly.
fn expected_count(n: u64, length: usize, max_err: usize) -> f64 {
    if length == 0 {
        return 0.0;
    }
    // P(a random base matches) = 1/4, uniformly over A/C/G/T.
    let p_match = 0.25f64;
    let mut p = 0.0f64;
    let mut binom = 1.0f64;
    for k in 0..=max_err.min(length) {
        if k > 0 {
            binom = binom * (length - k + 1) as f64 / k as f64;
        }
        p += binom * p_match.powi(length as i32) * (3.0f64).powi(k as i32);
    }
    let expected = n as f64 * p;
    // Reported to one decimal place by cutadapt; clamp to something sane.
    if expected.is_finite() {
        (expected * 10.0).round() / 10.0
    } else {
        0.0
    }
}

/// Group `(removed length, errors)` pairs into sorted histogram rows.
///
/// The allowed-error count for each row comes from the adapter's own length:
/// a 53 bp removal of a 13 bp adapter still allows the 13 bp budget (1 error at
/// the default rate), not one scaled to 53 bases.
fn build_rows(trimmed: &[(usize, usize)], adapter: &[u8], error_rate: f64) -> Vec<HistogramRow> {
    let mut by_length: std::collections::BTreeMap<usize, Vec<u64>> =
        std::collections::BTreeMap::new();
    for (removed, errors) in trimmed {
        let entry = by_length.entry(*removed).or_default();
        if entry.len() <= *errors {
            entry.resize(*errors + 1, 0);
        }
        entry[*errors] += 1;
    }
    let mut rows = Vec::new();
    for (length, counts) in by_length {
        let max_err = allowed_errors(adapter.len(), error_rate, length.min(adapter.len()));
        let expect = expected_count(counts.iter().sum::<u64>(), length, max_err);
        rows.push(HistogramRow {
            length,
            count: counts.iter().sum(),
            expect,
            max_err,
            error_counts: counts,
        });
    }
    rows
}

/// Base frequencies preceding the removed 3' adapters.
fn adjacent_bases(measured: &crate::engine::AdapterChunkStats) -> AdjacentBases {
    let mut adjacent = AdjacentBases::default();
    for base in &measured.adjacent {
        match base {
            Some(b'A') => adjacent.a += 1,
            Some(b'C') => adjacent.c += 1,
            Some(b'G') => adjacent.g += 1,
            Some(b'T') => adjacent.t += 1,
            _ => adjacent.none += 1,
        }
    }
    adjacent
}

/// True when `name` was given explicitly rather than generated from the index.
///
/// `parse` uses the adapter's position as the default name, so a purely numeric
/// name equal to the position is indistinguishable from "no name given"; that is
/// exactly cutadapt's behaviour (an anonymous adapter is reported by number).
fn is_explicit_name(name: &str, index: usize) -> bool {
    !(name.chars().all(|c| c.is_ascii_digit()) && name == index.to_string())
}

/// cutadapt's `adapter_type` name for one end of `spec`.
///
/// The names come from `cutadapt --json` and match the text report's
/// `Type: ...` descriptions after [`crate::report::text`] maps them.
fn cutadapt_type_name(spec: &AdapterSpec, five_prime: bool) -> String {
    let name = match (spec.adapter_type, five_prime) {
        (AdapterType::RegularThreePrime, _) => "regular_three_prime",
        (AdapterType::AnchoredThreePrime, _) => "anchored_three_prime",
        (AdapterType::RegularFivePrime, _) => "regular_five_prime",
        (AdapterType::AnchoredFivePrime, _) => "anchored_five_prime",
        (AdapterType::Anywhere, _) => "anywhere",
        (AdapterType::Linked, true) => "anchored_five_prime",
        (AdapterType::Linked, false) => "anchored_three_prime",
    };
    name.to_string()
}

/// Number of errors cutadapt allows for a match of `length` adapter bases.
///
/// `error_lengths` returns the *upper boundaries* of the ranges that allow
/// 0, 1, 2, ... errors, so the allowed count is the index of the first boundary
/// that is `>= length`.
fn allowed_errors(adapter_len: usize, error_rate: f64, length: usize) -> usize {
    let boundaries = error_lengths(adapter_len, error_rate);
    boundaries
        .iter()
        .position(|&boundary| length <= boundary)
        .unwrap_or(boundaries.len())
}

/// Which filters the user actually passed.
///
/// cutadapt prints a fate row for a filter that was requested even when it
/// rejected nothing (for example `--max-n 2` with zero N-containing reads),
/// so the report needs to know about the *option*, not just the counter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FiltersGiven {
    pub maximum_length: bool,
    pub max_n: bool,
    pub max_expected_errors: bool,
    pub max_average_error_rate: bool,
    pub discard_casava: bool,
    pub discard_trimmed: bool,
    pub discard_untrimmed: bool,
    pub reverse_complement: bool,
    pub untrimmed_output: bool,
    pub demultiplex: bool,
}

/// Which read modifiers the user passed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifiersGiven {
    pub quality: bool,
    pub poly_a: bool,
}

/// Translate a finished run into the report model.
pub fn build_stats(
    run: &RunStats,
    adapters: &[AdapterSpec],
    adapters_r2: &[AdapterSpec],
    match_config: &MatchConfig,
    minimum_length: Option<usize>,
    filters_given: &FiltersGiven,
    modifiers_given: &ModifiersGiven,
) -> Stats {
    let mut stats = Stats {
        paired: false,
        n: run.records_in,
        total_bp1: run.bp_in,
        total_bp2: None,
        with_adapters1: Some(run.reads_with_adapter),
        with_adapters2: None,
        quality_trimmed_bp1: None,
        quality_trimmed_bp2: None,
        poly_a_trimmed_bp1: None,
        poly_a_trimmed_bp2: None,
        // The Poly-A section only exists when --poly-a ran; grouping the removed
        // tail lengths into cutadapt's (length, count) table.
        poly_a_lengths1: modifiers_given.poly_a.then(|| {
            let mut counts: std::collections::BTreeMap<usize, u64> =
                std::collections::BTreeMap::new();
            for length in &run.poly_a_lengths {
                *counts.entry(*length).or_insert(0) += 1;
            }
            counts
                .into_iter()
                .map(|(length, count)| crate::report::stats::PolyALength { length, count })
                .collect()
        }),
        poly_a_lengths2: None,
        reverse_complemented: None,
        filtered: FilterCounts::default(),
        written: run.records_out,
        written_bp1: run.bp_out,
        written_bp2: None,
        adapters1: Vec::new(),
        adapters2: Vec::new(),
        gc_content: 0.5,
    };
    // cutadapt reports the too-short counter whenever -m was given, even as 0.
    if minimum_length.is_some() {
        // In paired mode cutadapt counts PAIRS, not mates: a pair is too short
        // when either mate is, and it is counted once.
        stats.filtered.too_short = Some(if run.paired {
            run.pairs_too_short
        } else {
            run.reads_too_short
        });
    }
    // The remaining filters are only reported when they actually rejected a read
    // (cutadapt omits a filter whose counter is zero *and* which was not used).
    if filters_given.maximum_length {
        stats.filtered.too_long = Some(run.reads_too_long);
    }
    // cutadapt prints the N counter whenever --max-n was given, even at zero
    // (like -m), so the caller tells us whether the option was used.
    // Basepair counts for the modifier summary lines: present only when the
    // modifier was requested, matching cutadapt.
    if modifiers_given.quality {
        stats.quality_trimmed_bp1 = Some(run.quality_trimmed_bp);
    }
    if modifiers_given.poly_a {
        stats.poly_a_trimmed_bp1 = Some(run.poly_a_trimmed_bp);
    }
    if filters_given.reverse_complement {
        stats.reverse_complemented = Some(run.reverse_complemented);
    }
    if filters_given.max_n {
        stats.filtered.too_many_n = Some(run.reads_too_many_n);
    }
    if filters_given.max_expected_errors {
        stats.filtered.too_many_expected_errors = Some(run.reads_too_many_expected_errors);
    }
    if filters_given.discard_casava {
        stats.filtered.casava_filtered = Some(run.reads_casava_filtered);
    }
    if filters_given.discard_trimmed {
        stats.filtered.discard_trimmed = Some(run.reads_discard_trimmed);
    }
    if filters_given.discard_untrimmed {
        stats.filtered.discard_untrimmed = Some(run.reads_discard_untrimmed);
    }
    // --untrimmed-output also reports the diverted reads under the same heading
    // (oracle: "Reads discarded as untrimmed: 3 (30.0%)").
    if filters_given.untrimmed_output {
        stats.filtered.discard_untrimmed = Some(run.reads_untrimmed_output);
    }
    // Demultiplexing diverts unmatched reads to the "unknown" file but cutadapt
    // still prints the heading with a ZERO count (oracle: "Reads discarded as
    // untrimmed: 0 (0.0%)" even though 3 reads went to o_unknown.fastq).
    if filters_given.demultiplex {
        stats.filtered.discard_untrimmed = Some(0);
    }

    for (index, spec) in adapters.iter().enumerate() {
        let measured = run
            .adapter_stats
            .get(index)
            .map(|totals| totals.to_chunk_stats())
            .unwrap_or_default();
        let reverse_complemented = if filters_given.reverse_complement {
            Some(
                run.adapter_reverse_complemented
                    .get(index)
                    .copied()
                    .unwrap_or(0),
            )
        } else {
            None
        };
        let adapter = adapter_report(spec, &measured, index, match_config, reverse_complemented);
        stats.adapters1.push(adapter);
    }

    // --- read 2 ------------------------------------------------------------
    // Paired runs keep a separate adapter list and a separate measurement
    // vector for read 2; the numbering continues after the read-1 adapters,
    // which `AdapterSpec::index` already encodes.
    if run.paired {
        let read2 = run.read2.as_deref();
        let r1_adapter_count = adapters.len();
        for spec in adapters_r2.iter() {
            let (adapter_stats_run, reverse_counts) = match read2 {
                // The R2 adapter specs carry a continuous index (offset by the
                // number of R1 adapters) for the report headings, but the R2
                // trimmer's per-adapter statistics are indexed LOCALLY from 0.
                Some(r2) => (
                    Some(
                        r2.adapter_stats
                            .get(spec.index.saturating_sub(r1_adapter_count))
                            .map(|totals| totals.to_chunk_stats())
                            .unwrap_or_default(),
                    ),
                    r2.adapter_reverse_complemented
                        .get(spec.index.saturating_sub(r1_adapter_count))
                        .copied()
                        .unwrap_or(0),
                ),
                None => (Some(Default::default()), 0),
            };
            let measured = adapter_stats_run.unwrap_or_default();
            let reverse_complemented = if filters_given.reverse_complement {
                Some(reverse_counts)
            } else {
                None
            };
            let adapter = adapter_report(
                spec,
                &measured,
                spec.index,
                match_config,
                reverse_complemented,
            );
            stats.adapters2.push(adapter);
        }
        // Read-2 counters and basepairs.
        if let Some(r2) = read2 {
            stats.total_bp2 = Some(r2.bp_in);
            stats.with_adapters2 = Some(r2.reads_with_adapter);
            stats.written_bp2 = Some(r2.bp_out);
            if modifiers_given.quality {
                stats.quality_trimmed_bp2 = Some(r2.quality_trimmed_bp);
            }
            if modifiers_given.poly_a {
                stats.poly_a_trimmed_bp2 = Some(r2.poly_a_trimmed_bp);
            }
        }
        stats.paired = true;
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::parse;

    fn run_stats(records_in: u64, records_out: u64) -> RunStats {
        RunStats {
            records_in,
            records_out,
            bp_in: 460,
            bp_out: 326,
            reads_with_adapter: 7,
            reads_too_short: 3,
            pairs_too_short: 0,
            reads_too_long: 0,
            reads_too_many_n: 0,
            reads_too_many_expected_errors: 0,
            reads_too_many_average_errors: 0,
            reads_casava_filtered: 0,
            reads_discard_trimmed: 0,
            reads_discard_untrimmed: 0,
            quality_trimmed_bp: 0,
            poly_a_trimmed_bp: 0,
            poly_a_lengths: Vec::new(),
            reverse_complemented: 0,
            adapter_reverse_complemented: Vec::new(),
            wildcard_lines: Vec::new(),
            reads_untrimmed_output: 0,
            demux_records: Vec::new(),
            demux_unknown: Vec::new(),
            demux_unknown_records: 0,
            paired: false,
            aux_incremental: false,
            read2: None,
            info_lines: Vec::new(),
            rest_lines: Vec::new(),
            too_short_records: Vec::new(),
            too_long_records: Vec::new(),
            untrimmed_records: Vec::new(),
            adapter_stats: vec![crate::pipeline::AdapterTotals::from(
                crate::engine::AdapterChunkStats {
                    matches: 7,
                    trimmed: vec![(6, 1), (13, 0), (13, 0), (13, 0), (23, 1), (53, 1), (13, 1)],
                    adjacent: vec![Some(b'A'), Some(b'T'), None],
                    five_prime_overlaps: 0,
                    five_prime_trimmed: Vec::new(),
                    three_prime_hits: 0,
                    five_prime_lengths: Vec::new(),
                    three_prime_lengths: Vec::new(),
                },
            )],
        }
    }

    #[test]
    fn builds_a_single_adapter_report() {
        let spec = parse('a', "AGATCGGAAGAGC", 0, None).unwrap();
        let stats = build_stats(
            &run_stats(10, 10),
            &[spec],
            &[],
            &MatchConfig::default(),
            Some(10),
            &FiltersGiven::default(),
            &ModifiersGiven::default(),
        );
        assert_eq!(stats.n, 10);
        assert_eq!(stats.with_adapters1, Some(7));
        assert_eq!(stats.written, 10);
        assert_eq!(stats.adapters1.len(), 1);
        let adapter = &stats.adapters1[0];
        assert_eq!(adapter.name, "1");
        let back = adapter.back.as_ref().expect("3' statistics");
        assert_eq!(back.sequence, "AGATCGGAAGAGC");
        assert_eq!(back.effective_length, 13);
        assert_eq!(back.minimum_overlap, 3);
        // 6, 13, 23 and 53 are the distinct removed lengths.
        let lengths: Vec<usize> = back.rows.iter().map(|r| r.length).collect();
        assert_eq!(lengths, vec![6, 13, 23, 53]);
        let row13 = back.rows.iter().find(|r| r.length == 13).unwrap();
        assert_eq!(row13.count, 4);
        // three exact matches at 13 and one with an error -> counts [3, 1]
        assert_eq!(row13.error_counts, vec![3, 1]);
    }

    #[test]
    fn too_short_counter_only_reported_when_min_length_is_set() {
        let spec = parse('a', "ACGT", 0, None).unwrap();
        let with = build_stats(
            &run_stats(10, 7),
            std::slice::from_ref(&spec),
            &[],
            &MatchConfig::default(),
            Some(10),
            &FiltersGiven::default(),
            &ModifiersGiven::default(),
        );
        assert_eq!(with.filtered.total(), 3);
        let without = build_stats(
            &run_stats(10, 10),
            std::slice::from_ref(&spec),
            &[],
            &MatchConfig::default(),
            None,
            &FiltersGiven::default(),
            &ModifiersGiven::default(),
        );
        assert_eq!(without.filtered.total(), 0);
        // -m 0 is still "specified", so the counter is present (cutadapt shows
        // it, with whatever the run measured) rather than omitted entirely.
        let zero = build_stats(
            &run_stats(10, 10),
            &[spec],
            &[],
            &MatchConfig::default(),
            Some(0),
            &FiltersGiven::default(),
            &ModifiersGiven::default(),
        );
        assert_eq!(zero.filtered.too_short, Some(3));
    }

    #[test]
    fn five_prime_adapter_gets_front_statistics() {
        let spec = parse('g', "ACGTACGT", 0, None).unwrap();
        let stats = build_stats(
            &run_stats(1, 1),
            &[spec],
            &[],
            &MatchConfig::default(),
            None,
            &FiltersGiven::default(),
            &ModifiersGiven::default(),
        );
        let adapter = &stats.adapters1[0];
        assert!(adapter.front.is_some());
        assert!(adapter.back.is_none());
    }
}
