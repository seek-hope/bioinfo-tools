//! Read filters (`-m`, `-M`, `--max-n`, `--max-ee`, `--max-aer`, `--discard-*`).
//!
//! cutadapt evaluates filters after adapter trimming and the read modifications,
//! in a fixed order, and records *which* filter rejected each read so the report
//! can print the fate breakdown. The first filter that rejects a read wins.

/// Why a read was discarded, in cutadapt's report order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// Passed every filter.
    Written,
    /// Shorter than `-m`.
    TooShort,
    /// Longer than `-M`.
    TooLong,
    /// More than `--max-n` N bases.
    TooManyN,
    /// More expected errors than `--max-ee`.
    TooManyExpectedErrors,
    /// Average error rate above `--max-aer`.
    TooManyAverageErrors,
    /// Failed the CASAVA `Y` filter (`--discard-casava`).
    CasavaFiltered,
    /// Had an adapter and `--discard-trimmed` was given.
    DiscardTrimmed,
    /// Had no adapter and `--discard-untrimmed` was given.
    DiscardUntrimmed,
    /// `--max-average-error-rate` was given and the read became empty. cutadapt
    /// 4.6 raises ZeroDivisionError here; this is modelled as a fatal condition
    /// so the run fails closed with exit 1 and no output.
    AverageErrorRateOnEmptyRead,
}

/// The configured filters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filters {
    pub minimum_length: Option<usize>,
    pub maximum_length: Option<usize>,
    pub max_n: Option<f64>,
    pub max_expected_errors: Option<f64>,
    pub max_average_error_rate: Option<f64>,
    pub discard_trimmed: bool,
    pub discard_untrimmed: bool,
    pub discard_casava: bool,
    /// Quality offset used by the expected-error computation.
    pub quality_base: u8,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            minimum_length: None,
            maximum_length: None,
            max_n: None,
            max_expected_errors: None,
            max_average_error_rate: None,
            discard_trimmed: false,
            discard_untrimmed: false,
            discard_casava: false,
            quality_base: 33,
        }
    }
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        *self == Filters::default()
    }
}

/// Expected number of errors in a read, from its qualities.
///
/// cutadapt sums `10 ** (-Q/10)` over every base, which is the expected number of
/// mis-sequenced bases.
pub fn expected_errors(qualities: &[u8], quality_base: u8) -> f64 {
    qualities
        .iter()
        .map(|&q| {
            let phred = q as i32 - quality_base as i32;
            10f64.powf(-(phred as f64) / 10.0)
        })
        .sum()
}

/// Number of N bases in a sequence.
pub fn count_ns(sequence: &[u8]) -> usize {
    sequence
        .iter()
        .filter(|b| b.eq_ignore_ascii_case(&b'N'))
        .count()
}

/// The CASAVA filter bit: the second field of a CASAVA 1.8 header is
/// `Y:filtered` or `N:passed`. Returns `true` when the read is *filtered*
/// (`Y`), which only matters with `--discard-casava`.
pub fn casava_filtered(header: &[u8]) -> bool {
    // The header looks like "@name 1:N:0:INDEX" or "@name 1:Y:0:INDEX".
    let text = String::from_utf8_lossy(header);
    let mut fields = text.split_whitespace();
    if fields.next().is_none() {
        return false;
    }
    match fields.next() {
        Some(second) => {
            let mut parts = second.split(':');
            let _read_number = parts.next();
            matches!(parts.next(), Some("Y"))
        }
        None => false,
    }
}

/// Decide the fate of one read after trimming.
///
/// `retained` is the trimmed sequence (used for the length and N filters),
/// `qualities` its matching qualities, and `had_adapter` whether an adapter was
/// removed. The order below matches cutadapt's: length, then N, then expected
/// errors, then average error rate, then CASAVA, then the discard options.
pub fn classify(
    header: &[u8],
    retained: &[u8],
    qualities: &[u8],
    had_adapter: bool,
    filters: &Filters,
) -> Fate {
    if let Some(min) = filters.minimum_length {
        if retained.len() < min {
            return Fate::TooShort;
        }
    }
    // A read can also be too long for `-M`, or too short for its R2 counterpart;
    // the caller passes the effective bound for this read.
    if let Some(max) = filters.maximum_length {
        if retained.len() > max {
            return Fate::TooLong;
        }
    }
    if let Some(max_n) = filters.max_n {
        // cutadapt accepts both an absolute count and a fraction of the length.
        let count = count_ns(retained);
        let limit = if max_n < 1.0 {
            (max_n * retained.len() as f64) as usize
        } else {
            max_n as usize
        };
        if count > limit {
            return Fate::TooManyN;
        }
    }
    if let Some(max_ee) = filters.max_expected_errors {
        if expected_errors(qualities, filters.quality_base) > max_ee {
            return Fate::TooManyExpectedErrors;
        }
    }
    if let Some(max_aer) = filters.max_average_error_rate {
        if retained.is_empty() {
            // cutadapt 4.6 divides by zero here and aborts the run with a Python
            // traceback and exit 1. Fail closed instead: the same exit status
            // and no successful output, but with a readable diagnostic.
            return Fate::AverageErrorRateOnEmptyRead;
        }
        let rate = expected_errors(qualities, filters.quality_base) / retained.len() as f64;
        if rate > max_aer {
            return Fate::TooManyAverageErrors;
        }
    }
    if filters.discard_casava && casava_filtered(header) {
        return Fate::CasavaFiltered;
    }
    if filters.discard_trimmed && had_adapter {
        return Fate::DiscardTrimmed;
    }
    if filters.discard_untrimmed && !had_adapter {
        return Fate::DiscardUntrimmed;
    }
    Fate::Written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f() -> Filters {
        Filters::default()
    }

    #[test]
    fn length_filters() {
        let filters = Filters {
            minimum_length: Some(10),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"ACGT", b"IIII", false, &filters),
            Fate::TooShort
        );
        assert_eq!(
            classify(b"@r", b"ACGTACGTAC", b"IIIIIIIIII", false, &filters),
            Fate::Written
        );
        let filters = Filters {
            maximum_length: Some(4),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"ACGTA", b"IIIII", false, &filters),
            Fate::TooLong
        );
    }

    #[test]
    fn n_filter_counts_absolute_and_fractional_limits() {
        let filters = Filters {
            max_n: Some(2.0),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"ACGTNNACGT", b"IIIIIIIIII", false, &filters),
            Fate::Written
        );
        assert_eq!(
            classify(b"@r", b"ACGTNNNACGT", b"IIIIIIIIIII", false, &filters),
            Fate::TooManyN
        );
        // Fractional: 0.1 of 10 bases allows 1 N.
        let filters = Filters {
            max_n: Some(0.1),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"ACGTNNACGT", b"IIIIIIIIII", false, &filters),
            Fate::TooManyN
        );
    }

    #[test]
    fn expected_errors_uses_phred() {
        // 'I' is Phred 40: 1e-4 per base, 10 bases -> 1e-3.
        let ee = expected_errors(b"IIIIIIIIII", 33);
        assert!((ee - 1e-3).abs() < 1e-9, "{ee}");
        // '#' is Phred 2: ~0.63 per base.
        let ee = expected_errors(b"##", 33);
        assert!((ee - 2.0 * 10f64.powf(-0.2)).abs() < 1e-9, "{ee}");
    }

    #[test]
    fn expected_error_and_average_rate_filters() {
        let filters = Filters {
            max_expected_errors: Some(0.5),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"IIII", b"IIII", false, &filters),
            Fate::Written
        );
        // '#' is Phred 2 -> 0.630957 expected errors, already above the 0.5
        // limit (verified with cutadapt's own expected_errors()).
        assert_eq!(
            classify(b"@r", b"A", b"#", false, &filters),
            Fate::TooManyExpectedErrors
        );
        assert_eq!(
            classify(b"@r", b"AAAA", b"####", false, &filters),
            Fate::TooManyExpectedErrors
        );
        let filters = Filters {
            max_average_error_rate: Some(0.01),
            ..f()
        };
        // 4 bases at Phred 2 -> ~2.5 expected errors, rate 0.63 > 0.01
        assert_eq!(
            classify(b"@r", b"####", b"####", false, &filters),
            Fate::TooManyAverageErrors
        );
    }

    #[test]
    fn empty_read_with_average_rate_filter_is_fatal() {
        // cutadapt 4.6 raises ZeroDivisionError and exits 1. We model the same
        // outcome as a fatal fate so the run fails closed with exit 1.
        let filters = Filters {
            max_average_error_rate: Some(0.1),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"", b"", false, &filters),
            Fate::AverageErrorRateOnEmptyRead
        );
    }

    #[test]
    fn casava_filter_reads_the_second_field() {
        assert!(casava_filtered(b"@r 1:Y:0:ACGT"));
        assert!(!casava_filtered(b"@r 1:N:0:ACGT"));
        assert!(!casava_filtered(b"@plainname"));
        let filters = Filters {
            discard_casava: true,
            ..f()
        };
        assert_eq!(
            classify(b"@r 1:Y:0:ACGT", b"ACGT", b"IIII", false, &filters),
            Fate::CasavaFiltered
        );
        assert_eq!(
            classify(b"@r 1:N:0:ACGT", b"ACGT", b"IIII", false, &filters),
            Fate::Written
        );
    }

    #[test]
    fn discard_trimmed_and_untrimmed() {
        let trimmed = Filters {
            discard_trimmed: true,
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"ACGT", b"IIII", true, &trimmed),
            Fate::DiscardTrimmed
        );
        assert_eq!(
            classify(b"@r", b"ACGT", b"IIII", false, &trimmed),
            Fate::Written
        );
        let untrimmed = Filters {
            discard_untrimmed: true,
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"ACGT", b"IIII", false, &untrimmed),
            Fate::DiscardUntrimmed
        );
        assert_eq!(
            classify(b"@r", b"ACGT", b"IIII", true, &untrimmed),
            Fate::Written
        );
    }

    #[test]
    fn first_failing_filter_wins() {
        // Too short AND too many Ns -> reported as too short (cutadapt's order).
        let filters = Filters {
            minimum_length: Some(10),
            max_n: Some(0.0),
            ..f()
        };
        assert_eq!(
            classify(b"@r", b"NNN", b"III", false, &filters),
            Fate::TooShort
        );
    }

    #[test]
    fn defaults_are_empty() {
        assert!(f().is_empty());
        assert!(!Filters {
            minimum_length: Some(1),
            ..f()
        }
        .is_empty());
    }
}
