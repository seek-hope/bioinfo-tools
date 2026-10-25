//! Rendering the `stats` table in SeqKit's two documented layouts.
//!
//! # Why this is its own module
//!
//! The layout is the compatibility surface, and it has several conventions that
//! are easy to get subtly wrong:
//!
//! * **Thousands separators in the human table.** `28,645`, `2,949,871`.
//! * **No thousands separators in `-T` (tabular) mode.** `28645`, `2949871` —
//!   that is the whole point of the flag.
//! * **Right-aligned numbers, left-aligned text.** The first three columns are
//!   text and left-aligned; every metric is right-aligned.
//! * **`avg_len` is the one column that is not an integer.** In the human table
//!   it prints without a forced decimal (`103`, `21.8`, `449.7`); in `-T` mode it
//!   always carries at least one decimal (`103.0`, `21.8`). Both appear in the
//!   published examples.
//! * **A dashed rule under the header**, whose width tracks the widest cell in
//!   each column.
//!
//! Source for all of the above: the published `seqkit stats` examples at
//! `https://bioinf.shenwei.me/seqkit/usage/` (retrieved 2026-10-20). The SeqKit
//! binary is **not available here**, so these are documentation-derived and are
//! recorded as `experimental` in `docs/EVIDENCE-W1-3.md` (DR-1, DR-3).

use std::collections::BTreeMap;

/// One row of the stats table.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsRow {
    /// Input label: the path as given, or `-` for standard input.
    pub file: String,
    /// `FASTA` or `FASTQ`.
    pub format: &'static str,
    /// `DNA`, `RNA`, `Protein` or `Unlimit`.
    pub seq_type: &'static str,
    /// Number of records.
    pub num_seqs: u64,
    /// Total bases, gaps included.
    pub sum_len: u64,
    /// Shortest sequence.
    pub min_len: u64,
    /// Mean length.
    pub avg_len: f64,
    /// Longest sequence.
    pub max_len: u64,
    /// First quartile of length (only in `-a`).
    pub q1: Option<u64>,
    /// Median length (only in `-a`).
    pub q2: Option<u64>,
    /// Third quartile of length (only in `-a`).
    pub q3: Option<u64>,
    /// Gap characters (only in `-a`).
    pub sum_gap: Option<u64>,
    /// N50 (only in `-a`).
    pub n50: Option<u64>,
    /// L50 (only in `-a`).
    pub n50_num: Option<u64>,
    /// Percentage of bases above Q20 (only in `-a`).
    pub q20: Option<f64>,
    /// Percentage of bases above Q30 (only in `-a`).
    pub q30: Option<f64>,
    /// Mean error-derived quality (only in `-a`).
    pub avg_qual: Option<f64>,
    /// GC percentage (only in `-a`).
    pub gc: Option<f64>,
    /// Ambiguous base count (only in `-a`).
    pub sum_n: Option<u64>,
}

/// Which layout to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Human table: aligned columns, thousands separators, dashed rule.
    Human,
    /// `-T`: tab-separated, no separators, no rule.
    Tabular,
}

/// Format an integer with thousands separators (`28645` -> `28,645`).
pub fn with_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Render a float the way the human stats table does: no trailing `.0`, and at
/// most one decimal place unless the value needs more to stay accurate.
///
/// The published examples show `103`, `21.8`, `227`, `449.7` — so an integral
/// mean prints with no decimal part at all. Non-integral values are printed to
/// one decimal, which is what every published example shows.
pub fn human_float(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded:.1}")
    }
}

/// Render a float the way `-T` does: always at least one decimal place.
pub fn tabular_float(value: f64) -> String {
    if !value.is_finite() {
        return "0.0".to_string();
    }
    let rounded = (value * 10.0).round() / 10.0;
    format!("{rounded:.1}")
}

/// Render a percentage: always exactly two decimal places.
///
/// Verified against SeqKit 2.14.0, which never trims. `GC(%)`, `Q20(%)`,
/// `Q30(%)`, `fx2tab -g` and `fx2tab -B/-C` all print two decimals even for
/// round values:
///
///   GC 50%      -> `50.00`   (was `50`)
///   GC 100%     -> `100.00`  (was `100`)
///   GC 0%       -> `0.00`    (was `0`)
///   GC 42.857%  -> `42.86`
///
/// An earlier version trimmed trailing zeros because the plan's quoted examples
/// showed `47.6` and `0`; the oracle disagrees, and the oracle is authoritative.
pub fn percent(value: f64) -> String {
    if !value.is_finite() {
        return "0.00".to_string();
    }
    let rounded = (value * 100.0).round() / 100.0;
    format!("{rounded:.2}")
}

/// Drop trailing zeros from a fixed-point string: `"93.50"` -> `"93.5"`,
/// `"40.00"` -> `"40"`. Used by the human layout, which prints the shortest
/// exact form, unlike the tabular layout which keeps a fixed width.
fn trim_trailing_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let mut out = text.to_string();
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out
}

/// The header labels, in the documented column order.
pub fn header(all: bool) -> Vec<&'static str> {
    let mut columns = vec![
        "file", "format", "type", "num_seqs", "sum_len", "min_len", "avg_len", "max_len",
    ];
    if all {
        columns.extend([
            "Q1", "Q2", "Q3", "sum_gap", "N50", "N50_num", "Q20(%)", "Q30(%)", "AvgQual", "GC(%)",
            "sum_n",
        ]);
    }
    columns
}

/// A cell renderer: takes an optional value and produces the column text.
type Cell = fn(Option<f64>) -> String;

/// `AvgQual` in the tabular layout: always two decimals, `NaN` when undefined.
fn tabular_avg_qual(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v:.2}"),
        Some(_) => "NaN".to_string(),
        None => String::new(),
    }
}

/// The cells of one row, as strings, in the documented column order.
pub fn cells(row: &StatsRow, layout: Layout) -> Vec<String> {
    let number = |value: u64| match layout {
        Layout::Human => with_thousands(value),
        Layout::Tabular => value.to_string(),
    };
    let float = |value: f64| match layout {
        Layout::Human => human_float(value),
        Layout::Tabular => tabular_float(value),
    };
    let optional_number = |value: Option<u64>| match value {
        Some(v) => number(v),
        None => String::new(),
    };
    let optional_percent = |value: Option<f64>| match value {
        Some(v) => percent(v),
        None => String::new(),
    };
    // `Q20(%)`/`Q30(%)` are rendered DIFFERENTLY in the two layouts, and
    // `AvgQual` follows the same split. Both were measured against SeqKit 2.14.0:
    //
    //   fraction   HUMAN Q20   TABULAR Q20
    //   58/62      93.55       94
    //   2/3        66.67       67
    //   5/8        62.5        62
    //   3/4        75          75
    //   1/2        50          50
    //
    // So the human layout keeps up to two decimals and trims trailing zeros,
    // while the tabular layout rounds to a whole number. `AvgQual` is the mirror
    // image: the human layout trims (`40`, `11.9`) and the tabular layout always
    // shows two decimals (`40.00`, `11.90`). This asymmetry is real, not a bug in
    // the comparison.
    let trimmed_percent = |value: Option<f64>| match value {
        Some(v) if v.is_finite() => trim_trailing_zeros(&format!("{v:.2}")),
        Some(_) => "0".to_string(),
        None => String::new(),
    };
    let rounded_percent = |value: Option<f64>| match value {
        Some(v) if v.is_finite() => format!("{}", v.round() as i64),
        Some(_) => "0".to_string(),
        None => String::new(),
    };
    let (q20_cell, q30_cell, avg_qual_cell): (Cell, Cell, Cell) = match layout {
        Layout::Human => (trimmed_percent, trimmed_percent, trimmed_percent),
        Layout::Tabular => (rounded_percent, rounded_percent, tabular_avg_qual),
    };

    vec![
        row.file.clone(),
        row.format.to_string(),
        row.seq_type.to_string(),
        number(row.num_seqs),
        number(row.sum_len),
        number(row.min_len),
        float(row.avg_len),
        number(row.max_len),
        optional_number(row.q1),
        optional_number(row.q2),
        optional_number(row.q3),
        optional_number(row.sum_gap),
        optional_number(row.n50),
        optional_number(row.n50_num),
        q20_cell(row.q20),
        q30_cell(row.q30),
        avg_qual_cell(row.avg_qual),
        optional_percent(row.gc),
        optional_number(row.sum_n),
    ]
}

/// The first three columns are text and left-aligned; the rest are numbers and
/// right-aligned.
const LEFT_ALIGNED: usize = 3;

/// Render the whole table.
///
/// A trailing newline is included, because both documented layouts end with one.
pub fn render(rows: &[StatsRow], all: bool, layout: Layout) -> String {
    let labels = header(all);
    let width = if all { labels.len() } else { 8 };
    let labels: Vec<&str> = labels.into_iter().take(width).collect();
    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let mut cells = cells(row, layout);
            cells.truncate(width);
            cells
        })
        .collect();

    if layout == Layout::Tabular {
        let mut out = String::new();
        out.push_str(&labels.join("\t"));
        out.push('\n');
        for cells in &body {
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        return out;
    }

    // Human layout: size each column to its widest cell, then pad.
    //
    // Two details are oracle-verified against SeqKit 2.14.0 and are NOT the
    // "markdown table" layout an earlier version emitted:
    //
    //   1. There is no dashed rule line under the header. SeqKit prints the
    //      header then the rows, nothing between.
    //   2. Columns are separated by exactly TWO spaces, not three. The apparent
    //      three-space gap after `file` in
    //          `file   format  type  ...`
    //      is a two-space separator plus the one space of padding that the
    //      left-aligned `file` column carries before the next column starts.
    //
    //     $ seqkit stats in.fq
    //     file   format  type  num_seqs  sum_len  min_len  avg_len  max_len
    //     in.fq  FASTQ   DNA          4       62        4     15.5       34
    let mut widths: Vec<usize> = labels.iter().map(|label| label.len()).collect();
    for cells in &body {
        for (index, cell) in cells.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }

    let mut out = String::new();
    out.push_str(&pad_row(
        &labels.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &widths,
    ));
    out.push('\n');
    for cells in &body {
        out.push_str(&pad_row(cells, &widths));
        out.push('\n');
    }
    out
}

fn pad_row(cells: &[String], widths: &[usize]) -> String {
    let mut out = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            // Exactly two spaces between columns; see the layout note in
            // `render`. Three produced a table one blank wider than the oracle's.
            out.push_str("  ");
        }
        if index < LEFT_ALIGNED {
            out.push_str(cell);
            out.push_str(&" ".repeat(widths[index] - cell.len()));
        } else {
            out.push_str(&" ".repeat(widths[index] - cell.len()));
            out.push_str(cell);
        }
    }
    // Trailing spaces are invisible but would break a byte comparison.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Compute N50 and L50 from sequence lengths.
///
/// N50 is the length at which half the total bases are covered when lengths are
/// sorted descending; L50 is how many sequences that took. Returns `(N50, L50)`,
/// or `(0, 0)` for no lengths.
pub fn n50(lengths: &mut [u64]) -> (u64, u64) {
    if lengths.is_empty() {
        return (0, 0);
    }
    lengths.sort_unstable_by(|a, b| b.cmp(a));
    let total: u64 = lengths.iter().sum();
    let mut running = 0u64;
    for (index, &len) in lengths.iter().enumerate() {
        running += len;
        if running * 2 >= total {
            return (len, index as u64 + 1);
        }
    }
    (0, 0)
}

/// N50/L50 from an exact length histogram.
///
/// Memory is proportional to the number of distinct lengths, not the number of
/// reads. The arithmetic is the same as [`n50`]: lengths descending, half the
/// bases covered, L50 is the number of reads consumed including the crossing
/// read. A zero-length-only input keeps the old `(0, 1)` result.
pub fn n50_from_histogram(lengths: &BTreeMap<u64, u64>) -> (u64, u64) {
    if lengths.is_empty() {
        return (0, 0);
    }
    let total: u64 = lengths
        .iter()
        .map(|(&len, &count)| len.saturating_mul(count))
        .fold(0u64, u64::saturating_add);
    let mut running = 0u64;
    let mut consumed = 0u64;
    for (&len, &count) in lengths.iter().rev() {
        if count == 0 {
            continue;
        }
        if total == 0 {
            // The old implementation returns the first (only) length and L50 1.
            return (len, consumed + 1);
        }
        let after = running.saturating_add(len.saturating_mul(count));
        if after.saturating_mul(2) >= total {
            let need = total.saturating_sub(running.saturating_mul(2));
            let k = if len == 0 {
                1
            } else {
                need.div_ceil(len.saturating_mul(2)).max(1)
            };
            return (len, consumed + k.min(count));
        }
        running = after;
        consumed += count;
    }
    (0, 0)
}

/// Quartiles from an exact length histogram.
///
/// Uses the same Tukey hinges and half-to-even rounding as [`quartiles`], but
/// reads order statistics from the histogram instead of materialising every
/// read length.
pub fn quartiles_from_histogram(lengths: &BTreeMap<u64, u64>) -> (u64, u64, u64) {
    if lengths.is_empty() {
        return (0, 0, 0);
    }
    let n: u64 = lengths.values().copied().sum();
    if n == 0 {
        return (0, 0, 0);
    }
    if n == 1 {
        let value = value_at(lengths, 0);
        return (value, value, value);
    }

    let median = |start: u64, len: u64| -> f64 {
        let mid = start + len / 2;
        if len % 2 == 1 {
            value_at(lengths, mid) as f64
        } else {
            (value_at(lengths, mid - 1) as f64 + value_at(lengths, mid) as f64) / 2.0
        }
    };
    let (low_len, high_start, high_len) = if n % 2 == 1 {
        (n / 2 + 1, n / 2, n - n / 2)
    } else {
        (n / 2, n / 2, n / 2)
    };
    (
        round_half_even(median(0, low_len)),
        round_half_even(median(0, n)),
        round_half_even(median(high_start, high_len)),
    )
}

/// The value at a 0-based rank in an ascending histogram.
fn value_at(lengths: &BTreeMap<u64, u64>, mut rank: u64) -> u64 {
    for (&length, &count) in lengths {
        if rank < count {
            return length;
        }
        rank -= count;
    }
    0
}

/// Quartiles of a length list.
///
/// The interpolation method is **not documented** by SeqKit, so it was
/// reverse-engineered from the oracle and is pinned here.
///
/// Method: **Tukey's hinges**, with the median included in both halves when the
/// count is odd, and each half-median rounded **half to even**.
///
/// This was determined empirically against SeqKit 2.14.0 over every n from 4 to
/// 20 with lengths 1..n, plus the two real fixtures used by the differential:
///
///   n=4  lengths 1..4   -> Q1=2 Q2=2 Q3=4      (oracle agrees)
///   n=8  lengths 1..8   -> Q1=2 Q2=4 Q3=6      (oracle agrees; type-7 gives 3)
///   n=20 lengths 1..20  -> Q1=6 Q2=10 Q3=16    (oracle agrees)
///   in.fq lengths [4,12,12,34] -> Q1=8 Q2=12 Q3=23
///   g.fa  lengths [10,8,4]     -> Q1=6 Q2=8 Q3=9
///
/// Nearest-rank (the previous implementation) and type-7/linear interpolation
/// both disagree on some n; Tukey hinges agree on all of them. Rounding matters:
/// for n=4 the upper half-median is 3.5, and round-half-to-even yields 4 while
/// round-half-up also yields 4, but for n=20 the value 16 comes out clean only
/// under half-to-even.
pub fn quartiles(lengths: &mut [u64]) -> (u64, u64, u64) {
    if lengths.is_empty() {
        return (0, 0, 0);
    }
    lengths.sort_unstable();
    let n = lengths.len();
    if n == 1 {
        return (lengths[0], lengths[0], lengths[0]);
    }

    // Median of a sorted run, as a float so a half can be rounded once.
    let median = |slice: &[u64]| -> f64 {
        let len = slice.len();
        if len % 2 == 1 {
            slice[len / 2] as f64
        } else {
            (slice[len / 2 - 1] as f64 + slice[len / 2] as f64) / 2.0
        }
    };

    // Tukey's hinges: an odd count shares its median with both halves.
    //
    //   low  = lengths[..n/2 + 1]   (odd)  / lengths[..n/2]   (even)
    //   high = lengths[n/2..]       (odd)  / lengths[n/2..]   (even)
    //
    // The two slices overlap on the median when the count is odd, which is the
    // defining property of Tukey's hinges. Getting `high` wrong here is what made
    // Q3 report 20 instead of 14 for lengths [4, 7, 20]: the upper half must be
    // [7, 20], not just [20].
    let (low, high) = if n % 2 == 1 {
        (&lengths[..n / 2 + 1], &lengths[n / 2..])
    } else {
        (&lengths[..n / 2], &lengths[n / 2..])
    };
    (
        round_half_even(median(low)),
        round_half_even(median(lengths)),
        round_half_even(median(high)),
    )
}

/// Round a non-negative value half to even, the way the oracle's output implies.
///
/// `f64::round` rounds half away from zero, which would turn 3.5 into 4 and 12.5
/// into 13. SeqKit's values match banker's rounding instead.
fn round_half_even(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let floor = value.floor();
    let frac = value - floor;
    if (frac - 0.5).abs() < f64::EPSILON {
        // Exactly halfway: pick the even neighbour.
        let lower = floor as u64;
        if lower.is_multiple_of(2) {
            lower
        } else {
            lower + 1
        }
    } else {
        value.round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> StatsRow {
        StatsRow {
            file: "reads.fq".to_string(),
            format: "FASTQ",
            seq_type: "DNA",
            num_seqs: 2500,
            sum_len: 567_516,
            min_len: 226,
            avg_len: 227.0,
            max_len: 229,
            q1: None,
            q2: None,
            q3: None,
            sum_gap: None,
            n50: None,
            n50_num: None,
            q20: None,
            q30: None,
            avg_qual: None,
            gc: None,
            sum_n: None,
        }
    }

    #[test]
    fn thousands_separators_group_by_three() {
        assert_eq!(with_thousands(0), "0");
        assert_eq!(with_thousands(7), "7");
        assert_eq!(with_thousands(999), "999");
        assert_eq!(with_thousands(1000), "1,000");
        assert_eq!(with_thousands(28645), "28,645");
        assert_eq!(with_thousands(2_949_871), "2,949,871");
        assert_eq!(with_thousands(1_500_000), "1,500,000");
    }

    // Every pair below is taken from the published `seqkit stats` examples.
    #[test]
    fn human_float_drops_an_integral_decimal() {
        assert_eq!(human_float(103.0), "103");
        assert_eq!(human_float(227.0), "227");
        assert_eq!(human_float(150.0), "150");
        assert_eq!(human_float(21.8), "21.8");
        assert_eq!(human_float(449.7), "449.7");
        assert_eq!(human_float(6.0), "6");
    }

    #[test]
    fn tabular_float_always_has_a_decimal() {
        assert_eq!(tabular_float(103.0), "103.0");
        assert_eq!(tabular_float(227.0), "227.0");
        assert_eq!(tabular_float(21.8), "21.8");
        assert_eq!(tabular_float(150.0), "150.0");
    }

    #[test]
    fn percentages_always_show_two_decimals() {
        // From the published -a example.
        // SeqKit 2.14.0 never trims: 50% prints 50.00, 100% prints 100.00.
        assert_eq!(percent(96.16), "96.16");
        assert_eq!(percent(89.71), "89.71");
        assert_eq!(percent(45.77), "45.77");
        assert_eq!(percent(47.6), "47.60");
        assert_eq!(percent(0.0), "0.00");
        assert_eq!(percent(100.0), "100.00");
        assert_eq!(percent(50.0), "50.00");
        assert_eq!(percent(42.857), "42.86");
    }

    #[test]
    fn header_has_eight_columns_by_default() {
        assert_eq!(header(false).len(), 8);
        assert_eq!(header(false)[0], "file");
        assert_eq!(header(false)[7], "max_len");
    }

    #[test]
    fn header_has_nineteen_columns_with_all() {
        let columns = header(true);
        assert_eq!(columns.len(), 19);
        assert_eq!(columns[8], "Q1");
        assert_eq!(columns[18], "sum_n");
    }

    #[test]
    fn tabular_layout_is_tab_separated_without_separators() {
        let rows = vec![sample_row()];
        let rendered = render(&rows, false, Layout::Tabular);
        assert_eq!(
            rendered,
            "file\tformat\ttype\tnum_seqs\tsum_len\tmin_len\tavg_len\tmax_len\n\
             reads.fq\tFASTQ\tDNA\t2500\t567516\t226\t227.0\t229\n"
        );
        assert!(
            !rendered.contains(','),
            "tabular mode must not group digits"
        );
    }

    #[test]
    fn human_layout_groups_digits_and_aligns() {
        let rows = vec![sample_row()];
        let rendered = render(&rows, false, Layout::Human);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2, "header then one row: there is no rule line");
        // No dashed rule under the header: SeqKit prints the header then rows.
        assert!(lines[1].contains("2,500"));
        assert!(rendered.contains("2,500"));
        assert!(rendered.contains("567,516"));
        // avg_len is right-aligned with one decimal place dropped.
        assert!(rendered.contains("227"));
        // No line has trailing whitespace.
        for line in &lines {
            assert_eq!(*line, line.trim_end(), "trailing space in {line:?}");
        }
    }

    #[test]
    fn human_layout_pads_columns_to_the_widest_cell() {
        let mut short = sample_row();
        short.file = "a.fq".to_string();
        short.num_seqs = 1;
        let mut long = sample_row();
        long.file = "a-much-longer-file-name.fastq.gz".to_string();
        let rendered = render(&[short, long], false, Layout::Human);
        let lines: Vec<&str> = rendered.lines().collect();
        // Every row has the same width once trailing space is stripped? Not
        // necessarily, but the header rule width must track the longest content.
        assert!(lines[0].len() >= "a-much-longer-file-name.fastq.gz".len());
        assert_eq!(lines.len(), 3, "header plus two rows");
    }

    #[test]
    fn an_empty_row_set_still_renders_a_header() {
        let rendered = render(&[], false, Layout::Tabular);
        assert_eq!(
            rendered,
            "file\tformat\ttype\tnum_seqs\tsum_len\tmin_len\tavg_len\tmax_len\n"
        );
        let human = render(&[], false, Layout::Human);
        assert_eq!(human.lines().count(), 1, "header only: no rule line");
    }

    #[test]
    fn histogram_statistics_match_the_sorted_path() {
        let cases: Vec<Vec<u64>> = vec![
            Vec::new(),
            vec![0],
            vec![0, 0],
            vec![5],
            vec![4, 7, 20],
            vec![1, 2, 3, 4],
            (1..=20).collect(),
            vec![150, 150, 150],
            vec![1, 1, 1, 2, 2, 3, 100],
            vec![100, 1, 1, 1],
        ];
        for lengths in cases {
            let mut expected_n50 = lengths.clone();
            let expected_n50 = n50(&mut expected_n50);
            let mut expected_quartiles = lengths.clone();
            let expected_quartiles = quartiles(&mut expected_quartiles);
            let mut histogram = BTreeMap::new();
            for length in lengths {
                *histogram.entry(length).or_insert(0) += 1;
            }
            assert_eq!(
                n50_from_histogram(&histogram),
                expected_n50,
                "N50 for {histogram:?}"
            );
            assert_eq!(
                quartiles_from_histogram(&histogram),
                expected_quartiles,
                "quartiles for {histogram:?}"
            );
        }
    }

    #[test]
    fn n50_matches_the_definition() {
        // Lengths 10, 9, 8, 7, 6 sum to 40; half is 20.
        // Descending: 10 (10), 9 (19), 8 (27) -> N50 is 8 after 3 sequences.
        let mut lengths = vec![10, 9, 8, 7, 6];
        assert_eq!(n50(&mut lengths), (8, 3));

        // A single sequence is its own N50.
        assert_eq!(n50(&mut [5]), (5, 1));

        // All identical.
        assert_eq!(n50(&mut [3, 3, 3, 3]), (3, 2));

        assert_eq!(n50(&mut []), (0, 0));
    }

    #[test]
    fn n50_handles_one_dominant_sequence() {
        let mut lengths = vec![100, 1, 1, 1];
        // Total 103, half 51.5; the first sequence alone (100) covers it.
        assert_eq!(n50(&mut lengths), (100, 1));
    }

    #[test]
    fn quartiles_are_monotonic_and_bounded() {
        let mut lengths = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let (q1, q2, q3) = quartiles(&mut lengths);
        assert!(q1 <= q2 && q2 <= q3, "quartiles must be ordered");
        assert!(q1 >= 1 && q3 <= 8);

        // All-identical lengths collapse to that length, which is the case the
        // published examples show.
        let mut same = vec![150, 150, 150];
        assert_eq!(quartiles(&mut same), (150, 150, 150));

        assert_eq!(quartiles(&mut []), (0, 0, 0));
    }

    #[test]
    fn cells_include_all_optionals_when_all_is_set() {
        let mut row = sample_row();
        row.q1 = Some(227);
        row.sum_gap = Some(0);
        row.gc = Some(53.63);
        row.avg_qual = Some(15.45);
        let cells = cells(&row, Layout::Human);
        assert_eq!(cells.len(), 19);
        assert_eq!(cells[8], "227");
        assert_eq!(cells[11], "0");
        assert_eq!(cells[16], "15.45", "human AvgQual keeps its two decimals");
        assert_eq!(cells[17], "53.63");
    }
}
