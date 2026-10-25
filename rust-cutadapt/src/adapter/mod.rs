//! cutadapt 4.6 adapter model: types, anchors, and best-match selection.
//!
//! cutadapt accepts adapters through three options and three notations:
//!
//! | notation | type | trimming behaviour |
//! | --- | --- | --- |
//! | `-a ADAPTER` | regular 3' | removes the adapter and everything after it |
//! | `-a ADAPTER$` | anchored 3' | only matches as a suffix of the read |
//! | `-g ADAPTER` | regular 5' | removes the adapter and everything before it |
//! | `-g ^ADAPTER` | anchored 5' | only matches as a prefix of the read |
//! | `-b ADAPTER` | anywhere | 5' if the match starts at read position 0, else 3' |
//! | `-a ^A...B` / `-g A...B` | linked | removes the 5' part and the 3' part together |
//!
//! When several adapters match one read, cutadapt does **not** simply take the
//! first one listed. Each candidate match contributes a score and the best one
//! wins; ties go to the adapters that were specified first. The exact rule,
//! established against the oracle, is:
//!
//! * a match that leaves the read shorter (more of the read removed) is better,
//! * among equal cut positions the adapter with more matching bases wins,
//! * adapters are then considered in declaration order.
//!
//! This module is deliberately free of I/O and of the trimming pipeline so it can
//! be unit-tested directly against the oracle's decisions.

pub mod matcher;

use std::fmt;

/// Which end of the read an adapter is anchored to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterType {
    /// `-a`: adapter ligated to the 3' end. The adapter and the sequence after
    /// it are removed.
    RegularThreePrime,
    /// `-a ADAPTER$`: like `RegularThreePrime`, but only matches as a suffix.
    AnchoredThreePrime,
    /// `-g`: adapter ligated to the 5' end. The adapter and the sequence before
    /// it are removed.
    RegularFivePrime,
    /// `-g ^ADAPTER`: like `RegularFivePrime`, but only matches as a prefix.
    AnchoredFivePrime,
    /// `-b`: may be ligated to either end; resolved per match.
    Anywhere,
    /// `^A...B`: a 5' part and a 3' part removed in one step.
    Linked,
}

impl AdapterType {
    /// True if this adapter type can trim the read's 5' end.
    pub fn trims_five_prime(&self) -> bool {
        matches!(
            self,
            AdapterType::RegularFivePrime
                | AdapterType::AnchoredFivePrime
                | AdapterType::Anywhere
                | AdapterType::Linked
        )
    }

    /// True if this adapter type can trim the read's 3' end.
    pub fn trims_three_prime(&self) -> bool {
        matches!(
            self,
            AdapterType::RegularThreePrime
                | AdapterType::AnchoredThreePrime
                | AdapterType::Anywhere
                | AdapterType::Linked
        )
    }

    /// Human-readable name used in the report (`Type: regular 3'`, ...).
    pub fn report_name(&self) -> &'static str {
        match self {
            AdapterType::RegularThreePrime => "regular 3'",
            AdapterType::AnchoredThreePrime => "anchored 3'",
            AdapterType::RegularFivePrime => "regular 5'",
            AdapterType::AnchoredFivePrime => "anchored 5'",
            AdapterType::Anywhere => "variable 5'/3'",
            AdapterType::Linked => "linked",
        }
    }
}

/// A parsed adapter specification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterSpec {
    /// Adapter name (`{name}` in `-x`/`-y`/`--rename`); defaults to the numeric
    /// index cutadapt assigns, as a string.
    pub name: String,
    pub adapter_type: AdapterType,
    /// The 3' part of the adapter (the whole sequence for non-linked adapters).
    pub sequence: Vec<u8>,
    /// The 5' part of a linked adapter (`^A...B`); empty otherwise.
    pub five_prime_sequence: Vec<u8>,
    /// Declaration order, used to break ties (earlier wins).
    pub index: usize,
}

impl AdapterSpec {
    /// The adapter's declared sequence as shown in the report (`AAA...TTT` for
    /// linked adapters).
    pub fn display_sequence(&self) -> String {
        if self.adapter_type == AdapterType::Linked {
            format!(
                "{}...{}",
                String::from_utf8_lossy(&self.five_prime_sequence),
                String::from_utf8_lossy(&self.sequence)
            )
        } else {
            String::from_utf8_lossy(&self.sequence).into_owned()
        }
    }
}

/// A concrete match of one adapter against one read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    /// Index into the adapter list.
    pub adapter_index: usize,
    /// Cut position: bases `[..cut_from]` and `[cut_to..]` are kept for a 3'
    /// match; for a 5' match `cut_from` is the number of removed 5' bases.
    pub removed_5prime: usize,
    pub removed_3prime: usize,
    /// Number of adapter bases that aligned (used for scoring and reporting).
    pub adapter_bases: usize,
    /// Number of mismatches/indels in the alignment.
    pub errors: usize,
}

impl Match {
    /// Bases left in the read after this match.
    pub fn retained(&self, read_len: usize) -> usize {
        read_len
            .saturating_sub(self.removed_5prime)
            .saturating_sub(self.removed_3prime)
    }

    /// cutadapt's "best match" ordering.
    ///
    /// Established against the cutadapt 4.6 oracle: the match that aligns **more
    /// adapter bases** wins, regardless of where it cuts. Remaining ties go to
    /// the adapter declared first.
    ///
    /// Evidence (all oracle-verified):
    /// * read `GGGGGGGGAGATCGGAAGAGC` with `-a AGATCGGAAGAGC -a GGGGGGGG`:
    ///   the 13-base internal match wins over the 8-base terminal match.
    /// * read `ACGTACGTACGTAAAAGGGG` with `-g ACGTACGTACGT -a GGGG`: the 12-base
    ///   5' match wins over the 4-base 3' match, even though the 3' match would
    ///   leave a shorter read.
    pub fn is_better_than(&self, other: &Match, _read_len: usize) -> bool {
        if self.adapter_bases != other.adapter_bases {
            return self.adapter_bases > other.adapter_bases;
        }
        self.adapter_index < other.adapter_index
    }
}

/// Errors produced while parsing an adapter specification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterParseError {
    Empty,
    EmptyFivePrimePart,
    EmptyThreePrimePart,
    WildcardsNotAllowed(char, usize),
    InvalidLinkedSyntax,
    /// `-b` adapters may not use the linked `^FIVE...THREE` notation.
    LinkedAnywhere,
    InvalidCharacter(char, usize),
    TooLong(usize, usize),
    /// An adapter option letter outside `a`/`g`/`b`/`A`/`G`/`B`.
    ///
    /// Unreachable from the current CLI, whose `value_arg`/`get_many` pairs
    /// only ever pass one of those six letters. It exists so an unknown letter
    /// fails closed (exit 2) rather than being silently treated as a 3'
    /// adapter, which is what the old `debug_assert!` fallback did in release
    /// builds.
    UnknownOption(char),
}

impl fmt::Display for AdapterParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdapterParseError::Empty => write!(f, "the adapter sequence must not be empty"),
            AdapterParseError::EmptyFivePrimePart => {
                write!(f, "the 5' part of the linked adapter must not be empty")
            }
            AdapterParseError::EmptyThreePrimePart => {
                write!(f, "the 3' part of the linked adapter must not be empty")
            }
            AdapterParseError::WildcardsNotAllowed(c, pos) => write!(
                f,
                "invalid character '{c}' at position {pos} in adapter: IUPAC wildcards in \
                 the adapter are not supported by --no-match-adapter-wildcards"
            ),
            AdapterParseError::LinkedAnywhere => {
                // cutadapt prefixes this validation error, and prints it on
                // stdout like its other option-validation messages.
                write!(f, "ERROR: 'anywhere' (-b) adapters may not be linked")
            }
            AdapterParseError::InvalidLinkedSyntax => write!(
                f,
                "a linked adapter must be written as ^FIVE_PRIME...THREE_PRIME"
            ),
            AdapterParseError::InvalidCharacter(c, pos) => write!(
                f,
                "invalid character '{c}' at position {pos} in adapter: only A, C, G, T (and \
                 U, treated as T) plus the anchor characters $, ^ and ... are allowed"
            ),
            AdapterParseError::TooLong(len, max) => {
                write!(f, "adapter is too long ({len} > {max} bp)")
            }
            AdapterParseError::UnknownOption(option) => {
                // Unreachable through the CLI; if it ever becomes reachable this
                // is a usage error (exit 2) rather than silent misbehaviour.
                write!(f, "ERROR: unknown adapter option '{option}'")
            }
        }
    }
}

impl std::error::Error for AdapterParseError {}

/// Split `name=SEQ` into an optional name and the sequence.
///
/// cutadapt accepts `-a NAME=SEQ` (and the same for -g/-b/-A/-G/-B). Only the
/// first `=` separates the two, so a sequence may not contain `=` but a name may
/// be empty (`-a =ACGT` reports "=== Adapter  ==="). Oracle-verified.
pub fn split_inline_name(raw: &str) -> (Option<&str>, &str) {
    match raw.split_once('=') {
        Some((name, sequence)) => (Some(name), sequence),
        None => (None, raw),
    }
}

/// Read adapters from a FASTA file (`-a file:FILE`).
///
/// cutadapt's `file:FILE` notation loads one adapter per FASTA record; the
/// record's name (the first token of the header, without `>`) becomes the
/// adapter name, which the report uses in "=== Adapter NAME ===" and which
/// `{name}` expands to in `-x`/`-y`. Oracle-verified.
pub fn parse_fasta_file(
    option: char,
    path: &str,
    first_index: usize,
) -> Result<Vec<AdapterSpec>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read adapter file {path:?}: {e}"))?;
    let mut specs = Vec::new();
    let mut name: Option<String> = None;
    let mut sequence = String::new();
    let flush = |name: &mut Option<String>,
                 sequence: &mut String,
                 specs: &mut Vec<AdapterSpec>,
                 index: usize|
     -> Result<(), String> {
        if sequence.is_empty() {
            *name = None;
            return Ok(());
        }
        let raw = std::mem::take(sequence);
        let spec = parse(option, &raw, index, name.take().as_deref())
            .map_err(|e| format!("{path}: {e}"))?;
        specs.push(spec);
        Ok(())
    };
    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('>') {
            let index = first_index + specs.len();
            flush(&mut name, &mut sequence, &mut specs, index)?;
            name = Some(header.split_whitespace().next().unwrap_or("").to_string());
        } else {
            if name.is_none() {
                // cutadapt's FASTA reader wording, verbatim.
                return Err(format!(
                    "Error in FASTA file at line {}: Expected '>' at beginning of record, but got '{}'.",
                    line_number + 1,
                    line
                ));
            }
            sequence.push_str(line);
        }
    }
    let index = first_index + specs.len();
    flush(&mut name, &mut sequence, &mut specs, index)?;
    Ok(specs)
}

/// Maximum supported adapter length, matching the v0.1.0 contract.
pub const MAX_ADAPTER_LENGTH: usize = 256;

/// Map an adapter option letter onto the adapter type it selects.
///
/// cutadapt spells read-1 adapters `-a`/`-g`/`-b` and the *same* three types for
/// read 2 as `-A`/`-G`/`-B`. The letter's case therefore carries no extra
/// meaning: `'A'` is exactly "3' adapter for the read this list belongs to",
/// which `cli` relies on when it fills `adapters2` from `-A`/`-G`/`-B`.
///
/// Before this function existed the mapping was a `match` that handled only the
/// lowercase letters, with a `debug_assert!(false, ...)` plus a
/// `RegularThreePrime` fallback for everything else. That was wrong twice over:
///
/// * a debug build aborted on any `-A`/`-G`/`-B` adapter, and
/// * a **release** build silently downgraded `-G`/`-B` to a 3' adapter, so a
///   documented 5' read-2 adapter trimmed the wrong end of the read.
///
/// The second is a user-visible defect, not just a failing test; it was found by
/// external verification of W1-1. See docs/EVIDENCE-W1-1.md (DR-10).
fn base_adapter_type(option: char) -> Result<AdapterType, AdapterParseError> {
    match option {
        'a' | 'A' => Ok(AdapterType::RegularThreePrime),
        'g' | 'G' => Ok(AdapterType::RegularFivePrime),
        'b' | 'B' => Ok(AdapterType::Anywhere),
        other => Err(AdapterParseError::UnknownOption(other)),
    }
}

/// Parse an adapter specification from `-a`, `-g` or `-b`.
///
/// * `option` decides the base type: `-a` 3', `-g` 5', `-b` anywhere.
///   The read-2 spellings `-A`/`-G`/`-B` are accepted and mean the same three
///   types (see `base_adapter_type`).
/// * A leading `^` on `-a`/`-g` makes it anchored (or linked when it also
///   contains `...`).
/// * A trailing `$` on `-a` makes it anchored 3'.
/// * `...` separates the two parts of a linked adapter.
pub fn parse(
    option: char,
    raw: &str,
    index: usize,
    name: Option<&str>,
) -> Result<AdapterSpec, AdapterParseError> {
    if raw.is_empty() {
        return Err(AdapterParseError::Empty);
    }
    let mut text = raw.to_string();
    let anchored_start = text.starts_with('^');
    if anchored_start {
        text.remove(0);
        if text.is_empty() {
            return Err(AdapterParseError::Empty);
        }
    }
    let anchored_end = text.ends_with('$');
    if anchored_end {
        text.pop();
        if text.is_empty() {
            return Err(AdapterParseError::Empty);
        }
    }

    let base_type = base_adapter_type(option)?;

    // Linked syntax `FIVE...THREE`.
    if let Some(pos) = text.find("...") {
        if base_type == AdapterType::Anywhere {
            return Err(AdapterParseError::LinkedAnywhere);
        }
        let (five, three) = (text[..pos].to_string(), text[pos + 3..].to_string());
        if five.is_empty() {
            return Err(AdapterParseError::EmptyFivePrimePart);
        }
        if three.is_empty() {
            return Err(AdapterParseError::EmptyThreePrimePart);
        }
        if three.contains("...") {
            return Err(AdapterParseError::InvalidLinkedSyntax);
        }
        let five = normalize_sequence(&five)?;
        let three = normalize_sequence(&three)?;
        if five.len() > MAX_ADAPTER_LENGTH || three.len() > MAX_ADAPTER_LENGTH {
            return Err(AdapterParseError::TooLong(
                five.len().max(three.len()),
                MAX_ADAPTER_LENGTH,
            ));
        }
        return Ok(AdapterSpec {
            name: name
                .map(str::to_string)
                .unwrap_or_else(|| index.to_string()),
            adapter_type: AdapterType::Linked,
            sequence: three,
            five_prime_sequence: five,
            index,
        });
    }

    let sequence = normalize_sequence(&text)?;
    if sequence.len() > MAX_ADAPTER_LENGTH {
        return Err(AdapterParseError::TooLong(
            sequence.len(),
            MAX_ADAPTER_LENGTH,
        ));
    }
    let adapter_type = match (base_type, anchored_start, anchored_end) {
        (AdapterType::RegularThreePrime, false, false) => AdapterType::RegularThreePrime,
        (AdapterType::RegularThreePrime, false, true) => AdapterType::AnchoredThreePrime,
        (AdapterType::RegularThreePrime, true, _) => {
            // `^ADAPTER` with -a is cutadapt's "anchored 5'" spelling only for
            // linked adapters; a bare `^A` with -a is treated as 5' anchored.
            AdapterType::AnchoredFivePrime
        }
        (AdapterType::RegularFivePrime, true, _) => AdapterType::AnchoredFivePrime,
        (AdapterType::RegularFivePrime, false, true) => {
            return Err(AdapterParseError::InvalidCharacter('$', raw.len()));
        }
        (AdapterType::RegularFivePrime, false, false) => AdapterType::RegularFivePrime,
        (AdapterType::Anywhere, _, _) => AdapterType::Anywhere,
        (AdapterType::Linked, _, _) => AdapterType::Linked,
        (t, _, _) => t,
    };
    Ok(AdapterSpec {
        name: name
            .map(str::to_string)
            .unwrap_or_else(|| index.to_string()),
        adapter_type,
        sequence,
        five_prime_sequence: Vec::new(),
        index,
    })
}

/// Uppercase and validate a plain adapter sequence (A/C/G/T, U -> T).
fn normalize_sequence(text: &str) -> Result<Vec<u8>, AdapterParseError> {
    let mut out = Vec::with_capacity(text.len());
    for (pos, ch) in text.char_indices() {
        let upper = ch.to_ascii_uppercase();
        match upper {
            // IUPAC wildcards are valid adapter characters; the aligner treats
            // any non-ACGT byte as "matches anything" (see `align::compatible`).
            'A' | 'C' | 'G' | 'T' | 'N' | 'X' | 'R' | 'Y' | 'S' | 'W' | 'K' | 'M' | 'B' | 'D'
            | 'H' | 'V' => out.push(upper as u8),
            'U' => out.push(b'T'),
            other => return Err(AdapterParseError::InvalidCharacter(other, pos)),
        }
    }
    if out.is_empty() {
        return Err(AdapterParseError::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regular_three_prime() {
        let a = parse('a', "AGATCGGAAGAGC", 0, None).unwrap();
        assert_eq!(a.adapter_type, AdapterType::RegularThreePrime);
        assert_eq!(a.sequence, b"AGATCGGAAGAGC");
        assert_eq!(a.name, "0");
    }

    #[test]
    fn parses_anchored_three_prime() {
        let a = parse('a', "AGATCGGAAGAGC$", 0, None).unwrap();
        assert_eq!(a.adapter_type, AdapterType::AnchoredThreePrime);
        assert_eq!(a.sequence, b"AGATCGGAAGAGC");
    }

    #[test]
    fn parses_regular_five_prime() {
        let a = parse('g', "ACGTACGT", 1, Some("myname")).unwrap();
        assert_eq!(a.adapter_type, AdapterType::RegularFivePrime);
        assert_eq!(a.name, "myname");
        assert_eq!(a.index, 1);
    }

    #[test]
    fn parses_anchored_five_prime() {
        let a = parse('g', "^ACGTACGT", 0, None).unwrap();
        assert_eq!(a.adapter_type, AdapterType::AnchoredFivePrime);
    }

    #[test]
    fn parses_anywhere() {
        let a = parse('b', "ACGTACGT", 0, None).unwrap();
        assert_eq!(a.adapter_type, AdapterType::Anywhere);
    }

    // Read-2 options (`-A`/`-G`/`-B`) reach this function as uppercase letters,
    // so the mapping must be case-insensitive. Before the fix, a debug build
    // panicked on any read-2 adapter and a release build silently turned `-G`
    // and `-B` into three-prime adapters, which trimmed the wrong end of the
    // read. Found by external verification of W1-1 (docs/EVIDENCE-W1-1.md DR-10).
    #[test]
    fn parses_read2_option_letters_like_their_read1_spelling() {
        for (read2, read1, expected) in [
            ('A', 'a', AdapterType::RegularThreePrime),
            ('G', 'g', AdapterType::RegularFivePrime),
            ('B', 'b', AdapterType::Anywhere),
        ] {
            let upper = parse(read2, "ACGTACGT", 0, None).unwrap();
            let lower = parse(read1, "ACGTACGT", 0, None).unwrap();
            assert_eq!(
                upper.adapter_type, expected,
                "-{read2} must select {expected:?}"
            );
            assert_eq!(
                upper.adapter_type, lower.adapter_type,
                "-{read2} and -{read1} must select the same type"
            );
            assert_eq!(upper.sequence, lower.sequence);
        }
    }

    #[test]
    fn read2_option_letters_keep_their_anchors() {
        // The anchor handling must run *after* the base type is known, so these
        // are the interesting combinations for the uppercase spellings.
        assert_eq!(
            parse('G', "^ACGT", 0, None).unwrap().adapter_type,
            AdapterType::AnchoredFivePrime
        );
        assert_eq!(
            parse('A', "ACGT$", 0, None).unwrap().adapter_type,
            AdapterType::AnchoredThreePrime
        );
        assert_eq!(
            parse('A', "^AAA...TTT", 0, None).unwrap().adapter_type,
            AdapterType::Linked
        );
        // A read-2 anywhere adapter may not be linked, same as its read-1 form.
        assert!(matches!(
            parse('B', "^AAA...TTT", 0, None),
            Err(AdapterParseError::LinkedAnywhere)
        ));
    }

    // Fail closed instead of silently defaulting. Unreachable from the CLI, but
    // the old fallback made a wrong answer look like a right one.
    #[test]
    fn unknown_option_letter_is_an_error_not_a_three_prime_adapter() {
        for option in ['x', 'c', '1', '\0'] {
            assert!(
                matches!(
                    parse(option, "ACGT", 0, None),
                    Err(AdapterParseError::UnknownOption(seen)) if seen == option
                ),
                "option {option:?} must be rejected"
            );
        }
        assert_eq!(
            AdapterParseError::UnknownOption('x').to_string(),
            "ERROR: unknown adapter option 'x'"
        );
    }

    #[test]
    fn parses_linked_adapter() {
        let a = parse('a', "^AAA...TTT", 0, None).unwrap();
        assert_eq!(a.adapter_type, AdapterType::Linked);
        assert_eq!(a.five_prime_sequence, b"AAA");
        assert_eq!(a.sequence, b"TTT");
        assert_eq!(a.display_sequence(), "AAA...TTT");
    }

    #[test]
    fn uppercases_and_converts_uracil() {
        let a = parse('a', "agatcggaagagc", 0, None).unwrap();
        assert_eq!(a.sequence, b"AGATCGGAAGAGC");
        let u = parse('a', "ACGU", 0, None).unwrap();
        assert_eq!(u.sequence, b"ACGT");
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse('a', "", 0, None), Err(AdapterParseError::Empty));
        assert_eq!(parse('a', "$", 0, None), Err(AdapterParseError::Empty));
        assert_eq!(parse('a', "^", 0, None), Err(AdapterParseError::Empty));
        // IUPAC wildcards are valid adapter characters (oracle: -a ACNT exits 0).
        assert!(parse('a', "ACNT", 0, None).is_ok());
        assert!(parse('a', "ACYR", 0, None).is_ok());
        // Characters outside the IUPAC set are still rejected.
        assert!(matches!(
            parse('a', "AC.T", 0, None),
            Err(AdapterParseError::InvalidCharacter('.', 2))
        ));
        assert_eq!(
            parse('a', "^AAA...", 0, None),
            Err(AdapterParseError::EmptyThreePrimePart)
        );
        assert_eq!(
            parse('a', "^...TTT", 0, None),
            Err(AdapterParseError::EmptyFivePrimePart)
        );
    }

    #[test]
    fn match_ordering_prefers_more_aligned_adapter_bases() {
        // Oracle-verified: the 13-base 3' match beats a 12-base 5' match even
        // though the 5' match would leave a shorter read.
        let three = Match {
            adapter_index: 0,
            removed_5prime: 0,
            removed_3prime: 10,
            adapter_bases: 13,
            errors: 0,
        };
        let five = Match {
            adapter_index: 1,
            removed_5prime: 12,
            removed_3prime: 0,
            adapter_bases: 12,
            errors: 0,
        };
        assert!(three.is_better_than(&five, 20));
        assert!(!five.is_better_than(&three, 20));
        // With equal adapter bases, the earlier-declared adapter wins.
        let same_len_later = Match {
            adapter_index: 5,
            removed_5prime: 0,
            removed_3prime: 10,
            adapter_bases: 13,
            errors: 0,
        };
        assert!(three.is_better_than(&same_len_later, 20));
    }

    #[test]
    fn match_ordering_falls_back_to_adapter_order() {
        let first = Match {
            adapter_index: 0,
            removed_5prime: 0,
            removed_3prime: 5,
            adapter_bases: 5,
            errors: 0,
        };
        let second = Match {
            adapter_index: 1,
            removed_5prime: 0,
            removed_3prime: 5,
            adapter_bases: 5,
            errors: 0,
        };
        assert!(first.is_better_than(&second, 20));
        assert!(!second.is_better_than(&first, 20));
    }

    #[test]
    fn retained_is_saturating() {
        let m = Match {
            adapter_index: 0,
            removed_5prime: 10,
            removed_3prime: 10,
            adapter_bases: 10,
            errors: 0,
        };
        assert_eq!(m.retained(15), 0);
        assert_eq!(m.retained(25), 5);
    }

    #[test]
    fn every_type_reports_its_end() {
        assert!(AdapterType::RegularThreePrime.trims_three_prime());
        assert!(!AdapterType::RegularThreePrime.trims_five_prime());
        assert!(AdapterType::RegularFivePrime.trims_five_prime());
        assert!(!AdapterType::RegularFivePrime.trims_three_prime());
        assert!(AdapterType::Anywhere.trims_five_prime());
        assert!(AdapterType::Anywhere.trims_three_prime());
        assert!(AdapterType::Linked.trims_five_prime());
        assert!(AdapterType::Linked.trims_three_prime());
    }
}
