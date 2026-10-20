//! cutadapt 4.6 text reports (`--report full` and `--report minimal`).
//!
//! This is a port of `cutadapt/report.py` (`full_report`, `minimal_report`,
//! `format_filter_report`, `histogram`, `AdjacentBaseStatistics`). The
//! functions are pure: they take [`Stats`] and return a `String`.
//!
//! # What is *not* included
//!
//! cutadapt prints three lines before the report body that are inherently
//! machine-dependent and are normalised away by the differential harness:
//!
//! ```text
//! This is cutadapt 4.6 with Python 3.14.8
//! Command line parameters: -a AGATCGGAAGAGC -o out.fastq in.fastq
//! Processing single-end reads on 1 core ...
//! ```
//!
//! and the first line of the body, `Finished in ... s (...)`, which contains
//! timing. [`full_report_with_elapsed`] can reproduce `Finished in ...` so a
//! caller that measures the elapsed time can emit cutadapt's exact line;
//! [`full_report`] omits it and therefore starts at cutadapt's
//! `=== Summary ===` heading. Everything from there to the end of the report is
//! byte-identical.
//!
//! `--report minimal` is fully reproducible: it contains no timing at all.

use crate::report::fmt::{comma_u64, fixed1_upper, percent_1};
use crate::report::stats::{AdapterStats, AdjacentBases, EndStats, Stats};

/// Description column text for each filter, in cutadapt's `FILTERS` order.
const FILTER_DESCRIPTIONS: [(&str, &str); 7] = [
    ("too_short", "that were too short"),
    ("too_long", "that were too long"),
    ("too_many_n", "with too many N"),
    ("too_many_expected_errors", "with too many exp. errors"),
    ("casava_filtered", "failed CASAVA filter"),
    ("discard_trimmed", "discarded as trimmed"),
    ("discard_untrimmed", "discarded as untrimmed"),
];

/// Render the full report *without* the `Finished in ...` timing line.
///
/// The result starts with cutadapt's `=== Summary ===` block when reads were
/// processed, or is exactly `"No reads processed!"` when `stats.n == 0`
/// (cutadapt returns that string before printing anything else).
pub fn full_report(stats: &Stats) -> String {
    if stats.n == 0 {
        return "No reads processed!".to_string();
    }
    let mut out = String::new();
    render_body(stats, &mut out);
    out
}

/// Render the full report including cutadapt's timed first line.
///
/// `elapsed` is the run time in seconds. cutadapt substitutes `1e-6` when the
/// elapsed time is exactly zero, which is reproduced here.
pub fn full_report_with_elapsed(stats: &Stats, elapsed: f64) -> String {
    if stats.n == 0 {
        return "No reads processed!".to_string();
    }
    let time = if elapsed == 0.0 { 1e-6 } else { elapsed };
    let mut out = String::new();
    // "Finished in {:.3F} s ({:.3F} {}s/read; {:.2F} M reads/minute)."
    out.push_str(&format!(
        "Finished in {:.3} s ({:.3} {}s/read; {:.2} M reads/minute).\n",
        time,
        MICRO_FACTOR * time / stats.n as f64,
        MICRO,
        stats.n as f64 / time * 60.0 / 1e6
    ));
    render_body(stats, &mut out);
    out
}

/// cutadapt's `MICRO` prefix (µ).
const MICRO: &str = "µ";
/// `1e6 * time` in the per-read conversion.
const MICRO_FACTOR: f64 = 1e6;

fn render_body(stats: &Stats, out: &mut String) {
    // cutadapt's report literal starts with "\n=== Summary ==="; the newline
    // that precedes it is consumed by print() after the "Finished in ..." line,
    // so the report body itself begins at the heading.
    out.push_str("=== Summary ===\n\n");
    if stats.paired {
        out.push_str(&format!(
            "Total read pairs processed:      {}\n",
            comma_u64(stats.n)
        ));
        for (i, with_adapters) in [stats.with_adapters1, stats.with_adapters2]
            .into_iter()
            .enumerate()
        {
            if let Some(count) = with_adapters {
                out.push_str(&format!(
                    "  Read {} with adapter:           {} ({})\n",
                    i + 1,
                    comma_u64(count),
                    percent_1(stats.with_adapters_fraction(i))
                ));
            }
        }
    } else {
        out.push_str(&format!(
            "Total reads processed:           {}\n",
            comma_u64(stats.n)
        ));
        if let Some(count) = stats.with_adapters1 {
            out.push_str(&format!(
                "Reads with adapters:             {} ({})\n",
                comma_u64(count),
                percent_1(stats.with_adapters_fraction(0))
            ));
        }
    }

    if let Some(reverse_complemented) = stats.reverse_complemented {
        out.push_str(&format!(
            "Reverse-complemented:            {} ({})\n",
            comma_u64(reverse_complemented),
            percent_1(stats.reverse_complemented_fraction())
        ));
    }

    let filter_report = format_filter_report(stats);
    if !filter_report.is_empty() {
        out.push_str("\n== Read fate breakdown ==\n");
        out.push_str(&filter_report);
    }

    let pairs_or_reads = if stats.paired { "Pairs" } else { "Reads" };
    out.push_str(&format!(
        "{pairs_or_reads} written (passing filters): {} ({})\n",
        comma_u64(stats.written),
        percent_1(stats.written_fraction())
    ));
    out.push('\n');
    out.push_str(&format!(
        "Total basepairs processed: {} bp\n",
        comma_u64(stats.total_bp())
    ));
    if stats.paired {
        out.push_str(&format!("  Read 1: {} bp\n", comma_u64(stats.total_bp1)));
        out.push_str(&format!(
            "  Read 2: {} bp\n",
            comma_u64(stats.total_bp2.unwrap_or(0))
        ));
    }

    if let Some(quality_trimmed) = stats.quality_trimmed() {
        out.push_str(&format!(
            "Quality-trimmed:           {} bp ({})\n",
            comma_u64(quality_trimmed),
            percent_1(stats.quality_trimmed_fraction())
        ));
        if stats.paired {
            for (i, value) in [stats.quality_trimmed_bp1, stats.quality_trimmed_bp2]
                .into_iter()
                .enumerate()
            {
                if let Some(value) = value {
                    out.push_str(&format!("  Read {}: {} bp\n", i + 1, comma_u64(value)));
                }
            }
        }
    }

    if let Some(poly_a_trimmed) = stats.poly_a_trimmed() {
        out.push_str(&format!(
            "Poly-A-trimmed:            {} bp ({})\n",
            comma_u64(poly_a_trimmed),
            percent_1(stats.poly_a_trimmed_fraction())
        ));
        if stats.paired {
            for (i, value) in [stats.poly_a_trimmed_bp1, stats.poly_a_trimmed_bp2]
                .into_iter()
                .enumerate()
            {
                if let Some(value) = value {
                    out.push_str(&format!("  Read {}: {} bp\n", i + 1, comma_u64(value)));
                }
            }
        }
    }

    out.push_str(&format!(
        "Total written (filtered):  {} bp ({})\n",
        comma_u64(stats.total_written_bp()),
        percent_1(stats.total_written_bp_fraction())
    ));
    if stats.paired {
        out.push_str(&format!("  Read 1: {} bp\n", comma_u64(stats.written_bp1)));
        out.push_str(&format!(
            "  Read 2: {} bp\n",
            comma_u64(stats.written_bp2.unwrap_or(0))
        ));
    }

    // cutadapt prints the summary block with print_s(), which appends a
    // newline; that produces the blank line before the first adapter section.
    out.push('\n');

    let mut warning = false;
    let mut rendered_any = false;
    let mut previous_was_zero_match = false;
    for which_in_pair in 0..2 {
        let adapter_stats = if which_in_pair == 0 {
            &stats.adapters1
        } else {
            &stats.adapters2
        };
        for adapter in adapter_stats {
            // A zero-match section ends after its sequence line with NO blank
            // line, so the following section must supply the separator itself.
            // Otherwise the previous section's own trailing blank already
            // separates them, at the R1/R2 boundary too.
            let needs_separator = rendered_any
                && (which_in_pair == 0 || previous_was_zero_match || stats.adapters1.len() == 1);
            if needs_separator {
                out.push('\n');
            }
            let total_before = adapter_total(adapter);
            warning |= render_adapter(stats, adapter, which_in_pair, out);
            previous_was_zero_match = total_before == 0;
            rendered_any = true;
        }
        render_poly_a(stats, which_in_pair, out);
    }

    if warning {
        out.push_str("WARNING:\n");
        out.push_str("    One or more of your adapter sequences may be incomplete.\n");
        out.push_str("    Please see the detailed output above.\n");
    }
    // Each block is emitted with print_s(), which appends a newline. The
    // histogram already ends with a blank line, so the report ends with exactly
    // one newline after its last line (verified with `od -c` against the
    // oracle). Only the tail is collapsed: blank lines *between* blocks (for
    // example before the Poly-A section) are part of cutadapt's output.
    while out.ends_with("\n\n") {
        out.pop();
    }
}

fn render_adapter(
    stats: &Stats,
    adapter: &AdapterStats,
    which_in_pair: usize,
    out: &mut String,
) -> bool {
    let extra = if stats.paired {
        if which_in_pair == 0 {
            "First read: "
        } else {
            "Second read: "
        }
    } else {
        ""
    };
    let total = adapter.total_matches();

    out.push_str(&format!("=== {extra}Adapter {} ===\n\n", adapter.name));

    if adapter.linked {
        // Linked adapters are reported with both sequences; `sequence` and
        // `effective_length` of the two ends are used here.
        let front = adapter.front.as_ref().expect("linked adapter has a 5' end");
        let back = adapter.back.as_ref().expect("linked adapter has a 3' end");
        out.push_str(&format!(
            "Sequence: {}...{}; Type: linked; Length: {}+{}; 5' trimmed: {} times; 3' trimmed: {} times",
            front.sequence,
            back.sequence,
            front.sequence.len(),
            back.sequence.len(),
            front.total(),
            // A linked match that removed only the 5' part must not count as a
            // 3' trim; the explicit counter records the real figure.
            back.three_prime_matches.unwrap_or_else(|| back.total())
        ));
    } else {
        // SingleAdapter / AnywhereAdapter share this line; the type string comes
        // from cutadapt's `descriptive_identifier()`.
        let end = adapter
            .back
            .as_ref()
            .or(adapter.front.as_ref())
            .expect("adapter has at least one end");
        out.push_str(&format!(
            "Sequence: {}; Type: {}; Length: {}; Trimmed: {} times",
            end.sequence,
            description(end.adapter_type.as_str()),
            end.sequence.len(),
            total
        ));
        // `-b` (anywhere) adapters additionally split the count by which end of
        // the read the match overlapped.
        if let Some(five_overlaps) = end.five_prime_overlaps {
            out.push_str(&format!(
                "\n{} times, it overlapped the 5' end of a read",
                five_overlaps
            ));
            out.push_str(&format!(
                "\n{} times, it overlapped the 3' end or was within the read",
                end.total().saturating_sub(five_overlaps)
            ));
        }
    }
    match stats.reverse_complemented {
        Some(_) => out.push_str(&format!(
            "; Reverse-complemented: {} times\n",
            adapter.reverse_complemented.unwrap_or(0)
        )),
        None => out.push('\n'),
    }
    if total == 0 {
        // Oracle: a section with no matches ends after its single sequence line
        // plus one blank line, so nothing extra is appended here.
        return false;
    }

    match (adapter.front.as_ref(), adapter.back.as_ref()) {
        (Some(front), Some(back)) if adapter.linked => {
            out.push('\n');
            out.push_str(&format!(
                "Minimum overlap: {}+{}\n",
                min_overlap(front),
                min_overlap(back)
            ));
            // Oracle layout for linked adapters: the 5' part's absolute error
            // count follows "No. of allowed errors:" directly, then a blank line,
            // then the 3' part's length ranges under a second heading.
            out.push_str("No. of allowed errors:");
            out.push_str(&front.error_ranges_text());
            out.push('\n');
            out.push_str("No. of allowed errors:");
            out.push_str(&back.error_ranges_text());
            out.push('\n');
            out.push_str("Overview of removed sequences at 5' end\n");
            out.push_str(&histogram(front, stats.n, stats.gc_content));
            out.push('\n');
            out.push('\n');
            out.push_str("Overview of removed sequences at 3' end\n");
            out.push_str(&histogram(back, stats.n, stats.gc_content));
            false
        }
        (Some(front), _) => {
            // FrontAdapterStatistics / the 5' half of an anywhere adapter.
            out.push('\n');
            if front.allows_partial_matches {
                out.push_str(&format!("Minimum overlap: {}\n", min_overlap(front)));
            }
            out.push_str("No. of allowed errors:");
            out.push_str(&front.error_ranges_text());
            out.push('\n');
            out.push_str("Overview of removed sequences\n");
            out.push_str(&histogram(front, stats.n, stats.gc_content));
            false
        }
        (None, Some(back)) => {
            // BackAdapterStatistics.
            out.push('\n');
            if back.allows_partial_matches {
                out.push_str(&format!("Minimum overlap: {}\n", min_overlap(back)));
            }
            out.push_str("No. of allowed errors:");
            out.push_str(&back.error_ranges_text());
            out.push('\n');
            if back.five_prime_overlaps.is_some() {
                // `-b` (anywhere) adapters are reported with the sequence split
                // by which end of the read the match overlapped, and without the
                // "Bases preceding removed adapters" block.
                out.push_str("Overview of removed sequences (5')\n");
                out.push_str(&histogram_rows(&back.five_prime_rows, stats.n));
                out.push_str("\n\n\nOverview of removed sequences (3' or within)\n");
                out.push_str(&histogram_rows(&back.rows, stats.n));
                return false;
            }
            let bases = back.adjacent_bases.clone().unwrap_or_default();
            let mut warned = false;
            let base_text = render_adjacent_bases(&bases, &mut warned);
            out.push_str(&base_text);
            if !base_text.is_empty() {
                // print_s(base_stats) appends a newline to the block.
                out.push('\n');
            }
            out.push_str("Overview of removed sequences\n");
            out.push_str(&histogram(back, stats.n, stats.gc_content));
            warned
        }
        (None, None) => false,
    }
}

/// Adapter `min_overlap`. `EndStats` does not carry it, so it is taken from the
/// effective length when the caller did not supply it explicitly; pipelines set
/// `minimum_overlap` through [`EndStats::adapter_type`]-independent fields.
///
/// Note: see [`minimum_overlap`] — reported for clarity because cutadapt prints
/// this literal value.
fn min_overlap(end: &EndStats) -> usize {
    end.minimum_overlap
}

/// Adapter type identifiers as printed in the text report.
fn description(adapter_type: &str) -> &str {
    match adapter_type {
        "regular_five_prime" => "regular 5'",
        "regular_three_prime" => "regular 3'",
        "anywhere" => "variable 5'/3'",
        "noninternal_five_prime" => "non-internal 5'",
        "noninternal_three_prime" => "non-internal 3'",
        "anchored_five_prime" => "anchored 5'",
        "anchored_three_prime" => "anchored 3'",
        "rightmost_five_prime" => "rightmost 5'",
        other => other,
    }
}

/// Total matches of one adapter section.
fn adapter_total(adapter: &crate::report::stats::AdapterStats) -> u64 {
    adapter
        .back
        .as_ref()
        .map_or(0, |e| e.total())
        .max(adapter.front.as_ref().map_or(0, |e| e.total()))
}

fn render_poly_a(stats: &Stats, which_in_pair: usize, out: &mut String) {
    let lengths = if which_in_pair == 0 {
        stats.poly_a_lengths1.as_ref()
    } else {
        stats.poly_a_lengths2.as_ref()
    };
    let Some(lengths) = lengths else {
        return;
    };
    let title = if !stats.paired {
        "Poly-A"
    } else if which_in_pair == 0 {
        "R1 poly-A"
    } else {
        "R2 poly-A"
    };
    // The adapter histogram above already ends with a blank line, so only one
    // newline is added before this heading (oracle: a single blank line).
    out.push('\n');
    out.push_str(&format!("=== {title} trimmed ===\n\n"));
    out.push_str("length\tcount\n");
    for entry in lengths {
        out.push_str(&format!("{}\t{}\n", entry.length, entry.count));
    }
    // poly_a_report() ends with "\n" and print_s() adds another one; the tail
    // collapse below removes one, leaving exactly one trailing newline.
    out.push('\n');
}

/// Render a histogram table from precomputed rows.
fn histogram_rows(rows: &[crate::report::stats::HistogramRow], _n: u64) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("length\tcount\texpect\tmax.err\terror counts\n");
    for row in rows {
        out.push_str(&format!(
            "{}\t{}\t{:.1}\t{}\t{}\n",
            row.length,
            row.count,
            row.expect,
            row.max_err,
            row.error_counts
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    out
}

/// cutadapt's `AdjacentBaseStatistics.__str__`.
///
/// Returns the block including its trailing newlines; `warned` is set when the
/// "preceded by ... extremely often" warning is printed.
fn render_adjacent_bases(bases: &AdjacentBases, warned: &mut bool) -> String {
    let mut out = String::new();
    let total = bases.total();
    if total == 0 {
        return out;
    }
    out.push_str("Bases preceding removed adapters:\n");
    for (label, count) in bases.iter_report() {
        let text = if label.is_empty() {
            "none/other"
        } else {
            label
        };
        let fraction = count as f64 / total as f64;
        out.push_str(&format!("  {text}: {}\n", percent_1(fraction)));
    }
    if let Some(base) = bases.warnbase() {
        *warned = true;
        out.push_str("WARNING:\n");
        out.push_str(&format!(
            "    The adapter is preceded by '{base}' extremely often.\n"
        ));
        out.push_str("    The provided adapter sequence could be incomplete at its 5' end.\n");
        out.push_str("    Ignore this warning when trimming primers.\n");
    }
    out
}

/// cutadapt's `format_filter_report`.
/// True when one base dominates the bases preceding removed adapters.
///
/// cutadapt warns when a single base accounts for the large majority of the
/// observed neighbourhoods, which usually means the adapter sequence is wrong.
fn has_warnbase(stats: &Stats) -> bool {
    for adapter in &stats.adapters1 {
        if let Some(back) = &adapter.back {
            if let Some(bases) = &back.adjacent_bases {
                let total = bases.total();
                if total == 0 {
                    continue;
                }
                let highest = bases.a.max(bases.c).max(bases.g).max(bases.t);
                if (highest as f64) / (total as f64) * 100.0 >= 90.0 {
                    return true;
                }
            }
        }
    }
    false
}

fn format_filter_report(stats: &Stats) -> String {
    let pairs_or_reads = if stats.paired { "Pairs" } else { "Reads" };
    let mut out = String::new();
    for (name, description) in FILTER_DESCRIPTIONS {
        if stats.filtered.get(name).is_none() {
            continue;
        }
        let value = stats.filtered.get(name).unwrap_or(0);
        let fraction = stats.filtered_fraction(name);
        // "Reads " + (description + ":").ljust(27) + value + fraction
        let mut label = String::from(description);
        label.push(':');
        out.push_str(&format!(
            "{pairs_or_reads} {label:<27}{} ({})\n",
            comma_u64(value),
            percent_1(fraction)
        ));
    }
    out
}

/// cutadapt's `histogram(end_statistics, n, gc_content)`.
pub fn histogram(end: &EndStats, n: u64, gc_content: f64) -> String {
    let mut out = String::new();
    out.push_str("length\tcount\texpect\tmax.err\terror counts\n");
    let probabilities = random_match_probabilities(&end.sequence, gc_content);
    for row in &end.rows {
        let index = row.length.min(end.sequence.len());
        let expect = n as f64 * probabilities[index];
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            row.length,
            row.count,
            fixed1_upper(expect),
            row.max_err,
            row.error_counts
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    out.push('\n');
    out
}

/// `EndStatistics.random_match_probabilities`.
///
/// Returns `p[i]`, the probability that the first `i` bases of the adapter
/// match a random sequence with the given GC content.
pub fn random_match_probabilities(sequence: &str, gc_content: f64) -> Vec<f64> {
    let mut p = 1.0f64;
    let mut probabilities = Vec::with_capacity(sequence.len() + 1);
    probabilities.push(p);
    for base in sequence.chars() {
        match base {
            'G' | 'C' | 'g' | 'c' => p *= gc_content / 2.0,
            _ => p *= (1.0 - gc_content) / 2.0,
        }
        probabilities.push(p);
    }
    probabilities
}

/// cutadapt's `minimal_report` (`--report minimal`).
///
/// The first field is `OK`, or `WARN` when a 3' adapter is dominated by one
/// preceding base (cutadapt's `AdjacentBaseStatistics.should_warn`).
pub fn minimal_report(stats: &Stats) -> String {
    // cutadapt's --report minimal is a single TSV table: a header line and one
    // data row. There is no banner, no summary block and no timing, which makes
    // it fully reproducible. Oracle-verified header order and the "OK" status.
    let mut out = String::new();
    out.push_str(
        "status\tin_reads\tin_bp\ttoo_short\ttoo_long\ttoo_many_n\tout_reads\tw/adapters\tqualtrim_bp\tout_bp\n",
    );
    // cutadapt reports WARN when the base statistics are suspicious: a single
    // base dominating the bases preceding removed adapters (the same condition
    // that prints the "may be incomplete" warning in the full report).
    let status = if has_warnbase(stats) { "WARN" } else { "OK" };
    let qualtrim = stats.quality_trimmed_bp().unwrap_or(0);
    out.push_str(&format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        status,
        stats.n,
        stats.total_bp(),
        stats.filtered.too_short.unwrap_or(0),
        stats.filtered.too_long.unwrap_or(0),
        stats.filtered.too_many_n.unwrap_or(0),
        stats.written,
        stats.with_adapters1.unwrap_or(0),
        qualtrim,
        stats.written_bp1,
    ));
    out
}

/// Convenience: cutadapt's `error_ranges` text for an end.
///
/// Kept here (rather than only in `stats`) because the text report is the only
/// consumer.
pub fn error_ranges(end: &EndStats) -> String {
    end.error_ranges_text()
}

/// Padding helper used by tests: the exact 27-character filter label field.
#[cfg(test)]
fn filter_label(description: &str) -> String {
    let mut label = String::from(description);
    label.push(':');
    format!("{label:<27}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::stats::{FilterCounts, HistogramRow};

    fn end_3prime() -> EndStats {
        EndStats {
            adapter_type: "regular_three_prime".to_string(),
            sequence: "AGATCGGAAGAGC".to_string(),
            error_rate: 0.1,
            indels: true,
            allows_partial_matches: true,
            effective_length: 13,
            minimum_overlap: 3,
            adjacent_bases: Some(AdjacentBases {
                a: 2,
                c: 1,
                g: 0,
                t: 3,
                none: 1,
            }),
            rows: vec![
                HistogramRow {
                    length: 6,
                    count: 1,
                    expect: 0.0,
                    max_err: 0,
                    error_counts: vec![1],
                },
                HistogramRow {
                    length: 13,
                    count: 4,
                    expect: 0.0,
                    max_err: 1,
                    error_counts: vec![3, 1],
                },
                HistogramRow {
                    length: 23,
                    count: 1,
                    expect: 0.0,
                    max_err: 1,
                    error_counts: vec![1],
                },
                HistogramRow {
                    length: 53,
                    count: 1,
                    expect: 0.0,
                    max_err: 1,
                    error_counts: vec![1],
                },
            ],
            five_prime_overlaps: None,
            five_prime_rows: Vec::new(),
            matches_total: None,
            three_prime_matches: None,
        }
    }

    fn simple_stats() -> Stats {
        Stats {
            n: 10,
            total_bp1: 460,
            with_adapters1: Some(7),
            written: 10,
            written_bp1: 326,
            adapters1: vec![AdapterStats {
                name: "1".to_string(),
                linked: false,
                reverse_complemented: None,
                front: None,
                back: Some(end_3prime()),
            }],
            ..Default::default()
        }
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC \
    //     -o .rp/a.fastq src/tests/fixtures/simple.fastq
    // (report body from "=== Summary ===" onwards)
    #[test]
    fn full_report_matches_oracle_plain() {
        let expected = "\
=== Summary ===

Total reads processed:                      10
Reads with adapters:                         7 (70.0%)
Reads written (passing filters):            10 (100.0%)

Total basepairs processed:           460 bp
Total written (filtered):            326 bp (70.9%)

=== Adapter 1 ===

Sequence: AGATCGGAAGAGC; Type: regular 3'; Length: 13; Trimmed: 7 times

Minimum overlap: 3
No. of allowed errors:
1-9 bp: 0; 10-13 bp: 1

Bases preceding removed adapters:
  A: 28.6%
  C: 14.3%
  G: 0.0%
  T: 42.9%
  none/other: 14.3%

Overview of removed sequences
length\tcount\texpect\tmax.err\terror counts
6\t1\t0.0\t0\t1
13\t4\t0.0\t1\t3 1
23\t1\t0.0\t1\t1
53\t1\t0.0\t1\t1\n";
        assert_eq!(full_report(&simple_stats()), expected);
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -m 10 \
    //     -o .rp/c.fastq src/tests/fixtures/simple.fastq
    #[test]
    fn full_report_matches_oracle_with_min_length_filter() {
        let stats = Stats {
            written: 7,
            written_bp1: 313,
            filtered: FilterCounts {
                too_short: Some(3),
                ..Default::default()
            },
            ..simple_stats()
        };
        let expected = "\
=== Summary ===

Total reads processed:                      10
Reads with adapters:                         7 (70.0%)

== Read fate breakdown ==
Reads that were too short:                   3 (30.0%)
Reads written (passing filters):             7 (70.0%)

Total basepairs processed:           460 bp
Total written (filtered):            313 bp (68.0%)

=== Adapter 1 ===

Sequence: AGATCGGAAGAGC; Type: regular 3'; Length: 13; Trimmed: 7 times

Minimum overlap: 3
No. of allowed errors:
1-9 bp: 0; 10-13 bp: 1

Bases preceding removed adapters:
  A: 28.6%
  C: 14.3%
  G: 0.0%
  T: 42.9%
  none/other: 14.3%

Overview of removed sequences
length\tcount\texpect\tmax.err\terror counts
6\t1\t0.0\t0\t1
13\t4\t0.0\t1\t3 1
23\t1\t0.0\t1\t1
53\t1\t0.0\t1\t1\n";
        assert_eq!(full_report(&stats), expected);
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -m 10 \
    //     -o .rp/b.fastq --report minimal src/tests/fixtures/simple.fastq
    // status\tin_reads\tin_bp\ttoo_short\ttoo_long\ttoo_many_n\tout_reads\tw/adapters\tqualtrim_bp\tout_bp
    // OK\t10\t460\t3\t0\t0\t7\t7\t0\t313
    #[test]
    fn minimal_report_matches_oracle_with_filter() {
        let stats = Stats {
            written: 7,
            written_bp1: 313,
            filtered: FilterCounts {
                too_short: Some(3),
                ..Default::default()
            },
            ..simple_stats()
        };
        assert_eq!(
            minimal_report(&stats),
            "status\tin_reads\tin_bp\ttoo_short\ttoo_long\ttoo_many_n\tout_reads\tw/adapters\tqualtrim_bp\tout_bp\n\
             OK\t10\t460\t3\t0\t0\t7\t7\t0\t313\n"
        );
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC \
    //     -o .rp/b2.fastq --report minimal src/tests/fixtures/simple.fastq
    // OK\t10\t460\t0\t0\t0\t10\t7\t0\t326
    #[test]
    fn minimal_report_matches_oracle_without_filter() {
        assert_eq!(
            minimal_report(&simple_stats()),
            "status\tin_reads\tin_bp\ttoo_short\ttoo_long\ttoo_many_n\tout_reads\tw/adapters\tqualtrim_bp\tout_bp\n\
             OK\t10\t460\t0\t0\t0\t10\t7\t0\t326\n"
        );
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -q 20 \
    //     -o .rp/x.fastq src/tests/fixtures/simple.fastq
    // "Quality-trimmed:                     0 bp (0.0%)"
    #[test]
    fn full_report_includes_quality_trimmed_line_when_option_used() {
        let stats = Stats {
            quality_trimmed_bp1: Some(0),
            ..simple_stats()
        };
        let report = full_report(&stats);
        assert!(
            report.contains("Quality-trimmed:                       0 bp (0.0%)\n"),
            "{report}"
        );
        // The line must not appear when -q was not used.
        assert!(!full_report(&simple_stats()).contains("Quality-trimmed"));
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC --rc \
    //     -o .rp/x.fastq src/tests/fixtures/simple.fastq
    // "Reverse-complemented:                     1 (10.0%)"
    // "; Reverse-complemented: 0 times"
    #[test]
    fn full_report_includes_reverse_complemented_lines() {
        let mut stats = simple_stats();
        stats.reverse_complemented = Some(1);
        if let Some(adapter) = stats.adapters1.first_mut() {
            adapter.reverse_complemented = Some(0);
        }
        let report = full_report(&stats);
        assert!(
            report.contains("Reverse-complemented:                        1 (10.0%)\n"),
            "{report}"
        );
        assert!(
            report.contains("Length: 13; Trimmed: 7 times; Reverse-complemented: 0 times\n"),
            "{report}"
        );
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -M 40 \
    //     -o .rp/x.fastq src/tests/fixtures/simple.fastq
    // "Reads that were too long:                    4 (40.0%)"
    #[test]
    fn filter_breakdown_uses_cutadapt_label_padding() {
        let stats = Stats {
            filtered: FilterCounts {
                too_short: Some(3),
                too_long: Some(4),
                ..Default::default()
            },
            ..simple_stats()
        };
        let report = full_report(&stats);
        assert!(
            report.contains("Reads that were too short:                   3 (30.0%)\n"),
            "{report}"
        );
        assert!(
            report.contains("Reads that were too long:                    4 (40.0%)\n"),
            "{report}"
        );
        // Both filters are present, in cutadapt's order: too_short first.
        let short = report.find("too short").unwrap();
        let long = report.find("too long").unwrap();
        assert!(short < long);
    }

    #[test]
    fn filter_label_field_is_27_characters() {
        assert_eq!(filter_label("that were too short").len(), 27);
        assert_eq!(filter_label("discarded as untrimmed").len(), 27);
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -m 10 \
    //     -o .rp/x.fastq --report minimal src/tests/fixtures/simple.fastq
    // with a dominated adjacent base the status becomes WARN
    #[test]
    fn minimal_report_status_is_warn_when_a_base_dominates() {
        let mut stats = simple_stats();
        if let Some(back) = stats.adapters1[0].back.as_mut() {
            back.adjacent_bases = Some(AdjacentBases {
                a: 19,
                c: 0,
                g: 0,
                t: 0,
                none: 1,
            });
        }
        assert!(minimal_report(&stats).starts_with("status\t"));
        assert!(minimal_report(&stats).contains("\nWARN\t"));
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a 'AGATCGGAAGAGC$' \
    //     -o .rp/an.fastq src/tests/fixtures/simple.fastq
    //   "No. of allowed errors: 1" (a single absolute count, no length ranges)
    #[test]
    fn anchored_adapter_reports_absolute_error_count() {
        let mut end = end_3prime();
        end.adapter_type = "anchored_three_prime".to_string();
        end.allows_partial_matches = false;
        end.adjacent_bases = Some(AdjacentBases {
            a: 1,
            c: 1,
            g: 0,
            t: 2,
            none: 0,
        });
        end.rows = vec![HistogramRow {
            length: 13,
            count: 4,
            expect: 0.0,
            max_err: 1,
            error_counts: vec![3, 1],
        }];
        let stats = Stats {
            n: 10,
            total_bp1: 460,
            with_adapters1: Some(4),
            written: 10,
            written_bp1: 307,
            adapters1: vec![AdapterStats {
                name: "1".to_string(),
                linked: false,
                reverse_complemented: None,
                front: None,
                back: Some(end),
            }],
            ..Default::default()
        };
        let expected = "\
=== Summary ===

Total reads processed:                      10
Reads with adapters:                         4 (40.0%)
Reads written (passing filters):            10 (100.0%)

Total basepairs processed:           460 bp
Total written (filtered):            307 bp (66.7%)

=== Adapter 1 ===

Sequence: AGATCGGAAGAGC; Type: anchored 3'; Length: 13; Trimmed: 4 times

No. of allowed errors: 1

Bases preceding removed adapters:
  A: 25.0%
  C: 25.0%
  G: 0.0%
  T: 50.0%
  none/other: 0.0%

Overview of removed sequences
length	count	expect	max.err	error counts
13	4	0.0	1	3 1
";
        assert_eq!(full_report(&stats), expected);
    }

    #[test]
    fn no_reads_processed_message() {
        let stats = Stats::default();
        assert_eq!(full_report(&stats), "No reads processed!");
        assert_eq!(full_report_with_elapsed(&stats, 0.1), "No reads processed!");
    }

    #[test]
    fn histogram_expect_uses_random_match_probabilities() {
        // gc_content 0.5: every base has probability 0.25, so p[i] = 0.25^i.
        let probabilities = random_match_probabilities("ACGT", 0.5);
        assert_eq!(probabilities[0], 1.0);
        assert_eq!(probabilities[1], 0.25);
        assert_eq!(probabilities[2], 0.0625);
        // With 100 reads and a 2-base match, expect = 6.2 (0.0625 * 100).
        let end = EndStats {
            rows: vec![HistogramRow {
                length: 2,
                count: 1,
                expect: 0.0,
                max_err: 0,
                error_counts: vec![1],
            }],
            ..end_3prime()
        };
        let rendered = histogram(&end, 100, 0.5);
        assert!(rendered.contains("2\t1\t6.2\t0\t1\n"), "{rendered}");
    }

    #[test]
    fn history_grammar_is_header_plus_rows_and_blank_line() {
        let rendered = histogram(&end_3prime(), 10, 0.5);
        assert!(rendered.starts_with("length\tcount\texpect\tmax.err\terror counts\n"));
        assert!(rendered.ends_with("\n\n"));
    }
}
