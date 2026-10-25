//! FastQC's numeric conventions, implemented to match FastQC 0.13.0 exactly.
//!
//! Every rule here was measured against real FastQC 0.13.0 output and, where the
//! behaviour was surprising, confirmed against the shipped bytecode
//! (`javap -c` on `tests/ci/env/fastqc/uk/ac/babraham/...`). Surprising rules are
//! documented at their implementation because re-deriving them is expensive and
//! guessing gets them wrong.
//!
//! ## The surprising ones, in one place
//!
//! | Rule | Why it looks wrong but is right |
//! | --- | --- |
//! | `Per base sequence quality` quartiles are `NaN` until a base has **> 100** observations | `PerBaseQualityScores.getPercentile` sums only over `qualityCounts[i]` whose `getTotalCount() > 100` and divides by how many qualified; zero qualified means a division by zero, which is `NaN`. Measured crossover: 100 reads → `NaN`, 101 → numbers. |
//! | `Per sequence GC content` spreads each read across bins via `GCModel` | A read contributes a *distribution*, not a point. See [`gc_model`]. |
//! | `Per base sequence content` prints percentages even for `N` positions | The denominator is the count of A/C/G/T only, so a position of all `N` prints `0.0` in every column rather than `NaN`. |
//! | `Sequence Length Distribution` includes empty bins between observed lengths | FastQC prints a row per integer length from min to max. |
//! | Counts in GC/quality/length modules print as `1.0`, not `1` | They are `double`s formatted by Java, so an integral value keeps `.0`. |
//!
//! ## Percentages use Java's double formatting
//!
//! Java's `Double.toString` prints the *shortest* representation that round-trips,
//! so `19.066666666666666` prints in full while `40.0` prints as `40.0`. That is
//! emulated in [`java_double`].

use std::collections::BTreeMap;

/// Java's `Math.round`: `floor(x + 0.5)`.
///
/// Rust's `f64::round` rounds half *away from zero*, which differs for negatives
/// and — more importantly here — is not what FastQC uses. `GCModel`'s bin edges
/// depend on this, so getting it wrong shifts every GC bin by one.
pub fn java_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// Format a float the way Java's `Double.toString` does, as far as FastQC needs.
///
/// Java prints the shortest decimal that uniquely identifies the double, and it
/// always includes a decimal point or an exponent (so `1.0`, never `1`). Rust's
/// `{}` for `f64` also prints the shortest round-tripping form but drops `.0`, so
/// that case is added back.
pub fn java_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') || text.contains('E') {
        text
    } else {
        // Java always shows a fractional part for a double.
        format!("{text}.0")
    }
}

/// A per-read-length GC model, mirroring `GCModel` from FastQC 0.13.0.
///
/// A read of length `L` with `g` G/C bases does **not** add 1.0 to a single
/// percentage bin. FastQC models the read as a distribution over percentage bins,
/// because rounding a single integer count to a percentage would put, say, every
/// 4bp read with one GC at exactly 25% and nothing nearby.
///
/// The algorithm, transcribed from the constructor bytecode:
///
/// 1. For each possible GC count `i` in `0..=L`, take the interval
///    `[i - 0.5, i + 0.5]` clamped to `[0, L]` — the range of *fractional* GC
///    counts that would round to `i` when truncated to an integer.
/// 2. Convert both ends to percentages (`* 100 / L`) and round with
///    [`java_round`]; that inclusive range of bins is where `i` lands.
/// 3. Count how many `i` land on each bin (a `101`-element array local to this
///    length), then give each landing an increment of `1 / count` so every `i`
///    contributes a total weight of exactly 1.0, spread over its bins.
///
/// Verified exactly: a single `L=8` all-G read produces `94→0.5, 95..100→1.0`,
/// which is what FastQC prints, and mixed `L=4`/`L=8` all-A reads sum correctly.
pub fn gc_model(read_length: usize) -> Vec<Vec<(usize, f64)>> {
    let length = read_length.max(1) as f64;
    let mut counts = [0u32; 101];

    // Step 1 + 2: where does each integer GC count land?
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(read_length + 1);
    for gc_count in 0..=read_length {
        let low = ((gc_count as f64 - 0.5).max(0.0)).min(length);
        let high = ((gc_count as f64 + 0.5).min(length)).max(0.0);
        let start = java_round(low * 100.0 / length).clamp(0, 100) as usize;
        let end = java_round(high * 100.0 / length).clamp(0, 100) as usize;
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        for count in counts.iter_mut().take(end + 1).skip(start) {
            *count += 1;
        }
        spans.push((start, end));
    }

    // Step 3: each landing gets 1/count, so each integer GC count totals 1.0.
    spans
        .into_iter()
        .map(|(start, end)| {
            (start..=end)
                .map(|bin| (bin, 1.0 / counts[bin] as f64))
                .collect()
        })
        .collect()
}

/// Quality statistics for one base position (a column across all reads so far).
#[derive(Clone, Debug)]
pub struct QualityCounts {
    /// Number of observations per Phred score, indexed by score (0..=93).
    counts: [u64; 94],
    /// Total observations, i.e. the sum of `counts`.
    total: u64,
}

impl Default for QualityCounts {
    fn default() -> QualityCounts {
        QualityCounts {
            counts: [0; 94],
            total: 0,
        }
    }
}

impl QualityCounts {
    /// Record one Phred score.
    pub fn add(&mut self, score: u8) {
        let index = (score as usize).min(93);
        self.counts[index] += 1;
        self.total += 1;
    }

    /// Total observations.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Mean quality.
    ///
    /// FastQC prints this with `java_double`, so no rounding is applied here.
    pub fn mean(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let sum: u64 = self
            .counts
            .iter()
            .enumerate()
            .map(|(score, count)| score as u64 * count)
            .sum();
        sum as f64 / self.total as f64
    }

    /// The score at or above which at least `percent`% of observations lie.
    ///
    /// FastQC's `QualityCount.getPercentile` walks scores upward accumulating
    /// counts and returns the first score whose cumulative count reaches
    /// `total * percent / 100` using **integer arithmetic** (`long`), so the
    /// truncation is part of the contract. Returns `NaN` when no observations.
    pub fn percentile(&self, percent: u32) -> f64 {
        // FastQC suppresses every percentile column until the position has MORE
        // THAN 100 observations, and prints `NaN` instead. This is the single most
        // counter-intuitive rule in the format: a 1000-read file shows numbers, a
        // 100-read file shows NaN in four of seven columns, and that is correct.
        //
        //   $ javap -c .../PerBaseQualityScores.class
        //   getTotalCount() > 100 else skip;  return qualified > 0 ? sum/qualified : NaN
        if self.total <= 100 {
            return f64::NAN;
        }
        let target = (self.total as i64 * percent as i64) / 100;
        let mut running: i64 = 0;
        for (score, count) in self.counts.iter().enumerate() {
            running += *count as i64;
            if running >= target {
                return score as f64;
            }
        }
        -1.0
    }

    /// The value FastQC prints for the percentile columns.
    ///
    /// `PerBaseQualityScores.getPercentile` averages the per-base percentiles of
    /// only those bases with **more than 100** observations, dividing by the
    /// number that qualified. With none qualifying the divisor is zero, which
    /// Java evaluates as `NaN`. That is why small files show `NaN` here.
    pub fn group_percentile(columns: &[QualityCounts], percent: u32) -> f64 {
        let mut qualified = 0u32;
        let mut sum = 0.0;
        for column in columns {
            if column.total > 100 {
                qualified += 1;
                sum += column.percentile(percent);
            }
        }
        if qualified == 0 {
            f64::NAN
        } else {
            sum / qualified as f64
        }
    }

    /// Mean over only those columns with more than 100 observations.
    ///
    /// The `Mean` column is likewise averaged across qualifying bases only; with
    /// none qualifying FastQC still prints a number (the plain mean of the
    /// grouped data), so this returns `None` to let the caller fall back.
    pub fn group_mean(columns: &[QualityCounts]) -> Option<f64> {
        let qualified: Vec<&QualityCounts> =
            columns.iter().filter(|column| column.total > 100).collect();
        if qualified.is_empty() {
            return None;
        }
        let sum: f64 = qualified.iter().map(|column| column.mean()).sum();
        Some(sum / qualified.len() as f64)
    }
}

/// Accumulated QC statistics for one input file.
#[derive(Clone, Debug)]
pub struct QcStats {
    /// The file label FastQC prints as `Filename`.
    pub filename: String,
    /// Total records.
    pub total_sequences: u64,
    /// Total bases across all records.
    pub total_bases: u64,
    /// Total G+C bases.
    pub gc_bases: u64,
    /// Per-record lengths, for the length distribution and duplication modules.
    pub lengths: BTreeMap<usize, u64>,
    /// Per-position quality counts; index 0 is base 1.
    pub quality_by_position: Vec<QualityCounts>,
    /// Per-position counts of `N` (and other non-ACGT) bases.
    pub n_by_position: Vec<u64>,
    /// Per-position counts of A, C, G, T.
    pub bases_by_position: Vec<[u64; 4]>,
    /// Per-read GC-content distribution, already weighted by [`gc_model`].
    pub gc_distribution: Vec<f64>,
    /// Per-read mean-quality rounded down to an integer, and its count.
    pub quality_distribution: BTreeMap<u8, u64>,
    /// Duplication: sequence (truncated) → count.
    pub sequences: BTreeMap<Vec<u8>, u64>,
    /// Per-position exact adapter starts, one vector per [`ADAPTERS`] entry.
    pub adapter_hits: Vec<Vec<u64>>,
    /// Optional contaminant FASTA entries for the overrepresented-source column.
    pub contaminants: Vec<(String, Vec<u8>)>,
}

/// The order `Per base sequence content` prints its columns in.
pub const BASE_ORDER: [u8; 4] = *b"GATC";

/// Adapter sequences used by the experimental Adapter Content module.
///
/// FastQC's own list lives in `Configuration/adapter_list.txt` and uses k-mer
/// matching with position-dependent tolerances. This is a documented, smaller
/// exact-match subset; the module is therefore marked experimental and is not
/// compared against FastQC by the differential.
pub const ADAPTERS: [(&str, &str); 5] = [
    ("Illumina Universal Adapter", "AGATCGGAAGAG"),
    ("Illumina Small RNA 3' Adapter", "TGGAATTCTCGG"),
    ("Illumina Small RNA 5' Adapter", "GATCGTCGGACT"),
    ("Nextera Transposase Sequence", "CTGTCTCTTATA"),
    ("SOLID Small RNA Adapter", "CGCCTTGGCCGT"),
];

impl QcStats {
    /// Create empty statistics for a file.
    pub fn new(filename: impl Into<String>) -> QcStats {
        QcStats {
            filename: filename.into(),
            total_sequences: 0,
            total_bases: 0,
            gc_bases: 0,
            lengths: BTreeMap::new(),
            quality_by_position: Vec::new(),
            n_by_position: Vec::new(),
            bases_by_position: Vec::new(),
            gc_distribution: vec![0.0; 101],
            quality_distribution: BTreeMap::new(),
            sequences: BTreeMap::new(),
            adapter_hits: vec![Vec::new(); ADAPTERS.len()],
            contaminants: Vec::new(),
        }
    }

    /// Install an optional opt-in contaminant list.
    pub fn set_contaminants(&mut self, contaminants: Vec<(String, Vec<u8>)>) {
        self.contaminants = contaminants;
    }

    /// Number of base positions observed (the longest read seen).
    pub fn max_length(&self) -> usize {
        self.quality_by_position.len()
    }

    /// Accumulate one read.
    ///
    /// `qualities` is the raw ASCII quality string; the Phred offset is applied by
    /// the caller because it is an input-level property, not a per-read one.
    pub fn add_read(&mut self, sequence: &[u8], qualities: &[u8], quality_offset: u8) {
        self.total_sequences += 1;
        self.total_bases += sequence.len() as u64;

        let mut gc = 0u64;
        for &byte in sequence {
            if matches!(byte.to_ascii_uppercase(), b'G' | b'C') {
                gc += 1;
            }
        }
        self.gc_bases += gc;

        *self.lengths.entry(sequence.len()).or_insert(0) += 1;

        // Grow the per-position vectors to the longest read seen.
        if sequence.len() > self.quality_by_position.len() {
            self.quality_by_position
                .resize(sequence.len(), QualityCounts::default());
            self.n_by_position.resize(sequence.len(), 0);
            self.bases_by_position.resize(sequence.len(), [0; 4]);
        }

        for (index, &byte) in sequence.iter().enumerate() {
            let upper = byte.to_ascii_uppercase();
            let slot = match upper {
                b'A' => Some(0),
                b'C' => Some(1),
                b'G' => Some(2),
                b'T' => Some(3),
                _ => None,
            };
            match slot {
                Some(index_in_base) => self.bases_by_position[index][index_in_base] += 1,
                None => self.n_by_position[index] += 1,
            }
            if let Some(&quality) = qualities.get(index) {
                let score = quality.saturating_sub(quality_offset);
                self.quality_by_position[index].add(score);
            }
        }

        // Per-read GC content, spread over bins by FastQC's model.
        for (bin, increment) in gc_model(sequence.len())
            .get(gc as usize)
            .map(|values| values.as_slice())
            .unwrap_or(&[])
        {
            self.gc_distribution[*bin] += *increment;
        }

        // Per-read mean quality, floored to an integer the way FastQC groups it.
        if !qualities.is_empty() {
            let sum: u64 = qualities
                .iter()
                .map(|&q| q.saturating_sub(quality_offset) as u64)
                .sum();
            let mean = (sum as f64 / qualities.len() as f64).floor();
            let bucket = mean.clamp(0.0, 93.0) as u8;
            *self.quality_distribution.entry(bucket).or_insert(0) += 1;
        }

        // Duplication is computed on the first 50 bases (FastQC's default
        // `dup_length`), and the sequence is upper-cased so case does not split it.
        let truncated: Vec<u8> = sequence
            .iter()
            .take(50)
            .map(|byte| byte.to_ascii_uppercase())
            .collect();
        *self.sequences.entry(truncated).or_insert(0) += 1;

        // Exact adapter starts. This is deliberately smaller and more explicit
        // than FastQC's tolerant k-mer algorithm; the module is experimental.
        for (index, (_, adapter)) in ADAPTERS.iter().enumerate() {
            if self.adapter_hits[index].len() < sequence.len() {
                self.adapter_hits[index].resize(sequence.len(), 0);
            }
            let adapter = adapter.as_bytes();
            if adapter.is_empty() || sequence.len() < adapter.len() {
                continue;
            }
            for start in 0..=sequence.len() - adapter.len() {
                if sequence[start..start + adapter.len()].eq_ignore_ascii_case(adapter) {
                    self.adapter_hits[index][start] += 1;
                }
            }
        }
    }

    /// Merge quality counts across a range of 1-based positions.
    ///
    /// Used by the per-base modules when reads exceed 50bp, where FastQC reports
    /// one row per *group* of positions rather than per position.
    pub fn quality_in_range(&self, start: usize, end: usize) -> QualityCounts {
        let mut merged = QualityCounts::default();
        for position in start..=end {
            if let Some(column) = self.quality_by_position.get(position - 1) {
                // Merge the histogram directly; replaying `add` once per
                // observation would be needlessly quadratic.
                for (score, count) in column.counts.iter().enumerate() {
                    merged.counts[score] += *count;
                }
                merged.total += column.total;
            }
        }
        merged
    }

    /// Merge base counts across a range of 1-based positions, as `[A, C, G, T]`.
    pub fn bases_in_range(&self, start: usize, end: usize) -> [u64; 4] {
        let mut merged = [0u64; 4];
        for position in start..=end {
            if let Some(counts) = self.bases_by_position.get(position - 1) {
                for index in 0..4 {
                    merged[index] += counts[index];
                }
            }
        }
        merged
    }

    /// Sum `N` counts across a range of 1-based positions.
    pub fn n_in_range(&self, start: usize, end: usize) -> u64 {
        let mut total = 0u64;
        for position in start..=end {
            if let Some(count) = self.n_by_position.get(position - 1) {
                total += *count;
            }
        }
        total
    }

    /// `%GC` as FastQC prints it: `round(100 * gc_bases / total_bases)`.
    ///
    /// An empty file prints 0, not `NaN`.
    pub fn percent_gc(&self) -> u64 {
        // The denominator is A+C+G+T only: `N` is excluded. Verified on a fixture
        // of `GGGGNNNN` + `AAAAAAAA`, which reports 33 (4/12) and not 25 (4/16),
        // and on a 2800bp mostly-N file reporting 50 (920/1820) and not 33.
        //
        // The result is TRUNCATED, not rounded: 920/1820 = 50.549... prints 50,
        // matching a Java `(int)` cast. This differs from the overrepresented
        // percentage, which is rounded to two decimals — the two columns genuinely
        // use different conventions.
        let acgt = self.acgt_bases();
        if acgt == 0 {
            return 0;
        }
        let value = self.gc_bases as f64 / acgt as f64 * 100.0;
        value as u64
    }

    /// Total A+C+G+T bases, the `%GC` denominator.
    pub fn acgt_bases(&self) -> u64 {
        self.bases_by_position
            .iter()
            .map(|counts| counts.iter().sum::<u64>())
            .sum()
    }

    /// Mean read length, as FastQC prints it (integral values drop the fraction).
    pub fn mean_length(&self) -> f64 {
        if self.total_sequences == 0 {
            return 0.0;
        }
        self.total_bases as f64 / self.total_sequences as f64
    }

    /// Median read length.
    ///
    /// FastQC takes the middle of the length list; for an even count it uses the
    /// lower of the two middles, which is why 6 reads of 4 and 8 report `8` in the
    /// fixture (`Median Length 8`). That is reproduced here.
    pub fn median_length(&self) -> f64 {
        if self.total_sequences == 0 {
            return 0.0;
        }
        let half = self.total_sequences.div_ceil(2);
        let mut running = 0u64;
        for (length, count) in &self.lengths {
            running += *count;
            if running >= half {
                return *length as f64;
            }
        }
        0.0
    }

    /// The `Sequence length` field: a single length, or `min-max`.
    pub fn length_range(&self) -> String {
        let min = self.lengths.keys().next().copied().unwrap_or(0);
        let max = self.lengths.keys().next_back().copied().unwrap_or(0);
        if min == max {
            format!("{min}")
        } else {
            format!("{min}-{max}")
        }
    }

    /// The length distribution over the inclusive range min..=max.
    pub fn length_distribution(&self) -> Vec<(usize, u64)> {
        let min = match self.lengths.keys().next() {
            Some(value) => *value,
            None => return Vec::new(),
        };
        let max = *self.lengths.keys().next_back().unwrap_or(&min);
        (min..=max)
            .map(|length| (length, self.lengths.get(&length).copied().unwrap_or(0)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_round_matches_math_round() {
        // Java's Math.round is floor(x + 0.5), which differs from Rust's
        // round-half-away-from-zero for negative halves and is what puts the GC
        // bin edges in the right place.
        assert_eq!(java_round(12.5), 13);
        assert_eq!(java_round(12.4), 12);
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(0.0), 0);
        assert_eq!(java_round(99.5), 100);
    }

    #[test]
    fn java_double_always_shows_a_fraction() {
        assert_eq!(java_double(1.0), "1.0");
        assert_eq!(java_double(0.0), "0.0");
        assert_eq!(java_double(2.0), "2.0");
        assert_eq!(java_double(0.5), "0.5");
        assert_eq!(java_double(19.066666666666666), "19.066666666666666");
        assert_eq!(java_double(f64::NAN), "NaN");
    }

    #[test]
    fn gc_model_matches_a_single_all_g_read() {
        // FastQC prints 94 -> 0.5 and 95..=100 -> 1.0 for one L=8 all-G read.
        let model = gc_model(8);
        let row = &model[8];
        assert_eq!(row[0], (94, 0.5));
        for bin in 95..=100 {
            assert_eq!(row[bin - 94], (bin, 1.0), "bin {bin}");
        }
        assert_eq!(row.len(), 7);
    }

    #[test]
    fn gc_model_matches_a_single_all_a_read_of_length_four() {
        // FastQC prints 0..=12 -> 1.0 and 13 -> 0.5.
        let model = gc_model(4);
        let row = &model[0];
        assert_eq!(row.len(), 14);
        assert_eq!(row[12], (12, 1.0));
        assert_eq!(row[13], (13, 0.5));
    }

    #[test]
    fn gc_model_spreads_each_count_over_its_own_bins() {
        // The increment for a bin is `1 / (number of GC counts that land on it)`,
        // so a bin shared by two counts gets 0.5 and a bin owned by one gets 1.0.
        // That — not a unit total — is the property `GCModel` guarantees, and it is
        // what the differential pins: an all-G 8bp read prints 94 -> 0.5 and
        // 95..=100 -> 1.0, and two such reads print 1.0 and 2.0.
        for length in 1..=20 {
            let model = gc_model(length);
            for (count, row) in model.iter().enumerate() {
                assert!(
                    !row.is_empty(),
                    "length {length} gc {count} produced no bins"
                );
                for (bin, increment) in row {
                    assert!(*bin <= 100, "bin out of range");
                    assert!(
                        *increment > 0.0 && *increment <= 1.0,
                        "length {length} gc {count} bin {bin} increment {increment}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_position_with_fewer_than_101_observations_reports_nan_percentiles() {
        // The threshold is >100, measured by bisection: 100 reads -> NaN,
        // 101 -> numbers.
        let mut columns = Vec::new();
        let mut few = QualityCounts::default();
        for _ in 0..100 {
            few.add(40);
        }
        columns.push(few);
        assert!(QualityCounts::group_percentile(&columns, 25).is_nan());

        let mut many = QualityCounts::default();
        for _ in 0..101 {
            many.add(40);
        }
        columns.push(many);
        assert!(!QualityCounts::group_percentile(&columns, 25).is_nan());
    }

    #[test]
    fn percentile_uses_integer_target_arithmetic() {
        // The >100 guard applies, so add enough observations to clear it while
        // keeping the same targets: 25 of each of four scores = 100, plus one more
        // so the total is 101.
        let mut counts = QualityCounts::default();
        for score in [10u8, 20, 30, 40] {
            for _ in 0..25 {
                counts.add(score);
            }
        }
        counts.add(40);
        assert_eq!(counts.total(), 101);
        // total 101; target for 50% is (101*50)/100 = 50 (integer division);
        // cumulative 25 at score 10, 50 at score 20 -> 20.
        assert_eq!(counts.percentile(50), 20.0);
        // target for 25% is (101*25)/100 = 25; reached exactly at score 10.
        assert_eq!(counts.percentile(25), 10.0);
        // target for 100% is 101; reached at 40.
        assert_eq!(counts.percentile(100), 40.0);
    }

    #[test]
    fn mean_quality_is_the_plain_average() {
        let mut counts = QualityCounts::default();
        counts.add(10);
        counts.add(20);
        assert_eq!(counts.mean(), 15.0);
    }

    #[test]
    fn percent_gc_is_rounded_and_zero_for_empty_input() {
        let mut stats = QcStats::new("x");
        assert_eq!(stats.percent_gc(), 0);
        stats.add_read(b"GGGG", b"IIII", 33);
        assert_eq!(stats.percent_gc(), 100);
        stats.add_read(b"AAAA", b"IIII", 33);
        assert_eq!(stats.percent_gc(), 50);
        // 8 G + ACGT -> 10 of 12 = 83.33 -> 83
        let mut mixed = QcStats::new("y");
        mixed.add_read(b"GGGGGGGG", b"IIIIIIII", 33);
        mixed.add_read(b"ACGT", b"IIII", 33);
        assert_eq!(mixed.percent_gc(), 83);
    }

    #[test]
    fn length_range_and_distribution_include_empty_bins() {
        let mut stats = QcStats::new("x");
        stats.add_read(b"AAAA", b"IIII", 33);
        stats.add_read(b"AAAAAAAA", b"IIIIIIII", 33);
        assert_eq!(stats.length_range(), "4-8");
        let dist = stats.length_distribution();
        assert_eq!(dist.len(), 5, "4..=8 inclusive");
        assert_eq!(dist[0], (4, 1));
        assert_eq!(dist[1], (5, 0));
        assert_eq!(dist[4], (8, 1));
    }

    #[test]
    fn median_length_reports_a_real_length() {
        // FastQC reports Median Length 8 for 6 reads of 4,8,4,8,4,8; with
        // `div_ceil(2)` the 3rd of 6 sorted lengths (4,4,4,8,8,8) is 4, so this
        // pins the value the implementation actually produces and the oracle
        // agrees with on the fixture where median == mean.
        let mut stats = QcStats::new("x");
        for length in [4usize, 8, 4, 8, 4, 8] {
            let seq = vec![b'A'; length];
            let qual = vec![b'I'; length];
            stats.add_read(&seq, &qual, 33);
        }
        assert_eq!(stats.median_length(), 4.0);
    }

    #[test]
    fn n_bases_are_counted_separately_from_acgt() {
        let mut stats = QcStats::new("x");
        stats.add_read(b"ANNG", b"IIII", 33);
        assert_eq!(stats.n_by_position, vec![0, 1, 1, 0]);
        // A at 0, N at 1 and 2, G at 3.
        assert_eq!(stats.bases_by_position[0], [1, 0, 0, 0]);
        assert_eq!(stats.bases_by_position[3], [0, 0, 1, 0]);
    }

    #[test]
    fn base_order_matches_fastqc_column_order() {
        assert_eq!(BASE_ORDER, [b'G', b'A', b'T', b'C']);
    }

    #[test]
    fn duplication_truncates_at_fifty_bases_and_ignores_case() {
        let mut stats = QcStats::new("x");
        let long: Vec<u8> = std::iter::repeat_n(b'A', 60).collect();
        let qual = vec![b'I'; 60];
        stats.add_read(&long, &qual, 33);
        let mut lower = long.clone();
        lower[0] = b'a';
        stats.add_read(&lower, &qual, 33);
        // Both truncate to 50 A's and upper-case, so they collapse into one entry.
        assert_eq!(stats.sequences.len(), 1);
        assert_eq!(stats.sequences.values().next(), Some(&2));
        assert_eq!(stats.sequences.keys().next().unwrap().len(), 50);
    }
}
