//! FastQC's module results and the modules computed by this implementation.
//!
//! ## What is computed
//!
//! | Module | Computed | Notes |
//! | --- | --- | --- |
//! | Basic Statistics | **yes** | all nine fields |
//! | Per base sequence quality | **yes** | all six numeric columns |
//! | Per tile sequence quality | **yes** | Sampled Illumina headers; missing groups remain NaN |
//! | Per sequence quality scores | **yes** | |
//! | Per base sequence content | **yes** | |
//! | Per base N content | **yes** | |
//! | Per sequence GC content | **yes** | via `stats::gc_model` |
//! | Sequence Length Distribution | **yes** | |
//! | Sequence Duplication Levels | **yes** | 50bp truncation, FastQC's default |
//! | Overrepresented sequences | **yes** | threshold 0.1% of reads |
//! | Adapter Content | **yes** | Configured literal adapters, cumulative BaseGroup rows; documented counting corrections |
//!
//! ## Status verdicts
//!
//! FastQC labels each module `pass`, `warn` or `fail` using thresholds from its
//! `Configuration/limits.txt`. This implementation covers the verdicts used by
//! the measured fixtures, not the whole limits file; the thresholds are recorded
//! in [`Thresholds`].

use crate::stats::{java_double, QcStats, BASE_ORDER};

/// FastQC's module status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Within limits.
    Pass,
    /// Outside the warning threshold.
    Warn,
    /// Outside the error threshold.
    Fail,
}

impl Status {
    /// The word FastQC prints.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// The thresholds used to assign [`Status`].
///
/// Only the ones needed by the implemented modules are present. FastQC's full
/// `limits.txt` covers every module and is not reproduced.
pub struct Thresholds {
    /// `quality_base_lower warn`: lower-quartile warn threshold.
    pub quality_lower_warn: f64,
    /// `quality_base_lower error`.
    pub quality_lower_fail: f64,
    /// `quality_base_median warn`.
    pub quality_median_warn: f64,
    /// `quality_base_median error`.
    pub quality_median_fail: f64,
    /// `n_content warn`, as a percentage.
    pub n_warn: f64,
    /// `n_content error`.
    pub n_fail: f64,
    /// `duplication warn`: percentage remaining after deduplication.
    pub duplication_warn: f64,
    /// `duplication error`.
    pub duplication_fail: f64,
    /// `sequence warn`: max deviation between A/T or G/C.
    pub sequence_warn: f64,
    /// `sequence error`.
    pub sequence_fail: f64,
    /// `gc_sequence warn`: max deviation from the theoretical distribution.
    pub gc_sequence_warn: f64,
    /// `gc_sequence error`.
    pub gc_sequence_fail: f64,
    /// `quality_sequence warn`: the most frequent Phred score.
    pub quality_sequence_warn: f64,
    /// `quality_sequence error`.
    pub quality_sequence_fail: f64,
    /// `overrepresented warn`: percentage for a sequence to be reported.
    pub overrepresented_warn: f64,
    /// `overrepresented error`.
    pub overrepresented_fail: f64,
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        // Every value below is read from FastQC 0.13.0's own
        // Configuration/limits.txt, not guessed. The file's comments also explain
        // the semantics, which is why they are quoted here: the *lower quartile*
        // and the *median* are tested separately for per-base quality, and the
        // sequence-content test is a deviation between base pairs.
        Thresholds {
            quality_lower_warn: 10.0,
            quality_lower_fail: 5.0,
            quality_median_warn: 25.0,
            quality_median_fail: 20.0,
            n_warn: 5.0,
            n_fail: 20.0,
            duplication_warn: 70.0,
            duplication_fail: 50.0,
            sequence_warn: 10.0,
            sequence_fail: 20.0,
            gc_sequence_warn: 15.0,
            gc_sequence_fail: 30.0,
            quality_sequence_warn: 27.0,
            quality_sequence_fail: 20.0,
            overrepresented_warn: 0.1,
            overrepresented_fail: 1.0,
        }
    }
}

/// One `#Base`/`#Quality`/`#Length` table row.
pub type Row = Vec<String>;

/// A rendered module: its name, verdict and body lines.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleResult {
    /// The module name, exactly as FastQC prints it after `>>`.
    pub name: &'static str,
    /// The verdict.
    pub status: Status,
    /// Every line between the header and `>>END_MODULE`, in order.
    pub lines: Vec<String>,
}

impl ModuleResult {
    /// Render the module as FastQC's block.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(">>{}\t{}\n", self.name, self.status.as_str()));
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(">>END_MODULE\n");
        out
    }
}

/// Format a base count the way FastQC's `Basic Statistics` does.
///
/// Below 1000 bases the count is printed with a ` bp` suffix. At 1000 and above it
/// switches to kilobases with **one decimal, truncated rather than rounded**:
///
/// | bases | printed |
/// | --- | --- |
/// | 996 | `996 bp` |
/// | 1000 | `1 kbp` |
/// | 1998 | `1.9 kbp` (trunc, not 2.0) |
/// | 3000 | `3 kbp` |
/// | 60000 | `60 kbp` |
///
/// The integral cases (`1 kbp`, `3 kbp`) drop `.0` because Java prints the value
/// as an integer when the truncated tenths are zero.
pub fn format_bases(total_bases: u64) -> String {
    if total_bases < 1000 {
        return format!("{total_bases} bp");
    }
    // FastQC promotes units at 10^3 boundaries and truncates to one decimal.
    let (divisor, suffix) = if total_bases >= 1_000_000_000 {
        (1_000_000_000, "Gbp")
    } else if total_bases >= 1_000_000 {
        (1_000_000, "Mbp")
    } else {
        (1_000, "kbp")
    };
    let tenths = total_bases / (divisor / 10);
    let whole = tenths / 10;
    let fraction = tenths % 10;
    if fraction == 0 {
        format!("{whole} {suffix}")
    } else {
        format!("{whole}.{fraction} {suffix}")
    }
}

/// Basic Statistics.
pub fn basic_statistics(stats: &QcStats) -> ModuleResult {
    ModuleResult {
        name: "Basic Statistics",
        // FastQC never fails this module.
        status: Status::Pass,
        lines: vec![
            "#Measure\tValue".to_string(),
            format!("Filename\t{}", stats.filename),
            "File type\tConventional base calls".to_string(),
            "Encoding\tSanger / Illumina 1.9".to_string(),
            format!("Total Sequences\t{}", stats.total_sequences),
            format!("Total Bases\t{}", format_bases(stats.total_bases)),
            format!("Sequence length\t{}", stats.length_range()),
            // FastQC prints these as whole numbers: a mean of 6.667 prints 6,
            // truncating rather than rounding (verified: lengths 4,8,8 -> 6).
            format!("Mean Length\t{}", stats.mean_length().trunc() as i64),
            format!("Median Length\t{}", stats.median_length().trunc() as i64),
            format!("%GC\t{}", stats.percent_gc()),
        ],
    }
}

/// Per base sequence quality.
///
/// Each row is one base position. The `Mean` column is that position's plain mean.
/// The four percentile columns come from `QualityCount.getPercentile`, which
/// returns `NaN` until the position has **more than 100** observations:
///
/// ```text
/// if (c.getTotalCount() > 100) { ...include... }   // else skipped
/// return qualified > 0 ? sum / qualified : NaN;
/// ```
///
/// FastQC therefore shows `NaN` in the percentile columns for every small input,
/// and real numbers once a position passes 101 reads. Measured crossover: 100
/// reads -> `NaN`, 101 -> numbers.
///
/// The verdict uses the two thresholds `limits.txt` defines for this module: the
/// lower quartile (`quality_base_lower`) and the median (`quality_base_median`).
pub fn per_base_quality(stats: &QcStats, thresholds: &Thresholds) -> ModuleResult {
    let groups = crate::grouping::base_groups(stats.max_length(), stats.nogroup);
    let mut lines = vec![
        "#Base\tMean\tMedian\tLower Quartile\tUpper Quartile\t10th Percentile\t90th Percentile"
            .to_string(),
    ];

    let mut worst_lower = f64::INFINITY;
    let mut worst_median = f64::INFINITY;
    for group in &groups {
        let column = stats.quality_in_range(group.start, group.end);
        let column = &column;
        let mean = column.mean();
        // For a GROUP of positions every column is the AVERAGE of the per-position
        // values, not the percentile of the pooled histogram. Measured: positions
        // 10 and 11 have median 36 and 35 individually; the `10-11` row prints
        // 35.5 in every column. Pooling the histogram instead yields 35, which is
        // wrong for equalsized groups and badly wrong for the truncated last one.
        let averaged = |percent: u32| -> f64 {
            let width = group.end - group.start + 1;
            if width <= 1 {
                return column.percentile(percent);
            }
            let mut sum = 0.0;
            let mut count = 0u32;
            for position in group.start..=group.end {
                if let Some(single) = stats.quality_by_position.get(position - 1) {
                    sum += single.percentile(percent);
                    count += 1;
                }
            }
            if count == 0 {
                f64::NAN
            } else {
                sum / count as f64
            }
        };
        let median = averaged(50);
        let lower = averaged(25);
        let upper = averaged(75);
        let tenth = averaged(10);
        let ninetieth = averaged(90);
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            group.label(),
            java_double(mean),
            java_double(median),
            java_double(lower),
            java_double(upper),
            java_double(tenth),
            java_double(ninetieth),
        ));
        if !lower.is_nan() && lower < worst_lower {
            worst_lower = lower;
        }
        if !median.is_nan() && median < worst_median {
            worst_median = median;
        }
    }

    // Fail wins over warn. A threshold only applies when the statistic is a real
    // number: when every position is NaN there is nothing to judge, and FastQC
    // passes (as it does for small inputs).
    let mut status = Status::Pass;
    if worst_lower.is_finite() || worst_median.is_finite() {
        let lower = if worst_lower.is_finite() {
            worst_lower
        } else {
            f64::INFINITY
        };
        let median = if worst_median.is_finite() {
            worst_median
        } else {
            f64::INFINITY
        };
        if lower < thresholds.quality_lower_fail || median < thresholds.quality_median_fail {
            status = Status::Fail;
        } else if lower < thresholds.quality_lower_warn || median < thresholds.quality_median_warn {
            status = Status::Warn;
        }
    }

    ModuleResult {
        name: "Per base sequence quality",
        status,
        lines,
    }
}

/// Per sequence quality scores.
pub fn per_sequence_quality(stats: &QcStats, thresholds: &Thresholds) -> ModuleResult {
    let mut lines = vec!["#Quality\tCount".to_string()];
    // FastQC prints a row per integer score from the lowest to the highest
    // observed, including zero-count gaps in between. Printing only the observed
    // keys would silently shorten the table and change what MultiQC plots.
    if let (Some(min), Some(max)) = (
        stats.quality_distribution.keys().next().copied(),
        stats.quality_distribution.keys().next_back().copied(),
    ) {
        for score in min..=max {
            let count = stats.quality_distribution.get(&score).copied().unwrap_or(0);
            lines.push(format!("{}\t{}", score, java_double(count as f64)));
        }
    }
    // `limits.txt`: quality_sequence warn 27 / error 20, tested against the MOST
    // FREQUENT score (not the mean). Lower is worse.
    let most_frequent = stats
        .quality_distribution
        .iter()
        .max_by_key(|(score, count)| (**count, std::cmp::Reverse(**score)))
        .map(|(score, _)| *score as f64);
    let status = match most_frequent {
        Some(score) if score <= thresholds.quality_sequence_fail => Status::Fail,
        Some(score) if score <= thresholds.quality_sequence_warn => Status::Warn,
        _ => Status::Pass,
    };

    ModuleResult {
        name: "Per sequence quality scores",
        status,
        lines,
    }
}

/// Per base sequence content.
///
/// Percentages are of A/C/G/T only, so all-`N` positions print `0.0` in every
/// column rather than `NaN`. Columns print in FastQC's order: G, A, T, C.
pub fn per_base_sequence_content(stats: &QcStats) -> ModuleResult {
    let mut lines = vec!["#Base\tG\tA\tT\tC".to_string()];
    let mut max_deviation: f64 = 0.0;
    for group in &crate::grouping::base_groups(stats.max_length(), stats.nogroup) {
        let counts = &stats.bases_in_range(group.start, group.end);
        // counts is [A, C, G, T]
        let total: u64 = counts.iter().sum();
        let value = |which: u8| -> f64 {
            if total == 0 {
                // A position with no A/C/G/T at all (all N, or all gaps) prints
                // NaN in every column, not 0.0. Verified on an all-N read.
                f64::NAN
            } else {
                let raw = match which {
                    b'A' => counts[0],
                    b'C' => counts[1],
                    b'G' => counts[2],
                    _ => counts[3],
                };
                // Java does `count / total * 100` (divide, THEN multiply), which
                // is why it prints 27.500000000000004 for 22/80. Multiplying
                // first gives exactly 27.5, so the order is part of the contract.
                raw as f64 / total as f64 * 100.0
            }
        };
        let g = value(b'G');
        let a = value(b'A');
        let t = value(b'T');
        let c = value(b'C');
        // FastQC warns when A and T, or G and C, differ by more than 10 points.
        max_deviation = max_deviation.max((a - t).abs()).max((g - c).abs());
        // No rounding: FastQC prints the raw double, so 25.25 stays 25.25 and
        // 25.900000000000002 keeps its floating-point tail.
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}",
            group.label(),
            java_double(g),
            java_double(a),
            java_double(t),
            java_double(c),
        ));
    }
    let status = if max_deviation > 20.0 {
        Status::Fail
    } else if max_deviation > 10.0 {
        Status::Warn
    } else {
        Status::Pass
    };
    ModuleResult {
        name: "Per base sequence content",
        status,
        lines,
    }
}

/// Per base N content.
pub fn per_base_n_content(stats: &QcStats, thresholds: &Thresholds) -> ModuleResult {
    let mut lines = vec!["#Base\tN-Count".to_string()];
    let mut worst: f64 = 0.0;
    for group in &crate::grouping::base_groups(stats.max_length(), stats.nogroup) {
        let n_count = &stats.n_in_range(group.start, group.end);
        // Denominator is the number of reads that reached these positions.
        let acgt: u64 = stats.bases_in_range(group.start, group.end).iter().sum();
        let reached: u64 = acgt + n_count;
        let percent = if reached == 0 {
            0.0
        } else {
            *n_count as f64 / reached as f64 * 100.0
        };
        worst = worst.max(percent);
        lines.push(format!("{}\t{}", group.label(), java_double(percent)));
    }
    let status = if worst > thresholds.n_fail {
        Status::Fail
    } else if worst > thresholds.n_warn {
        Status::Warn
    } else {
        Status::Pass
    };
    ModuleResult {
        name: "Per base N content",
        status,
        lines,
    }
}

/// Per sequence GC content.
///
/// The verdict compares the observed `gc_distribution` against a **theoretical
/// normal distribution** whose mean and standard deviation are derived from the
/// observed distribution itself, exactly as `PerSequenceGCContent` does:
///
/// 1. find the bin with the highest observed value (the modal peak);
/// 2. walk outward from it while bins stay above `peak / 10`, and take the
///    midpoint of the contiguous run containing the peak — that midpoint is the
///    theoretical mean;
/// 3. take the variance of the whole observed distribution about that mean, take
///    its square root, and build `NormalDistribution(mean, stdev)`;
/// 4. the deviation is `sum(|theoretical[i] - observed[i]|) / sum(observed) * 100`,
///    compared against `gc_sequence warn` (15) and `error` (30).
///
/// Steps 1–4 were read from `PerSequenceGCContent.calculateDistribution`'s
/// bytecode; the normal density is the standard
/// `1 / sqrt(2*pi*sigma^2) * exp(-((x-mu)^2) / (2*sigma^2))`.
pub fn per_sequence_gc_content(stats: &QcStats, thresholds: &Thresholds) -> ModuleResult {
    let mut lines = vec!["#GC Content\tCount".to_string()];
    for (bin, value) in stats.gc_distribution.iter().enumerate() {
        lines.push(format!("{}\t{}", bin, java_double(*value)));
    }

    let observed = &stats.gc_distribution;
    let total: f64 = observed.iter().sum();
    let deviation = gc_deviation_percent(observed, total);

    let status = match deviation {
        Some(value) if value > thresholds.gc_sequence_fail => Status::Fail,
        Some(value) if value > thresholds.gc_sequence_warn => Status::Warn,
        _ => Status::Pass,
    };

    ModuleResult {
        name: "Per sequence GC content",
        status,
        lines,
    }
}

/// The `deviationPercent` FastQC computes for the GC-content module.
///
/// Exposed for tests because it is the only non-obvious arithmetic in the report.
pub fn gc_deviation_percent(observed: &[f64], total: f64) -> Option<f64> {
    if total <= 0.0 {
        return None;
    }

    // 1. The modal peak.
    let mut peak_index = 0usize;
    let mut peak_value = f64::NEG_INFINITY;
    for (index, value) in observed.iter().enumerate() {
        if *value > peak_value {
            peak_value = *value;
            peak_index = index;
        }
    }

    // 2. Walk outward from the peak. FastQC's rule, from the bytecode:
    //
    //      threshold = peakValue - peakValue/10
    //      forward  = walk up   while observed[i] > threshold
    //      backward = walk down while observed[i] > threshold
    //      mean = (forward ran to the end AND backward ran to the start)
    //             ? peakIndex
    //             : (sum of visited indices) / (count visited)
    //
    // The conditional matters: for a distribution concentrated at low bins the
    // forward walk runs off the end, both flags are set, and the mean becomes the
    // peak index rather than the midpoint. Getting this wrong reported ~26% where
    // FastQC reports ~41%, turning a `fail` into a `warn`.
    // The walk is asymmetric and has an early exit, exactly as the bytecode does:
    //
    //   flagForward = 1
    //   for (i = peakIndex; i < len; i++)
    //       if (obs[i] > peak - peak/10) { sum += i; count++ }
    //       else { flagForward = 0; goto backwardWalk; }   // <-- skips the rest
    //   flagBackward = 1
    //   for (i = peakIndex-1; i >= 0; i--)
    //       if (obs[i] > peak - peak/10) { sum += i; count++ }
    //       else { flagBackward = 0; break; }
    //   mean = (flagForward && flagBackward) ? peakIndex : sum / count
    //
    // Note the forward loop STARTS AT THE PEAK INDEX (not peakIndex+1) and the
    // `goto` on the first failure jumps straight to the backward phase. The
    // `flagForward && flagBackward` branch therefore only fires when neither loop
    // ever encountered a below-threshold bin.
    let threshold = peak_value - peak_value / 10.0;

    let mut sum_indices = 0.0f64;
    let mut visited = 0u32;
    let mut forward_ok = true;
    let mut index = peak_index;
    while index < observed.len() {
        if observed[index] > threshold {
            sum_indices += index as f64;
            visited += 1;
            index += 1;
        } else {
            forward_ok = false;
            break;
        }
    }

    let mut backward_ok = true;
    let mut back = peak_index as isize - 1;
    while back >= 0 {
        if observed[back as usize] > threshold {
            sum_indices += back as f64;
            visited += 1;
            back -= 1;
        } else {
            backward_ok = false;
            break;
        }
    }

    // Bytecode at the join point (offsets 255-269):
    //
    //   iload 10 (flagBackward); ifne 265   -> if flagBackward != 0 goto usePeak
    //   iload  9 (flagForward);  ifeq 272   -> if flagForward == 0 goto useMean
    //   usePeak:  mean = peakIndex
    //   useMean:  mean = sum / count
    //
    // So the peak index is used when **either** flag is still set — that is, when
    // at least one of the two walks never hit a below-threshold bin. Only when
    // BOTH walks terminated early is the arithmetic mean used. Requiring both to
    // succeed (the intuitive reading) produced 26.2% where FastQC reports 41.0%,
    // flipping a `fail` to a `warn`.
    let theoretical_mean = if backward_ok || forward_ok {
        peak_index as f64
    } else if visited > 0 {
        sum_indices / visited as f64
    } else {
        peak_index as f64
    };

    // 3. Standard deviation about that mean.
    let mut variance = 0.0;
    for (index, value) in observed.iter().enumerate() {
        variance += (index as f64 - theoretical_mean).powi(2) * value;
    }
    variance /= total - 1.0;
    let stdev = variance.sqrt();
    if !stdev.is_finite() || stdev <= 0.0 {
        return Some(0.0);
    }

    // 4. Sum of absolute deviations from the normal density, scaled to a percent.
    let normaliser = 1.0 / (2.0 * std::f64::consts::PI * stdev * stdev).sqrt();
    let mut deviation = 0.0;
    for (index, value) in observed.iter().enumerate() {
        let x = index as f64;
        let density =
            normaliser * (-((x - theoretical_mean).powi(2)) / (2.0 * stdev * stdev)).exp();
        let theoretical = density * total;
        deviation += (theoretical - value).abs();
    }
    Some(deviation / total * 100.0)
}

/// Sequence Length Distribution.
pub fn sequence_length_distribution(stats: &QcStats) -> ModuleResult {
    let mut lines = vec!["#Length\tCount".to_string()];
    for (length, count) in stats.length_distribution() {
        lines.push(format!("{}\t{}", length, java_double(count as f64)));
    }
    // `limits.txt`: "The actual tests warn if you have sequences of different
    // length, and error if you have sequences of zero length." The values (1/1)
    // are just on/off switches.
    let distinct = stats.lengths.len();
    let has_zero = stats.lengths.contains_key(&0);
    let status = if has_zero {
        Status::Fail
    } else if distinct > 1 {
        Status::Warn
    } else {
        Status::Pass
    };
    ModuleResult {
        name: "Sequence Length Distribution",
        status,
        lines,
    }
}

/// Sequence Duplication Levels.
pub fn sequence_duplication(stats: &QcStats, thresholds: &Thresholds) -> ModuleResult {
    // FastQC buckets duplication by "how many times was this sequence seen".
    let total = stats.total_sequences.max(1) as f64;
    let unique = stats.sequences.len() as f64;
    // `unique / total * 100`, divide first, to match Java's bit pattern.
    let deduplicated = unique / total * 100.0;

    let mut buckets: [f64; 16] = [0.0; 16];
    for count in stats.sequences.values() {
        let level = match *count {
            1 => 0,
            2 => 1,
            3 => 2,
            4 => 3,
            5 => 4,
            6 => 5,
            7 => 6,
            8 => 7,
            9 => 8,
            // Bins are 1,2,3,4,5,6,7,8,9,>10,>50,>100,>500,>1k,>5k,>10k+.
            //
            // Each ">N" label is INCLUSIVE of N on its LOWER side and exclusive on
            // the next label: measured against FastQC 0.13.0 by varying the
            // occurrence count of one sequence in a 200-read file:
            //
            //   count  10 -> >10      count 100 -> >50
            //   count  50 -> >10      count 101 -> >100
            //   count 500 -> >100     count 501 -> >500
            //
            // So >10 covers 10..=49, >50 covers 50..=100, >100 covers 101..=500,
            // and so on. The upper bound of each bucket is the next bucket's
            // threshold, not the next threshold minus one.
            10..=49 => 9,
            50..=100 => 10,
            101..=500 => 11,
            501..=1000 => 12,
            1001..=5000 => 13,
            5001..=10000 => 14,
            _ => 15,
        };
        buckets[level] += *count as f64;
    }
    let labels = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", ">10", ">50", ">100", ">500", ">1k", ">5k",
        ">10k+",
    ];
    let mut lines = vec![
        // Raw double: FastQC prints 0.9090909090909091, not 0.9.
        format!(
            "#Total Deduplicated Percentage\t{}",
            java_double(deduplicated)
        ),
        "#Duplication Level\tPercentage of total".to_string(),
    ];
    for (label, value) in labels.iter().zip(buckets.iter()) {
        // Divide first: Java computes `value / total * 100`, which yields
        // 62.66666666666667 where `value * 100 / total` yields ...664. The bit
        // pattern is part of the byte comparison.
        let percent = if total == 0.0 {
            0.0
        } else {
            value / total * 100.0
        };
        lines.push(format!("{}\t{}", label, java_double(percent)));
    }

    let status = if deduplicated < thresholds.duplication_fail {
        Status::Fail
    } else if deduplicated < thresholds.duplication_warn {
        Status::Warn
    } else {
        Status::Pass
    };
    ModuleResult {
        name: "Sequence Duplication Levels",
        status,
        lines,
    }
}

/// Overrepresented sequences.
///
/// A sequence is overrepresented when it accounts for more than 0.1% of reads,
/// which is FastQC's default.
pub fn overrepresented_sequences(stats: &QcStats, thresholds: &Thresholds) -> ModuleResult {
    // The `#Sequence` header row is emitted ONLY when there is at least one hit.
    // An empty module is just the two `>>` markers with no lines between them:
    //
    //   >>Overrepresented sequences	pass
    //   >>END_MODULE
    //
    // Emitting the header unconditionally left a stray line in every passing
    // report.
    let mut lines: Vec<String> = Vec::new();
    let total = stats.total_sequences;
    let mut hits: Vec<(&Vec<u8>, &u64)> = stats
        .sequences
        .iter()
        .filter(|(_, count)| {
            total > 0 && (**count as f64) / total as f64 * 100.0 > thresholds.overrepresented_warn
        })
        .collect();
    // Highest count first, then lexicographic.
    //
    // FastQC's own order for equal counts is the iteration order of a Java
    // `HashMap`, which is not a specified contract and is not reproducible by any
    // other implementation. Choosing a deterministic order here is therefore not
    // a deviation from a real contract — there is no contract to deviate from.
    // The differential normalises this module by sorting its rows on BOTH sides
    // before comparing, so counts and percentages are still compared exactly.
    hits.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

    // Header row only when there is something to tabulate. An empty module is
    // exactly the two `>>` markers with nothing between them, which is what
    // FastQC emits when no sequence clears the threshold.
    if hits.is_empty() {
        return ModuleResult {
            name: "Overrepresented sequences",
            status: Status::Pass,
            lines,
        };
    }
    lines.push("#Sequence\tCount\tPercentage\tPossible Source".to_string());

    for (sequence, count) in &hits {
        // Rounded to TWO decimals: 1/130 = 0.76923... prints 0.77, and
        // 43/160 = 26.875 prints 26.88. FastQC rounds here (unlike %GC, which
        // truncates), so the two columns genuinely differ in treatment.
        let percent = (**count as f64 / total as f64 * 100.0 * 100.0).round() / 100.0;
        let source = crate::contaminants::find_best_hit(&stats.contaminants, sequence)
            .map(|hit| hit.render())
            .unwrap_or_else(|| "No Hit".to_string());
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            String::from_utf8_lossy(sequence),
            count,
            java_double(percent),
            source
        ));
    }
    // `limits.txt`: `overrepresented warn 0.1` is the REPORTING threshold and
    // `overrepresented error 1` is the failure threshold. So any hit at all means
    // at least `warn`; only a sequence at 1% or more of the library fails.
    //
    // Both comparisons are STRICT (`>`), which the boundary case pins: a file
    // whose most common sequence is exactly 1.0% of reads still reports `warn`,
    // not `fail`. Measured at 0.71% (warn), 1.0% (warn) and 50% (fail).
    let worst_percent = hits
        .iter()
        .map(|(_, count)| **count as f64 / total as f64 * 100.0)
        .fold(0.0f64, f64::max);
    let status = if hits.is_empty() {
        Status::Pass
    } else if worst_percent > thresholds.overrepresented_fail {
        Status::Fail
    } else {
        Status::Warn
    };
    ModuleResult {
        name: "Overrepresented sequences",
        status,
        lines,
    }
}

/// Round to `places` decimal places, the way FastQC's report writer does before
/// handing the value to Java's double formatting.
///
/// FastQC pre-rounds several columns so the data file is compact and stable;
/// without this a mean would print all 17 significant digits.
pub fn round_to(value: f64, places: u32) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let factor = 10f64.powi(places as i32);
    (value * factor).round() / factor
}

/// Adapter Content.
///
/// FastQC groups positions with `BaseGroup` and averages their cumulative
/// adapter percentages, using Java-style double formatting.
/// The module warns above 5% and fails above 10% for any adapter.
pub fn adapter_content(stats: &QcStats) -> ModuleResult {
    let total = stats.total_sequences;
    let adapter_length = stats
        .adapters
        .iter()
        .map(|adapter| adapter.sequence.len())
        .max()
        .unwrap_or(0);
    if adapter_length > stats.max_length() {
        return ModuleResult {
            name: "Adapter Content",
            status: Status::Warn,
            lines: Vec::new(),
        };
    }
    let max_start = stats.max_length() - adapter_length + 1;
    let groups = crate::grouping::base_groups(max_start, stats.nogroup);
    let mut lines = Vec::new();
    if total > 0 && max_start > 0 && !stats.adapter_hits.is_empty() {
        let mut header = String::from("#Position");
        for adapter in &stats.adapters {
            header.push('\t');
            header.push_str(&adapter.name);
        }
        lines.push(header);
        let mut worst = 0.0f64;
        let mut cumulative = vec![0u64; stats.adapters.len()];
        for group in &groups {
            let width = (group.end - group.start + 1) as f64;
            let mut row = vec![group.label()];
            for (index, hits) in stats.adapter_hits.iter().enumerate() {
                let mut sum = 0.0;
                for position in group.start - 1..group.end {
                    cumulative[index] += hits.get(position).copied().unwrap_or(0);
                    sum += cumulative[index] as f64 * 100.0 / total as f64;
                }
                let percent = sum / width;
                worst = worst.max(percent);
                row.push(java_double(percent));
            }
            lines.push(row.join("\t"));
        }
        let status = if worst > 10.0 {
            Status::Fail
        } else if worst > 5.0 {
            Status::Warn
        } else {
            Status::Pass
        };
        ModuleResult {
            name: "Adapter Content",
            status,
            lines,
        }
    } else {
        ModuleResult {
            name: "Adapter Content",
            status: Status::Pass,
            lines,
        }
    }
}

/// Build every module result, in FastQC's order.
pub fn all_modules(stats: &QcStats) -> Vec<ModuleResult> {
    let thresholds = Thresholds::default();
    let mut modules = vec![
        basic_statistics(stats),
        per_base_quality(stats, &thresholds),
    ];
    if let Some(tile) = stats.tiles.report(stats.nogroup, 5.0, 10.0) {
        modules.push(tile);
    }
    modules.extend([
        per_sequence_quality(stats, &thresholds),
        per_base_sequence_content(stats),
        per_sequence_gc_content(stats, &thresholds),
        per_base_n_content(stats, &thresholds),
        sequence_length_distribution(stats),
        sequence_duplication(stats, &thresholds),
        overrepresented_sequences(stats, &thresholds),
        adapter_content(stats),
    ]);
    modules
}

/// The base order used by [`per_base_sequence_content`], exposed for tests.
pub fn base_order() -> [u8; 4] {
    BASE_ORDER
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_with(reads: &[(&[u8], &[u8])]) -> QcStats {
        let mut stats = QcStats::new("test.fq");
        for (seq, qual) in reads {
            stats.add_read(seq, qual, 33);
        }
        stats
    }

    #[test]
    fn basic_statistics_has_all_nine_fields() {
        let stats = stats_with(&[(b"ACGT", b"IIII")]);
        let module = basic_statistics(&stats);
        assert_eq!(module.lines.len(), 10, "#Measure plus nine fields");
        assert_eq!(module.lines[0], "#Measure\tValue");
        assert_eq!(module.lines[1], "Filename\ttest.fq");
        assert_eq!(module.lines[2], "File type\tConventional base calls");
        assert_eq!(module.lines[3], "Encoding\tSanger / Illumina 1.9");
        assert_eq!(module.status, Status::Pass);
    }

    #[test]
    fn a_module_renders_with_its_header_and_end_marker() {
        let stats = stats_with(&[(b"ACGT", b"IIII")]);
        let text = basic_statistics(&stats).render();
        assert!(text.starts_with(">>Basic Statistics\tpass\n"), "{text}");
        assert!(text.ends_with(">>END_MODULE\n"), "{text}");
    }

    #[test]
    fn format_bases_promotes_kbp_mbp_gbp_with_truncation() {
        assert_eq!(format_bases(999), "999 bp");
        assert_eq!(format_bases(1000), "1 kbp");
        assert_eq!(format_bases(1998), "1.9 kbp");
        assert_eq!(format_bases(1_000_000), "1 Mbp");
        assert_eq!(format_bases(1_500_000), "1.5 Mbp");
        assert_eq!(format_bases(1_000_000_000), "1 Gbp");
    }

    #[test]
    fn base_content_columns_are_g_a_t_c() {
        assert_eq!(base_order(), [b'G', b'A', b'T', b'C']);
        let stats = stats_with(&[(b"ACGT", b"IIII")]);
        let module = per_base_sequence_content(&stats);
        assert_eq!(module.lines[0], "#Base\tG\tA\tT\tC");
        // Position 1 is A only.
        assert_eq!(module.lines[1], "1\t0.0\t100.0\t0.0\t0.0");
    }

    #[test]
    fn base_content_of_all_n_prints_nan_not_zeros() {
        // Verified against FastQC 0.13.0: an all-N read prints NaN in every
        // column, because the A/C/G/T denominator is zero.
        let stats = stats_with(&[(b"NNNN", b"IIII")]);
        let module = per_base_sequence_content(&stats);
        assert_eq!(module.lines[1], "1\tNaN\tNaN\tNaN\tNaN");
    }

    #[test]
    fn n_content_prints_two_decimal_places() {
        let stats = stats_with(&[(b"AN", b"II")]);
        let module = per_base_n_content(&stats, &Thresholds::default());
        assert_eq!(module.lines[0], "#Base\tN-Count");
        assert_eq!(module.lines[1], "1\t0.0");
        assert_eq!(module.lines[2], "2\t100.0");
    }

    #[test]
    fn length_distribution_includes_gap_bins() {
        let stats = stats_with(&[(b"AA", b"II"), (b"AAAAAA", b"IIIIII")]);
        let module = sequence_length_distribution(&stats);
        assert_eq!(
            module.lines,
            vec![
                "#Length\tCount".to_string(),
                "2\t1.0".to_string(),
                "3\t0.0".to_string(),
                "4\t0.0".to_string(),
                "5\t0.0".to_string(),
                "6\t1.0".to_string(),
            ]
        );
    }

    #[test]
    fn overrepresented_uses_the_point_one_percent_threshold() {
        // 2000 reads, one sequence appearing 3 times = 0.15% -> reported.
        let mut reads: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for index in 0..2000 {
            if index < 3 {
                reads.push((b"ACGTACGT".to_vec(), b"IIIIIIII".to_vec()));
            } else {
                reads.push((
                    format!("GGGGGGGG{:04}", index).into_bytes(),
                    b"IIIIIIIIIIII".to_vec(),
                ));
            }
        }
        let refs: Vec<(&[u8], &[u8])> = reads
            .iter()
            .map(|(a, b)| (a.as_slice(), b.as_slice()))
            .collect();
        let stats = stats_with(&refs);
        let module = overrepresented_sequences(&stats, &Thresholds::default());
        assert!(module.lines.len() > 1, "{:?}", module.lines);
        // 3/2000 = 0.15% clears the 0.1% reporting threshold but is below the 1%
        // failure threshold, so the verdict is warn.
        assert_eq!(module.status, Status::Warn);
    }

    #[test]
    fn round_to_keeps_the_requested_places() {
        assert_eq!(round_to(19.066666666666666, 3), 19.067);
        assert_eq!(round_to(40.0, 3), 40.0);
        assert_eq!(round_to(52.173913, 1), 52.2);
    }

    #[test]
    fn per_sequence_quality_prints_floating_counts() {
        let stats = stats_with(&[(b"ACGT", b"IIII"), (b"ACGT", b"IIII")]);
        let module = per_sequence_quality(&stats, &Thresholds::default());
        assert_eq!(module.lines[0], "#Quality\tCount");
        // Both reads average quality 40, so one row with a floating count.
        assert_eq!(module.lines[1], "40\t2.0");
    }

    #[test]
    fn gc_content_prints_one_hundred_and_one_bins() {
        let stats = stats_with(&[(b"ACGT", b"IIII")]);
        let module = per_sequence_gc_content(&stats, &Thresholds::default());
        assert_eq!(module.lines.len(), 102, "#GC Content plus 101 bins");
    }
}
