//! `lint`: per-record and per-pair FASTQ validation.
//!
//! # Source of the rules
//!
//! The validator codes, names, levels and checks are taken verbatim from the
//! published `fq lint` documentation (`https://github.com/stjude-rust-labs/fq`,
//! README "Validators" table, retrieved 2026-10-20):
//!
//! | Code | Level | Name | Validation |
//! | --- | --- | --- | --- |
//! | S001 | low | PlusLine | Plus line starts with a `+`. |
//! | S002 | medium | Alphabet | All characters in sequence line are one of `ACGTN`, case-insensitive. |
//! | S003 | high | Name | Name line starts with an `@`. |
//! | S004 | low | Complete | All four record lines are present. |
//! | S005 | high | ConsistentSeqQual | Sequence and quality lengths are the same. |
//! | S006 | medium | QualityString | All characters in quality line are between `!` and `~`. |
//! | S007 | high | DuplicateName | All record names are unique. |
//! | P001 | medium | Names | Each paired read name is the same, excluding interleave. |
//!
//! # Status
//!
//! The `fq` binary is **not available in this environment**, so these rules are
//! documentation-derived and are recorded as `experimental` in
//! `docs/EVIDENCE-W1-3.md` (DR-1). Two details the documentation does not pin
//! down are recorded as open questions there (DR-5): whether S002's alphabet is
//! `ACGTN` only or the full IUPAC set, and how S007 compares names that carry a
//! read-pair suffix.
//!
//! # Exit contract
//!
//! `fq lint` is documented to exit cleanly (0) when no validation errors occur.
//! This implementation exits 3 (input precondition violated) when a record fails
//! validation, matching the workspace's shared exit-code contract rather than
//! inventing a new code.

/// The level a validator is gated behind (`--single-read-validation-level`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Runs at every validation level.
    Low,
    /// Runs at `medium` and `high`.
    Medium,
    /// Runs only at `high`.
    High,
}

impl Level {
    /// Parse the documented `--*-validation-level` values.
    pub fn parse(text: &str) -> Option<Level> {
        match text.to_ascii_lowercase().as_str() {
            "low" => Some(Level::Low),
            "medium" => Some(Level::Medium),
            "high" => Some(Level::High),
            _ => None,
        }
    }

    /// The label used in diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
        }
    }

    /// True when a validator at `self` should run given a ceiling of `ceiling`.
    pub fn runs_at(self, ceiling: Level) -> bool {
        self <= ceiling
    }
}

/// One validator, with its published identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Validator {
    /// S001 — plus line starts with `+`.
    PlusLine,
    /// S002 — sequence alphabet is `ACGTN`, case-insensitive.
    Alphabet,
    /// S003 — name line starts with `@`.
    Name,
    /// S004 — all four lines present.
    Complete,
    /// S005 — sequence and quality lengths agree.
    ConsistentSeqQual,
    /// S006 — quality characters are in `!`..=`~`.
    QualityString,
    /// S007 — record names are unique.
    DuplicateName,
    /// P001 — paired read names match.
    PairedNames,
}

impl Validator {
    /// The published code (`S001` … `P001`).
    pub fn code(self) -> &'static str {
        match self {
            Validator::PlusLine => "S001",
            Validator::Alphabet => "S002",
            Validator::Name => "S003",
            Validator::Complete => "S004",
            Validator::ConsistentSeqQual => "S005",
            Validator::QualityString => "S006",
            Validator::DuplicateName => "S007",
            Validator::PairedNames => "P001",
        }
    }

    /// The published name.
    pub fn name(self) -> &'static str {
        match self {
            Validator::PlusLine => "PlusLine",
            Validator::Alphabet => "Alphabet",
            Validator::Name => "Name",
            Validator::Complete => "Complete",
            Validator::ConsistentSeqQual => "ConsistentSeqQual",
            Validator::QualityString => "QualityString",
            Validator::DuplicateName => "DuplicateName",
            Validator::PairedNames => "Names",
        }
    }

    /// The published level.
    pub fn level(self) -> Level {
        match self {
            Validator::PlusLine => Level::Low,
            Validator::Complete => Level::Low,
            Validator::Alphabet => Level::Medium,
            Validator::QualityString => Level::Medium,
            Validator::PairedNames => Level::Medium,
            Validator::Name => Level::High,
            Validator::ConsistentSeqQual => Level::High,
            Validator::DuplicateName => Level::High,
        }
    }

    /// True for the paired-record validators.
    pub fn is_paired(self) -> bool {
        matches!(self, Validator::PairedNames)
    }

    /// Every single-record validator that fq 0.12.0 actually enables, in order.
    ///
    /// `S007` (DuplicateName) is deliberately **excluded**. fq 0.12.0 knows the
    /// code -- `fq lint --disable-validator S007` is accepted -- but never puts it
    /// in the enabled set at any `--single-read-validation-level`, and a file with
    /// duplicate names exits 0:
    ///
    /// ```text
    ///   $ fq lint --lint-mode log dup.fq ; echo $?
    ///   0                       # two identical @d records, no S007 error
    ///   $ fq lint --single-read-validation-level high --lint-mode log ok.fq
    ///   ...enabled single read validators: ["[S003] NameValidator",
    ///        "[S004] CompleteValidator", "[S002] AlphabetValidator",
    ///        "[S001] PlusLineValidator", "[S005] ConsistentSeqQualValidator",
    ///        "[S006] QualityStringValidator"]        # no S007
    /// ```
    ///
    /// S008 and S009 are likewise known-but-never-enabled. Listing S007 here made
    /// `lint_S007` fail with exit 3 where the oracle exits 0. The implementation
    /// is kept (and still accepts `--disable-validator S007`) so the code round
    /// trips, but it does not run by default.
    pub const SINGLE: [Validator; 6] = [
        Validator::PlusLine,
        Validator::Alphabet,
        Validator::Name,
        Validator::Complete,
        Validator::ConsistentSeqQual,
        Validator::QualityString,
    ];

    /// Every paired-record validator.
    pub const PAIRED: [Validator; 1] = [Validator::PairedNames];

    /// Look a validator up by its published code.
    pub fn from_code(code: &str) -> Option<Validator> {
        // S007/S008/S009 are recognised so a `--disable-validator` naming them is
        // not a usage error, even though they are not in `SINGLE`.
        if code.eq_ignore_ascii_case("S007") {
            return Some(Validator::DuplicateName);
        }
        Self::SINGLE
            .into_iter()
            .chain(Self::PAIRED)
            .find(|v| v.code().eq_ignore_ascii_case(code))
    }

    /// Whether the validator is in effect, given the disabled set and a level
    /// ceiling.
    pub fn is_enabled(self, disabled: &[Validator], ceiling: Level) -> bool {
        !disabled.contains(&self) && self.level().runs_at(ceiling)
    }
}

/// The record fields `lint` inspects.
///
/// Borrowed so validation does not copy sequence or quality data. Name retention
/// for paired validation belongs to the command, not this per-record validator.
#[derive(Clone, Copy, Debug)]
pub struct LintRecord<'a> {
    /// The header line *without* its marker, as the parser returns it.
    pub header: &'a [u8],
    /// The full header line including the marker, so S003 can check it.
    pub raw_header: &'a [u8],
    /// The sequence line.
    pub sequence: &'a [u8],
    /// The plus line including its `+`.
    pub plus: &'a [u8],
    /// The quality line, or `None` when the record was truncated.
    pub qualities: Option<&'a [u8]>,
}

/// One validation failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintError {
    /// The validator that fired.
    pub validator: Validator,
    /// 1-based record number in the input.
    pub record: u64,
    /// A human explanation, including the offending value.
    pub detail: String,
    /// 1-based line number inside the input file, when known.
    pub line: Option<u64>,
    /// 1-based column number on that line, when known.
    pub column: Option<u64>,
    /// fq 0.12.0's own wording for this failure, which is what a byte-level
    /// comparison of the log line needs. `None` when the failure has no
    /// counterpart in fq (for example S007, which fq never enables).
    pub oracle_message: Option<String>,
    /// True when fq attributes this failure to read 1's file rather than read 2's.
    /// A pair-name mismatch is reported against read 1.
    pub report_against_read1: bool,
}

impl LintError {
    /// Render as `<file>:<line>:<col>: [S005] ConsistentSeqQualValidator: ...`,
    /// which is fq 0.12.0's own shape for a single-read error:
    ///
    /// ```text
    ///   r1.fq:2:5: [S002] AlphabetValidator: invalid sequence character: 'Z'
    /// ```
    ///
    /// The file name is supplied by the caller, because a `LintError` does not
    /// know which input it came from. When a position is unknown the record
    /// number is used instead of a line, keeping the message useful.
    pub fn render_at(&self, file: &str) -> String {
        let line = self.line.unwrap_or(self.record);
        let column = self.column.unwrap_or(1);
        let message = self
            .oracle_message
            .clone()
            .unwrap_or_else(|| self.detail.clone());
        format!(
            "{file}:{line}:{column}: [{}] {}Validator: {message}",
            self.validator.code(),
            self.validator.name()
        )
    }

    /// Render as `[S005] ConsistentSeqQual (record 3): ...`.
    ///
    /// The position-free form, kept for callers that have no file name.
    pub fn render(&self) -> String {
        format!(
            "[{}] {} (record {}): {}",
            self.validator.code(),
            self.validator.name(),
            self.record,
            self.detail
        )
    }
}

/// A streaming validator.
///
/// This per-record validator retains only its configuration and record count.
/// The command manages the names required by the paired validation passes.
pub struct Linter {
    disabled: Vec<Validator>,
    single_ceiling: Level,
    paired_ceiling: Level,
    enabled_single: u16,
    record: u64,
}

impl Linter {
    /// A linter with the documented defaults (`high` for both levels, nothing
    /// disabled).
    pub fn new() -> Linter {
        let mut linter = Linter {
            disabled: Vec::new(),
            single_ceiling: Level::High,
            paired_ceiling: Level::High,
            enabled_single: 0,
            record: 0,
        };
        linter.refresh_single();
        linter
    }

    fn refresh_single(&mut self) {
        self.enabled_single = Validator::SINGLE
            .into_iter()
            .filter(|v| v.is_enabled(&self.disabled, self.single_ceiling))
            .fold(0, |mask, v| mask | (1 << v as u16));
    }

    /// Disable validators by code, as `--disable-validator` does.
    ///
    /// Returns the codes that did not match a known validator, so the CLI can
    /// reject them instead of silently ignoring a typo.
    pub fn disable(&mut self, codes: &[String]) -> Vec<String> {
        let mut unknown = Vec::new();
        for code in codes {
            match Validator::from_code(code) {
                Some(validator) => {
                    if !self.disabled.contains(&validator) {
                        self.disabled.push(validator);
                    }
                }
                None => unknown.push(code.clone()),
            }
        }
        self.refresh_single();
        unknown
    }

    /// Set the single-read validation level ceiling.
    pub fn with_single_level(mut self, level: Level) -> Linter {
        self.single_ceiling = level;
        self.refresh_single();
        self
    }

    /// Set the paired-read validation level ceiling.
    pub fn with_paired_level(mut self, level: Level) -> Linter {
        self.paired_ceiling = level;
        self
    }

    /// The validators that are in effect for single records.
    pub fn active_single(&self) -> Vec<Validator> {
        Validator::SINGLE
            .into_iter()
            .filter(|v| v.is_enabled(&self.disabled, self.single_ceiling))
            .collect()
    }

    /// The validators that are in effect for pairs.
    pub fn active_paired(&self) -> Vec<Validator> {
        Validator::PAIRED
            .into_iter()
            .filter(|v| v.is_enabled(&self.disabled, self.paired_ceiling))
            .collect()
    }

    /// Validate one record, returning every failure (not just the first).
    ///
    /// Returning all failures is what `--lint-mode log` needs; `panic` mode simply
    /// stops at the first one, which the caller does by inspecting the vector.
    pub fn check_record(&mut self, record: &LintRecord<'_>) -> Vec<LintError> {
        self.record += 1;
        let index = self.record;
        let mut errors = Vec::new();
        let enabled = |v: Validator| self.enabled_single & (1 << v as u16) != 0;

        if enabled(Validator::Complete) && record.qualities.is_none() {
            errors.push(LintError {
                validator: Validator::Complete,
                record: index,
                detail: "the record has fewer than four lines".to_string(),
                line: Some(index * 4 - 3),
                column: Some(1),
                oracle_message: Some("empty quality\nexpected 4, got 0".to_string()),
                report_against_read1: false,
            });
        }
        if enabled(Validator::Name) && !record.raw_header.starts_with(b"@") {
            errors.push(LintError {
                validator: Validator::Name,
                record: index,
                detail: format!(
                    "name line does not start with '@': {:?}",
                    String::from_utf8_lossy(&record.raw_header[..record.raw_header.len().min(40)])
                ),
                line: Some(index * 4 - 3),
                column: Some(1),
                oracle_message: Some("missing @ prefix".to_string()),
                report_against_read1: false,
            });
        }
        if enabled(Validator::PlusLine) && !record.plus.starts_with(b"+") {
            errors.push(LintError {
                validator: Validator::PlusLine,
                record: index,
                detail: format!(
                    "plus line does not start with '+': {:?}",
                    String::from_utf8_lossy(&record.plus[..record.plus.len().min(40)])
                ),
                line: Some(index * 4 - 1),
                column: Some(1),
                oracle_message: Some("missing + prefix".to_string()),
                report_against_read1: false,
            });
        }
        if enabled(Validator::Alphabet) {
            if let Some((offset, bad)) = first_invalid(record.sequence, is_documented_alphabet_byte)
            {
                errors.push(LintError {
                    validator: Validator::Alphabet,
                    record: index,
                    detail: format!(
                        "sequence contains {:?}, which is not one of A, C, G, T, N",
                        *bad as char
                    ),
                    // The sequence is line 2 of the record; the column is 1-based.
                    line: Some(index * 4 - 2),
                    column: Some(offset as u64 + 1),
                    oracle_message: Some(format!("invalid sequence character: {:?}", *bad as char)),
                    report_against_read1: false,
                });
            }
        }
        if enabled(Validator::QualityString) {
            if let Some(qualities) = record.qualities {
                if let Some((offset, bad)) =
                    first_invalid(qualities, |byte| (b'!'..=b'~').contains(&byte))
                {
                    errors.push(LintError {
                        validator: Validator::QualityString,
                        record: index,
                        detail: format!("quality contains {:#04x}, which is outside '!'..'~'", bad),
                        line: Some(index * 4),
                        column: Some(offset as u64 + 1),
                        oracle_message: Some(format!(
                            "invalid quality score character: {:?}",
                            *bad as char
                        )),
                        report_against_read1: false,
                    });
                }
            }
        }
        if enabled(Validator::ConsistentSeqQual) {
            if let Some(qualities) = record.qualities {
                if record.sequence.len() != qualities.len() {
                    errors.push(LintError {
                        validator: Validator::ConsistentSeqQual,
                        record: index,
                        detail: format!(
                            "sequence length {} != quality length {}",
                            record.sequence.len(),
                            qualities.len()
                        ),
                        line: Some(index * 4 - 2),
                        column: Some(1),
                        oracle_message: Some(format!(
                            "sequence-quality scores lengths mismatch: expected {}, got {}",
                            record.sequence.len(),
                            qualities.len()
                        )),
                        report_against_read1: false,
                    });
                }
            }
        }
        // S007 (DuplicateName) is deliberately not checked here.
        //
        // In fq 0.12.0 it is a *special* validator: it is announced on its own
        // line, "enabled special validators: [\"[S007] DuplicateNameValidator\"]",
        // and it only runs for a paired input. It is absent from the single-read
        // validator list at every level, and a single file with duplicate names
        // exits 0. Checking it here made `lint_S007` fail where the oracle passes.
        //
        // The paired command owns its name state. Retaining another set here
        // would consume memory without contributing to a validation decision.

        errors
    }

    /// Validate a pair of records with the same index (P001).
    ///
    /// `-1`/`-2` suffixes and the `1:`/`2:` CASAVA prefix are excluded, which is
    /// what "excluding interleave" in the published description implies; DR-5
    /// records that the exact comparison is not documented.
    pub fn check_pair(&self, index: u64, first: &[u8], second: &[u8]) -> Vec<LintError> {
        if !Validator::PairedNames.is_enabled(&self.disabled, self.paired_ceiling) {
            return Vec::new();
        }
        if pair_name(first) == pair_name(second) {
            Vec::new()
        } else {
            vec![LintError {
                validator: Validator::PairedNames,
                record: index,
                detail: format!(
                    "read 1 name {:?} does not match read 2 name {:?}",
                    String::from_utf8_lossy(first),
                    String::from_utf8_lossy(second)
                ),
                line: Some(index * 4 - 3),
                column: Some(1),
                // fq words a pair-name mismatch from read 2's point of view.
                // fq reports a pair mismatch against read 1's line and words it
                // from read 2's point of view, quoting the raw headers:
                //   r1.fq:5:1: [P001] NamesValidator: names mismatch: expected '@other', got '@p2'
                oracle_message: Some(format!(
                    "names mismatch: expected '{}', got '{}'",
                    String::from_utf8_lossy(second),
                    String::from_utf8_lossy(first)
                )),
                report_against_read1: true,
            }]
        }
    }

    /// Number of records validated so far.
    pub fn records(&self) -> u64 {
        self.record
    }
}

impl Default for Linter {
    fn default() -> Self {
        Linter::new()
    }
}

// Reduce validity over the full slice so the ordinary all-valid case can be
// vectorized. Only malformed records need the second scan to locate the exact
// first bad byte for the existing diagnostic.
fn first_invalid(bytes: &[u8], valid: impl Fn(u8) -> bool) -> Option<(usize, &u8)> {
    let invalid = bytes
        .iter()
        .fold(false, |invalid, &byte| invalid | !valid(byte));
    if invalid {
        bytes.iter().enumerate().find(|(_, byte)| !valid(**byte))
    } else {
        None
    }
}

/// The documented S002 alphabet: `ACGTN`, case-insensitive.
/// Other IUPAC letters fail S002; DR-5 records the upstream boundary.
fn is_documented_alphabet_byte(byte: u8) -> bool {
    matches!(byte.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T' | b'N')
}

/// Strip the description after a literal space and a trailing `/1` or `/2`.
pub fn normalise_pair_name(name: &[u8]) -> Vec<u8> {
    pair_name(name).to_vec()
}

fn pair_name(name: &[u8]) -> &[u8] {
    let mut value = name;
    // CASAVAv1.8 writes `@instrument:... 1:N:0:INDEX`; the part before the first
    // space is shared by both mates.
    if let Some(space) = value.iter().position(|&b| b == b' ') {
        value = &value[..space];
    }
    // `name/1` and `name/2`.
    if value.len() > 2
        && value[value.len() - 2] == b'/'
        && matches!(value[value.len() - 1], b'1' | b'2')
    {
        return &value[..value.len() - 2];
    }
    // Underscores remain significant.
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_validation_keeps_the_first_invalid_byte() {
        for len in [0, 1, 31, 32, 33, 150, 4097] {
            let mut bytes = vec![b'A'; len];
            assert_eq!(first_invalid(&bytes, is_documented_alphabet_byte), None);
            if len == 0 {
                continue;
            }
            for offset in [0, len / 2, len - 1] {
                for byte in 0..=255 {
                    bytes[offset] = byte;
                    assert_eq!(
                        first_invalid(&bytes, is_documented_alphabet_byte),
                        bytes
                            .iter()
                            .enumerate()
                            .find(|(_, b)| !is_documented_alphabet_byte(**b))
                    );
                    assert_eq!(
                        first_invalid(&bytes, |b| (b'!'..=b'~').contains(&b)),
                        bytes
                            .iter()
                            .enumerate()
                            .find(|(_, b)| !(b'!'..=b'~').contains(*b))
                    );
                }
                bytes[offset] = b'A';
            }
        }
    }

    fn record<'a>(
        header: &'a [u8],
        sequence: &'a [u8],
        plus: &'a [u8],
        qualities: Option<&'a [u8]>,
    ) -> LintRecord<'a> {
        let raws: &'a [u8] = header;
        LintRecord {
            header,
            raw_header: raws,
            sequence,
            plus,
            qualities,
        }
    }

    fn valid<'a>(header: &'a [u8], sequence: &'a [u8], qualities: &'a [u8]) -> LintRecord<'a> {
        LintRecord {
            header,
            raw_header: header,
            sequence,
            plus: b"+",
            qualities: Some(qualities),
        }
    }

    #[test]
    fn validator_identity_matches_the_published_table() {
        assert_eq!(Validator::PlusLine.code(), "S001");
        assert_eq!(Validator::PlusLine.level(), Level::Low);
        assert_eq!(Validator::PlusLine.name(), "PlusLine");

        assert_eq!(Validator::Alphabet.code(), "S002");
        assert_eq!(Validator::Alphabet.level(), Level::Medium);

        assert_eq!(Validator::Name.code(), "S003");
        assert_eq!(Validator::Name.level(), Level::High);

        assert_eq!(Validator::Complete.code(), "S004");
        assert_eq!(Validator::Complete.level(), Level::Low);

        assert_eq!(Validator::ConsistentSeqQual.code(), "S005");
        assert_eq!(Validator::ConsistentSeqQual.level(), Level::High);

        assert_eq!(Validator::QualityString.code(), "S006");
        assert_eq!(Validator::QualityString.level(), Level::Medium);

        assert_eq!(Validator::DuplicateName.code(), "S007");
        assert_eq!(Validator::DuplicateName.level(), Level::High);

        assert_eq!(Validator::PairedNames.code(), "P001");
        assert_eq!(Validator::PairedNames.level(), Level::Medium);
        assert_eq!(Validator::PairedNames.name(), "Names");
    }

    #[test]
    fn every_code_round_trips() {
        for validator in Validator::SINGLE.into_iter().chain(Validator::PAIRED) {
            assert_eq!(Validator::from_code(validator.code()), Some(validator));
            // Case-insensitive, so `--disable-validator s005` works.
            assert_eq!(
                Validator::from_code(&validator.code().to_lowercase()),
                Some(validator)
            );
        }
        assert_eq!(Validator::from_code("S999"), None);
        assert_eq!(Validator::from_code(""), None);
    }

    #[test]
    fn a_valid_record_passes_every_validator() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&valid(b"@r1", b"ACGT", b"IIII"));
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn s003_rejects_a_header_without_at() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&valid(b"r1", b"ACGT", b"IIII"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::Name);
        assert!(errors[0].render().contains("[S003] Name"));
    }

    #[test]
    fn s001_rejects_a_plus_line_without_plus() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&record(b"@r1", b"ACGT", b"-", Some(b"IIII")));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::PlusLine);
    }

    #[test]
    fn s002_rejects_a_base_outside_acgtn() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&valid(b"@r1", b"ACGTZ", b"IIIII"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::Alphabet);
        assert!(errors[0].detail.contains('Z'));
    }

    #[test]
    fn s002_accepts_lower_case_and_n() {
        let mut linter = Linter::new();
        assert!(linter
            .check_record(&valid(b"@r1", b"acgtn", b"IIIII"))
            .is_empty());
        assert!(linter
            .check_record(&valid(b"@r2", b"ACGTN", b"IIIII"))
            .is_empty());
    }

    // DR-5: the documentation says "one of ACGTN", so an IUPAC ambiguity code
    // outside N fails. This pins the faithful-to-the-text choice.
    #[test]
    fn s002_rejects_iupac_codes_beyond_n() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&valid(b"@r1", b"ACGTR", b"IIIII"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::Alphabet);
    }

    #[test]
    fn s004_rejects_a_truncated_record() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&record(b"@r1", b"ACGT", b"+", None));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::Complete);
    }

    #[test]
    fn s005_rejects_a_length_mismatch() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&valid(b"@r1", b"ACGTACGT", b"III"));
        let s005: Vec<&LintError> = errors
            .iter()
            .filter(|e| e.validator == Validator::ConsistentSeqQual)
            .collect();
        assert_eq!(s005.len(), 1, "{errors:?}");
        assert!(
            s005[0].detail.contains("sequence length 8")
                && s005[0].detail.contains("quality length 3"),
            "{}",
            s005[0].detail
        );
    }

    #[test]
    fn s006_rejects_a_quality_outside_the_printable_range() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&valid(b"@r1", b"ACGT", b"II\xffI"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::QualityString);
    }

    #[test]
    fn s006_accepts_the_documented_boundaries() {
        let mut linter = Linter::new();
        // '!' is 0x21 and '~' is 0x7e, both inclusive.
        assert!(linter.check_record(&valid(b"@r1", b"AC", b"!~")).is_empty());
    }

    #[test]
    fn s007_does_not_run_by_default() {
        // fq 0.12.0 never enables S007 as a single-read validator: two identical
        // names exit 0. The code is still known (so --disable-validator S007 is
        // accepted) but nothing fires.
        let mut linter = Linter::new();
        assert!(linter
            .check_record(&valid(b"@r1", b"ACGT", b"IIII"))
            .is_empty());
        let errors = linter.check_record(&valid(b"@r1", b"ACGT", b"IIII"));
        assert!(
            errors.is_empty(),
            "S007 must not fire by default: {errors:?}"
        );
        // The code still round-trips.
        assert_eq!(Validator::from_code("S007"), Some(Validator::DuplicateName));
    }

    #[test]
    fn s007_compares_full_headers_not_just_ids() {
        // Two reads can share an ID and differ in the comment; the published
        // validator is called "DuplicateName", so the whole header is compared.
        let mut linter = Linter::new();
        assert!(linter
            .check_record(&valid(b"@r1 a", b"ACGT", b"IIII"))
            .is_empty());
        assert!(linter
            .check_record(&valid(b"@r1 b", b"ACGT", b"IIII"))
            .is_empty());
    }

    #[test]
    fn several_failures_are_all_reported() {
        let mut linter = Linter::new();
        let errors = linter.check_record(&record(b"r1", b"ACGTZ", b"-", Some(b"II")));
        let codes: Vec<&str> = errors.iter().map(|e| e.validator.code()).collect();
        assert!(codes.contains(&"S003"), "missing S003: {codes:?}");
        assert!(codes.contains(&"S001"), "missing S001: {codes:?}");
        assert!(codes.contains(&"S002"), "missing S002: {codes:?}");
        assert!(codes.contains(&"S005"), "missing S005: {codes:?}");
    }

    #[test]
    fn disabling_a_validator_suppresses_only_that_one() {
        let mut linter = Linter::new();
        let unknown = linter.disable(&["S005".to_string()]);
        assert!(unknown.is_empty());
        // A mismatch now passes S005 but everything else is still checked.
        let errors = linter.check_record(&valid(b"@r1", b"ACGTACGT", b"III"));
        assert!(errors.is_empty(), "{errors:?}");
        // An unrelated failure still fires.
        let errors = linter.check_record(&valid(b"@r2", b"ACGTZ", b"IIIII"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::Alphabet);
    }

    #[test]
    fn an_unknown_validator_code_is_reported_not_ignored() {
        let mut linter = Linter::new();
        let unknown = linter.disable(&["S999".to_string(), "s005".to_string()]);
        assert_eq!(unknown, vec!["S999".to_string()]);
        // The valid one was still applied.
        assert!(!linter
            .active_single()
            .contains(&Validator::ConsistentSeqQual));
    }

    #[test]
    fn levels_gate_the_validators() {
        let linter = Linter::new().with_single_level(Level::Low);
        let active = linter.active_single();
        assert_eq!(active, vec![Validator::PlusLine, Validator::Complete]);

        let linter = Linter::new().with_single_level(Level::Medium);
        let active = linter.active_single();
        assert!(active.contains(&Validator::Alphabet));
        assert!(active.contains(&Validator::QualityString));
        assert!(!active.contains(&Validator::Name));
        assert!(!active.contains(&Validator::ConsistentSeqQual));
        assert!(!active.contains(&Validator::DuplicateName));

        let linter = Linter::new();
        // Six, not seven: S007 is not a single-read validator in fq 0.12.0.
        assert_eq!(linter.active_single().len(), 6);
        assert!(!linter.active_single().contains(&Validator::DuplicateName));
    }

    #[test]
    fn level_parse_and_ordering() {
        assert_eq!(Level::parse("low"), Some(Level::Low));
        assert_eq!(Level::parse("MEDIUM"), Some(Level::Medium));
        assert_eq!(Level::parse("high"), Some(Level::High));
        assert_eq!(Level::parse("bogus"), None);
        assert!(Level::Low < Level::Medium);
        assert!(Level::Medium < Level::High);
        assert!(Level::Low.runs_at(Level::High));
        assert!(!Level::High.runs_at(Level::Low));
        assert_eq!(Level::Medium.label(), "medium");
    }

    #[test]
    fn p001_accepts_matching_pair_names() {
        let linter = Linter::new();
        assert!(linter.check_pair(1, b"r1", b"r1").is_empty());
    }

    #[test]
    fn p001_accepts_slash_suffixes() {
        let linter = Linter::new();
        assert!(linter.check_pair(1, b"r1/1", b"r1/2").is_empty());
        assert!(linter.check_pair(1, b"r1", b"r1/2").is_empty());
    }

    #[test]
    fn p001_accepts_casava_pair_suffixes() {
        let linter = Linter::new();
        let first = b"@inst:1:2:3 1:N:0:ACGT";
        let second = b"@inst:1:2:3 2:N:0:ACGT";
        assert!(linter.check_pair(1, first, second).is_empty());
    }

    #[test]
    fn p001_rejects_genuinely_different_names() {
        let linter = Linter::new();
        let errors = linter.check_pair(4, b"r1", b"r2");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].validator, Validator::PairedNames);
        assert_eq!(errors[0].record, 4);
    }

    // An underscore is a normal name character, so `r1_1` and `r1_2` are NOT the
    // same pair (DR-5 records this choice).
    #[test]
    fn p001_does_not_strip_underscore_suffixes() {
        let linter = Linter::new();
        assert_eq!(linter.check_pair(1, b"r1_1", b"r1_2").len(), 1);
    }

    #[test]
    fn p001_can_be_disabled() {
        let mut linter = Linter::new();
        linter.disable(&["P001".to_string()]);
        assert!(linter.check_pair(1, b"r1", b"r2").is_empty());
        assert!(linter.active_paired().is_empty());
    }

    #[test]
    fn records_are_numbered_from_one() {
        let mut linter = Linter::new();
        assert_eq!(linter.records(), 0);
        linter.check_record(&valid(b"@r1", b"ACGT", b"IIII"));
        assert_eq!(linter.records(), 1);
        let errors = linter.check_record(&valid(b"r2", b"ACGT", b"IIII"));
        assert_eq!(errors[0].record, 2);
    }

    #[test]
    fn error_rendering_includes_code_name_and_record() {
        let mut linter = Linter::new();
        // 'I' is a valid quality byte, so only S005 fires for this record.
        let errors = linter.check_record(&valid(b"@r1", b"ACGTACGT", b"III"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            errors[0].render(),
            "[S005] ConsistentSeqQual (record 1): sequence length 8 != quality length 3"
        );
    }

    #[test]
    fn normalise_pair_name_handles_every_documented_shape() {
        assert_eq!(normalise_pair_name(b"r1"), b"r1");
        assert_eq!(normalise_pair_name(b"r1/1"), b"r1");
        assert_eq!(normalise_pair_name(b"r1/2"), b"r1");
        assert_eq!(normalise_pair_name(b"r1 comment"), b"r1");
        assert_eq!(normalise_pair_name(b"r1_1"), b"r1_1");
        // A lone trailing slash is not a pair marker.
        assert_eq!(normalise_pair_name(b"r1/"), b"r1/");
    }
}
