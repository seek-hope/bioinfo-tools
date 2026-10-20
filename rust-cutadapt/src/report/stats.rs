//! Input data model for the cutadapt 4.6 text and JSON reports.
//!
//! These types are deliberately **passive**: they describe *what to print*, not
//! how the pipeline computes it. The pipeline (or any caller) fills a
//! [`Stats`] value and hands it to [`crate::report::text`] or
//! [`crate::report::json`]. Neither report module touches the pipeline, the
//! file system or the clock.
//!
//! # Modelling cutadapt's `None` values
//!
//! cutadapt distinguishes "the counting option was not used" from "the option
//! was used and counted zero". That distinction is visible in the JSON report,
//! where unused counters are `null`:
//!
//! ```text
//! $ cutadapt -a AGATCGGAAGAGC -o out.fastq in.fastq --json out.json
//!   "read_counts": { "filtered": { "too_short": null, ... } }
//! $ cutadapt -a AGATCGGAAGAGC -m 10 -o out.fastq in.fastq --json out.json
//!   "read_counts": { "filtered": { "too_short": 3, ... } }
//! ```
//!
//! Every optional counter in this module mirrors that: `Option::None` prints as
//! `null` (JSON) or omits the line (text), `Some(0)` prints as `0`.
//!
//! # Percentages and `n == 0`
//!
//! cutadapt's `safe_divide` returns `0.0` when the denominator is zero; that is
//! reproduced by [`crate::report::fmt::fraction`].

/// Filter counters, in cutadapt's fixed report order.
///
/// The order matters: `--report minimal` columns and the text report's
/// "Read fate breakdown" follow this order, and `FILTERS` in cutadapt's
/// `report.py` is an ordered dict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterCounts {
    pub too_short: Option<u64>,
    pub too_long: Option<u64>,
    pub too_many_n: Option<u64>,
    pub too_many_expected_errors: Option<u64>,
    pub casava_filtered: Option<u64>,
    pub discard_trimmed: Option<u64>,
    pub discard_untrimmed: Option<u64>,
}

/// The seven filter names in cutadapt's `FILTERS` order.
pub const FILTER_NAMES: [&str; 7] = [
    "too_short",
    "too_long",
    "too_many_n",
    "too_many_expected_errors",
    "casava_filtered",
    "discard_trimmed",
    "discard_untrimmed",
];

impl FilterCounts {
    /// Value for `name` (one of [`FILTER_NAMES`]), or `None` if not counted.
    pub fn get(&self, name: &str) -> Option<u64> {
        match name {
            "too_short" => self.too_short,
            "too_long" => self.too_long,
            "too_many_n" => self.too_many_n,
            "too_many_expected_errors" => self.too_many_expected_errors,
            "casava_filtered" => self.casava_filtered,
            "discard_trimmed" => self.discard_trimmed,
            "discard_untrimmed" => self.discard_untrimmed,
            _ => None,
        }
    }

    /// Sum of the counters that were actually taken
    /// (cutadapt's `filtered_total`).
    pub fn total(&self) -> u64 {
        FILTER_NAMES.iter().filter_map(|n| self.get(n)).sum()
    }

    /// Iterate `(name, count)` for counters that were taken, in report order.
    pub fn present(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        FILTER_NAMES.iter().filter_map(|name| {
            self.get(name).map(|count| {
                let name: &'static str = name;
                (name, count)
            })
        })
    }
}

/// One row of the "Overview of removed sequences" histogram.
///
/// cutadapt builds these in `report.histogram_rows`. `error_counts[e]` is the
/// number of removed sequences of length `length` that matched with `e` errors;
/// the vector has `max_errors_observed + 1` entries.
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramRow {
    /// Length of the removed sequence.
    pub length: usize,
    /// How often this length occurred.
    pub count: u64,
    /// Reads expected to be trimmed by chance (cutadapt's `expect`).
    pub expect: f64,
    /// Adapter errors allowed at this length.
    pub max_err: usize,
    /// Counts per error count, indexed by the number of errors.
    pub error_counts: Vec<u64>,
}

/// Statistics for one adapter end (5' or 3').
#[derive(Clone, Debug, PartialEq)]
pub struct EndStats {
    /// cutadapt's `adapter_type`, e.g. `"regular_three_prime"`.
    pub adapter_type: String,
    /// The adapter sequence as reported (`BackAdapter.seq`), without `$`.
    pub sequence: String,
    /// Error rate for this end.
    pub error_rate: f64,
    /// Whether indels were allowed (`--no-indels` sets this to false).
    pub indels: bool,
    /// Whether partial matches are allowed. When `false`, cutadapt reports
    /// `error_lengths` as `null` (JSON) and the text report prints a single
    /// absolute error count instead of the length ranges.
    pub allows_partial_matches: bool,
    /// `len(sequence)` as seen by cutadapt's statistics. For 3' adapters this
    /// equals `sequence.len()`; it is stored separately because cutadapt uses
    /// `effective_length` in the error ranges and histogram.
    pub effective_length: usize,
    /// Adapter `min_overlap` (`-O`, default 3), printed as
    /// `Minimum overlap: N` when partial matches are allowed.
    pub minimum_overlap: usize,
    /// Adjacent base of each removal, keyed by `A`, `C`, `G`, `T`, `""`
    /// (the empty key means "none/other"). cutadapt only fills this for 3'
    /// adapters; `None` means the base statistics were never set.
    pub adjacent_bases: Option<AdjacentBases>,
    /// Histogram rows, already sorted by `length` ascending.
    pub rows: Vec<HistogramRow>,
    /// For a `-b` (anywhere) adapter: how many matches overlapped the read's 5'
    /// end. cutadapt prints a two-line breakdown for these adapters; `None`
    /// means the breakdown is not shown.
    pub five_prime_overlaps: Option<u64>,
    /// For a `-b` adapter: the histogram rows of the 5'-overlap matches, which
    /// cutadapt prints under `Overview of removed sequences (5')`.
    pub five_prime_rows: Vec<HistogramRow>,
    /// Explicit match total. `-b` adapters split their matches across two
    /// histograms, so `total()` cannot be derived from `rows` alone.
    pub matches_total: Option<u64>,
    /// For linked adapters: how many matches also removed a 3' part. The report
    /// prints `N times` for each end; a match that only removed the 5' part does
    /// not count towards the 3' figure.
    pub three_prime_matches: Option<u64>,
}

impl EndStats {
    /// Total number of removals recorded for this end.
    pub fn total(&self) -> u64 {
        if let Some(total) = self.matches_total {
            return total;
        }
        self.rows.iter().map(|r| r.count).sum()
    }

    /// `ErrorRanges(length, error_rate).lengths()` when partial matches are
    /// allowed, otherwise `None` (cutadapt reports `null`).
    ///
    /// This reproduces `report.ErrorRanges._compute_lengths` exactly:
    ///
    /// ```text
    /// >>> ErrorRanges(length=8, error_rate=0.1).lengths()
    /// [8]
    /// >>> ErrorRanges(length=19, error_rate=0.1).lengths()
    /// [9, 19]
    /// >>> ErrorRanges(length=20, error_rate=0.1).lengths()
    /// [9, 19, 20]
    /// ```
    pub fn error_lengths(&self) -> Option<Vec<usize>> {
        if !self.allows_partial_matches {
            return None;
        }
        Some(error_lengths(self.effective_length, self.error_rate))
    }

    /// cutadapt's `error_ranges()` text: the body after `"No. of allowed errors:"`.
    pub fn error_ranges_text(&self) -> String {
        error_ranges_text(self)
    }
}

/// Base frequencies preceding removed 3' adapters.
///
/// cutadapt keeps a `Counter` keyed by the adjacent base; missing keys count as
/// zero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdjacentBases {
    pub a: u64,
    pub c: u64,
    pub g: u64,
    pub t: u64,
    /// Reads where the removed sequence starts at position 0 (key `""`).
    pub none: u64,
}

/// Base order used by cutadapt's report (`A`, `C`, `G`, `T`, then none/other).
pub const ADJACENT_BASE_ORDER: [&str; 5] = ["A", "C", "G", "T", ""];

impl AdjacentBases {
    pub fn total(&self) -> u64 {
        self.a + self.c + self.g + self.t + self.none
    }

    /// Count for one key of [`ADJACENT_BASE_ORDER`].
    pub fn get(&self, base: &str) -> u64 {
        match base {
            "A" => self.a,
            "C" => self.c,
            "G" => self.g,
            "T" => self.t,
            "" => self.none,
            _ => 0,
        }
    }

    /// `(label, count)` pairs in report order. The empty key is labelled
    /// `none/other` in the text report.
    pub fn iter_report(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        ADJACENT_BASE_ORDER
            .into_iter()
            .map(|base| (base, self.get(base)))
    }

    /// cutadapt's `warnbase`: the base that precedes the adapter in more than
    /// 80 % of removals, but only when at least 20 removals were recorded.
    ///
    /// Reproduced from `report.AdjacentBaseStatistics.__init__`.
    pub fn warnbase(&self) -> Option<&'static str> {
        let total = self.total();
        if total == 0 || total < 20 {
            return None;
        }
        // cutadapt tests every base and keeps the last one above 0.8; the
        // empty key is skipped.
        let mut warn = None;
        for base in ["A", "C", "G", "T"] {
            let fraction = self.get(base) as f64 / total as f64;
            if fraction > 0.8 {
                warn = Some(base);
            }
        }
        warn
    }

    /// True when cutadapt prints the "adapter is preceded by ... extremely
    /// often" warning for this end.
    pub fn should_warn(&self) -> bool {
        self.warnbase().is_some()
    }
}

/// `report.ErrorRanges(length, error_rate).lengths()`.
///
/// The entry at index `i` is the length up to which `i` errors are allowed;
/// the last entry is always `length`.
pub fn error_lengths(length: usize, error_rate: f64) -> Vec<usize> {
    let max_errors = (error_rate * length as f64) as usize;
    let mut lengths: Vec<usize> = (1..=max_errors)
        .map(|errors| {
            let raw = errors as f64 / error_rate;
            // Python's int() truncates toward zero; values here are positive.
            let truncated = raw as usize;
            truncated.saturating_sub(1)
        })
        .collect();
    match lengths.last() {
        Some(&last) if last >= length => {}
        _ => lengths.push(length),
    }
    lengths
}

/// `report.error_ranges(end_statistics)`, i.e. the text that follows
/// `"No. of allowed errors:"`.
pub fn error_ranges_text(end: &EndStats) -> String {
    if !end.allows_partial_matches {
        let errors = (end.error_rate * end.effective_length as f64) as i64;
        return format!(" {errors}\n");
    }
    let lengths = error_lengths(end.effective_length, end.error_rate);
    let ranges = ErrorRangesText {
        length: end.effective_length,
        lengths,
    };
    format!("\n{ranges}\n")
}

/// `str(ErrorRanges(...))`, e.g. `"1-9 bp: 0; 10-13 bp: 1"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorRangesText {
    pub length: usize,
    pub lengths: Vec<usize>,
}

impl std::fmt::Display for ErrorRangesText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut prev = 1usize;
        for (errors, &r) in self.lengths.iter().enumerate().take(self.lengths.len() - 1) {
            write!(f, "{prev}-{r} bp: {errors}; ")?;
            prev = r + 1;
        }
        let last = *self.lengths.last().expect("error ranges are never empty");
        let max_errors = self.lengths.len() - 1;
        if prev == last {
            write!(f, "{prev} bp: {max_errors}")
        } else {
            write!(f, "{prev}-{last} bp: {max_errors}")
        }
    }
}

/// Statistics for one adapter (one report section).
#[derive(Clone, Debug, PartialEq)]
pub struct AdapterStats {
    /// Displayed name (`"1"`, `"2"`, or a `{name}` given via `-a name=SEQ`).
    pub name: String,
    /// Whether this is a linked adapter (`^...`).
    pub linked: bool,
    /// Number of times the adapter was found on the reverse complement, or
    /// `None` when `--rc` was not used.
    pub reverse_complemented: Option<u64>,
    /// 5' end statistics, or `None` when the adapter has no 5' end.
    pub front: Option<EndStats>,
    /// 3' end statistics, or `None` when the adapter has no 3' end.
    pub back: Option<EndStats>,
}

impl AdapterStats {
    /// `total_matches` in the JSON report: the sum of both ends.
    pub fn total_matches(&self) -> u64 {
        self.front.as_ref().map_or(0, EndStats::total)
            + self.back.as_ref().map_or(0, EndStats::total)
    }
}

/// One length bucket of the poly-A report (JSON: `{"len": L, "count": C}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolyALength {
    pub length: usize,
    pub count: u64,
}

/// Everything both reports need.
///
/// Field names follow cutadapt's `report.Statistics` so the mapping is obvious.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    /// `None` means "not known"; cutadapt uses `False` for single-end.
    pub paired: bool,
    /// Total number of input reads (or read pairs).
    pub n: u64,
    /// Total input basepairs, read 1.
    pub total_bp1: u64,
    /// Total input basepairs, read 2 (`Some` only for paired-end).
    pub total_bp2: Option<u64>,
    /// Reads with at least one adapter, read 1 (`None` if adapters were not
    /// searched for at all).
    pub with_adapters1: Option<u64>,
    /// Reads with at least one adapter, read 2 (paired-end only).
    pub with_adapters2: Option<u64>,
    /// Bases removed by quality trimming, read 1 (`None` unless `-q`).
    pub quality_trimmed_bp1: Option<u64>,
    /// Bases removed by quality trimming, read 2.
    pub quality_trimmed_bp2: Option<u64>,
    /// Bases removed by poly-A trimming, read 1 (`None` unless `--poly-a`).
    pub poly_a_trimmed_bp1: Option<u64>,
    /// Bases removed by poly-A trimming, read 2.
    pub poly_a_trimmed_bp2: Option<u64>,
    /// Length histogram of the poly-A sequences that were trimmed, read 1.
    pub poly_a_lengths1: Option<Vec<PolyALength>>,
    /// Length histogram of the poly-A sequences that were trimmed, read 2.
    pub poly_a_lengths2: Option<Vec<PolyALength>>,
    /// Number of reads that were reverse complemented (`None` unless `--rc`).
    pub reverse_complemented: Option<u64>,
    /// Filter counters.
    pub filtered: FilterCounts,
    /// Number of written reads (cutadapt's `written`).
    pub written: u64,
    /// Total written basepairs (both reads summed for paired-end).
    pub written_bp1: u64,
    /// Written basepairs, read 2.
    pub written_bp2: Option<u64>,
    /// Adapter statistics, read 1.
    pub adapters1: Vec<AdapterStats>,
    /// Adapter statistics, read 2 (paired-end only).
    pub adapters2: Vec<AdapterStats>,
    /// GC content as a fraction (cutadapt's `--gc-content`, default 50 %).
    pub gc_content: f64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            paired: false,
            n: 0,
            total_bp1: 0,
            total_bp2: None,
            with_adapters1: None,
            with_adapters2: None,
            quality_trimmed_bp1: None,
            quality_trimmed_bp2: None,
            poly_a_trimmed_bp1: None,
            poly_a_trimmed_bp2: None,
            poly_a_lengths1: None,
            poly_a_lengths2: None,
            reverse_complemented: None,
            filtered: FilterCounts::default(),
            written: 0,
            written_bp1: 0,
            written_bp2: None,
            adapters1: Vec::new(),
            adapters2: Vec::new(),
            gc_content: 0.5,
        }
    }
}

impl Stats {
    /// Bases removed by quality trimming (all reads), or `None` when `-q` was
    /// not used.
    pub fn quality_trimmed_bp(&self) -> Option<u64> {
        self.quality_trimmed_bp1
    }

    /// Bases removed by poly-A trimming (all reads), or `None` without
    /// `--poly-a`.
    pub fn poly_a_trimmed_bp(&self) -> Option<u64> {
        self.poly_a_trimmed_bp1
    }

    /// `total_bp = total_bp[0] + total_bp[1]` (cutadapt's `Statistics.total`).
    pub fn total_bp(&self) -> u64 {
        self.total_bp1 + self.total_bp2.unwrap_or(0)
    }

    /// `total_written_bp`.
    pub fn total_written_bp(&self) -> u64 {
        self.written_bp1 + self.written_bp2.unwrap_or(0)
    }

    /// `add_if_not_none(quality_trimmed_bp[0], quality_trimmed_bp[1])`.
    pub fn quality_trimmed(&self) -> Option<u64> {
        match (self.quality_trimmed_bp1, self.quality_trimmed_bp2) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        }
    }

    /// `add_if_not_none(poly_a_trimmed_bp[0], poly_a_trimmed_bp[1])`.
    pub fn poly_a_trimmed(&self) -> Option<u64> {
        match (self.poly_a_trimmed_bp1, self.poly_a_trimmed_bp2) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        }
    }

    /// `written_fraction`.
    pub fn written_fraction(&self) -> f64 {
        crate::report::fmt::fraction(Some(self.written), self.n)
    }

    /// `with_adapters_fraction(i)`.
    pub fn with_adapters_fraction(&self, i: usize) -> f64 {
        let value = if i == 0 {
            self.with_adapters1
        } else {
            self.with_adapters2
        };
        crate::report::fmt::fraction(value, self.n)
    }

    /// `quality_trimmed_fraction`.
    pub fn quality_trimmed_fraction(&self) -> f64 {
        crate::report::fmt::fraction(self.quality_trimmed(), self.total_bp())
    }

    /// `poly_a_trimmed_fraction`.
    pub fn poly_a_trimmed_fraction(&self) -> f64 {
        crate::report::fmt::fraction(self.poly_a_trimmed(), self.total_bp())
    }

    /// `total_written_bp_fraction`.
    pub fn total_written_bp_fraction(&self) -> f64 {
        crate::report::fmt::fraction(Some(self.total_written_bp()), self.total_bp())
    }

    /// `reverse_complemented_fraction`.
    pub fn reverse_complemented_fraction(&self) -> f64 {
        crate::report::fmt::fraction(self.reverse_complemented, self.n)
    }

    /// `filtered_fraction(name)`.
    pub fn filtered_fraction(&self, name: &str) -> f64 {
        crate::report::fmt::fraction(self.filtered.get(name), self.n)
    }

    /// True when any 3' adapter end of the selected read is dominated by one
    /// preceding base (the `WARN` status of `--report minimal`).
    pub fn has_warnbase(&self) -> bool {
        let all = self.adapters1.iter().chain(self.adapters2.iter());
        all.filter_map(|a| a.back.as_ref())
            .any(|end| end.adjacent_bases.as_ref().is_some_and(|b| b.should_warn()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // $ .work/oracle-venv/bin/python -c "from cutadapt.report import ErrorRanges; \
    //     print(ErrorRanges(8, 0.1).lengths(), ErrorRanges(19, 0.1).lengths(), \
    //           ErrorRanges(20, 0.1).lengths(), ErrorRanges(21, 0.1).lengths())"
    // [8] [9, 19] [9, 19, 20] [9, 19, 21]
    #[test]
    fn error_lengths_match_cutadapt_docstring() {
        assert_eq!(error_lengths(8, 0.1), vec![8]);
        assert_eq!(error_lengths(19, 0.1), vec![9, 19]);
        assert_eq!(error_lengths(20, 0.1), vec![9, 19, 20]);
        assert_eq!(error_lengths(21, 0.1), vec![9, 19, 21]);
    }

    // $ cutadapt -a AGATCGGAAGAGC -e 0.25 -o /dev/null in.fastq --json out.json
    //   -> "error_lengths": [3, 7, 11, 13]
    #[test]
    fn error_lengths_match_oracle_error_rate_025() {
        assert_eq!(error_lengths(13, 0.25), vec![3, 7, 11, 13]);
    }

    // $ cutadapt -a ACGTACGTAC -e 0.2 ... --json -> "error_lengths": [4, 9, 10]
    #[test]
    fn error_lengths_match_oracle_ten_base_adapter() {
        assert_eq!(error_lengths(10, 0.2), vec![4, 9, 10]);
    }

    // $ cutadapt -a AGATCGGAAGAGC -e 0 ... --json -> "error_lengths": [13]
    #[test]
    fn error_lengths_zero_error_rate() {
        assert_eq!(error_lengths(13, 0.0), vec![13]);
    }

    // $ .work/oracle-venv/bin/python -c "from cutadapt.report import ErrorRanges; \
    //     print(str(ErrorRanges(8,0.1)), '|', str(ErrorRanges(20,0.1)), '|', str(ErrorRanges(23,0.1)))"
    // 1-8 bp: 0 | 1-9 bp: 0; 10-19 bp: 1; 20 bp: 2 | 1-9 bp: 0; 10-19 bp: 1; 20-23 bp: 2
    #[test]
    fn error_ranges_text_matches_cutadapt_docstrings() {
        let render = |length: usize| {
            ErrorRangesText {
                length,
                lengths: error_lengths(length, 0.1),
            }
            .to_string()
        };
        assert_eq!(render(8), "1-8 bp: 0");
        assert_eq!(render(20), "1-9 bp: 0; 10-19 bp: 1; 20 bp: 2");
        assert_eq!(render(23), "1-9 bp: 0; 10-19 bp: 1; 20-23 bp: 2");
    }

    #[test]
    fn filter_counts_total_and_present_skip_none() {
        let filtered = FilterCounts {
            too_short: Some(3),
            too_long: None,
            too_many_n: Some(0),
            ..Default::default()
        };
        assert_eq!(filtered.total(), 3);
        let present: Vec<_> = filtered.present().collect();
        assert_eq!(present, vec![("too_short", 3), ("too_many_n", 0)]);
    }

    #[test]
    fn adjacent_bases_warnbase_needs_twenty_removals() {
        let mut bases = AdjacentBases {
            a: 19,
            ..Default::default()
        };
        assert_eq!(bases.warnbase(), None, "19 removals is below the threshold");
        bases.a = 20;
        assert_eq!(bases.warnbase(), Some("A"));
    }

    #[test]
    fn adjacent_bases_warnbase_is_none_for_the_empty_key() {
        let bases = AdjacentBases {
            none: 100,
            ..Default::default()
        };
        assert_eq!(bases.warnbase(), None);
    }

    #[test]
    fn stats_aggregates_optional_counters() {
        let stats = Stats {
            n: 10,
            total_bp1: 460,
            written: 7,
            written_bp1: 313,
            quality_trimmed_bp1: Some(5),
            ..Default::default()
        };
        assert_eq!(stats.quality_trimmed(), Some(5));
        assert_eq!(stats.poly_a_trimmed(), None);
        assert_eq!(stats.quality_trimmed_fraction(), 5.0 / 460.0);
        assert!(!stats.has_warnbase());
    }
}
