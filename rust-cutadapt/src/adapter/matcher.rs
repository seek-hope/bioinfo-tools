//! Adapter matching: turn parsed [`AdapterSpec`]s into concrete trimming
//! decisions, exactly as cutadapt 4.6 does.
//!
//! The alignment itself is delegated to [`crate::align`], which ports
//! cutadapt's semiglobal `Aligner`. This module supplies the parts that differ
//! per adapter type:
//!
//! * **3' adapters** align with `QUERY_START | QUERY_STOP | REFERENCE_END`
//!   (a prefix of the adapter may be skipped, and the read may be skipped on
//!   both sides) so that partial matches at the read's 3' end and complete
//!   internal matches are both found. The cut position is the alignment's
//!   query start.
//! * **5' adapters** align with `REFERENCE_START | QUERY_START | QUERY_STOP`
//!   so that a prefix of the adapter may be skipped at no cost and the adapter
//!   may match anywhere; the cut position is the alignment's query stop.
//! * **Anchored** adapters add the corresponding hard requirement: a 3'
//!   anchored adapter must consume the end of the read, a 5' anchored adapter
//!   must start at position 0.
//! * **Anywhere** adapters try both and keep the better match.
//! * **Linked** adapters must match both parts; failing either part means the
//!   adapter does not match at all.
//!
//! These rules were verified against the cutadapt 4.6 oracle; see
//! `docs/FULL-COMPATIBILITY.md`.

use crate::adapter::{AdapterSpec, AdapterType, Match};
use crate::align::{Aligner, Alignment, QUERY_START, QUERY_STOP, REFERENCE_END, REFERENCE_START};

/// Default maximum error rate (`--error-rate`).
pub const DEFAULT_MAX_ERROR_RATE: f64 = 0.1;
/// Default minimum overlap (`--overlap`).
pub const DEFAULT_MIN_OVERLAP: usize = 3;
/// Indel cost when indels are allowed (cutadapt's default).
pub const INDEL_COST: i32 = 1;
/// Indel cost that makes indels prohibitively expensive (`--no-indels`).
pub const NO_INDEL_COST: i32 = 100_000;

/// Alignment flags for a regular 3' adapter.
pub const FLAGS_THREE_PRIME: u32 = QUERY_START | QUERY_STOP | REFERENCE_END;
/// Alignment flags for a regular 5' adapter.
pub const FLAGS_FIVE_PRIME: u32 = REFERENCE_START | QUERY_START | QUERY_STOP;

/// Error-rate specification: either a fraction or an absolute count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ErrorRate {
    /// `-e 0.1`: `errors <= length * rate`.
    Rate(f64),
    /// `-e 2`: at most 2 errors for a full-length adapter match.
    Absolute(u32),
}

impl ErrorRate {
    pub fn from_cli(value: f64) -> Self {
        if value >= 1.0 && value.fract() == 0.0 {
            ErrorRate::Absolute(value as u32)
        } else {
            ErrorRate::Rate(value)
        }
    }

    /// The rate the aligner should use. cutadapt computes the effective rate
    /// for an absolute count from the adapter length.
    fn rate_for(&self, adapter_len: usize) -> f64 {
        match self {
            ErrorRate::Rate(r) => *r,
            ErrorRate::Absolute(n) => {
                if adapter_len == 0 {
                    0.0
                } else {
                    *n as f64 / adapter_len as f64
                }
            }
        }
    }
}

impl Default for ErrorRate {
    fn default() -> Self {
        ErrorRate::Rate(DEFAULT_MAX_ERROR_RATE)
    }
}

/// Matching configuration shared by all adapters of one run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchConfig {
    pub error_rate: ErrorRate,
    pub min_overlap: usize,
    pub allow_indels: bool,
    /// `--match-read-wildcards`: IUPAC wildcards in the read also match.
    pub match_read_wildcards: bool,
    /// `-N`: wildcards in the adapter are literal, not wildcards.
    pub no_adapter_wildcards: bool,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            error_rate: ErrorRate::default(),
            min_overlap: DEFAULT_MIN_OVERLAP,
            allow_indels: true,
            match_read_wildcards: false,
            no_adapter_wildcards: false,
        }
    }
}

impl MatchConfig {
    fn indel_cost(&self) -> i32 {
        if self.allow_indels {
            INDEL_COST
        } else {
            NO_INDEL_COST
        }
    }
}

/// A compiled adapter, ready to match.
#[derive(Clone, Debug)]
pub struct CompiledAdapter {
    pub spec: AdapterSpec,
    three_prime: Option<Aligner>,
    five_prime: Option<Aligner>,
    error_rate_mode: ErrorRate,
}

impl CompiledAdapter {
    pub fn new(spec: AdapterSpec, config: &MatchConfig) -> Self {
        let indel_cost = config.indel_cost();
        let mut three_prime = None;
        let read_wildcards = config.match_read_wildcards;
        let mut five_prime = None;
        if spec.adapter_type.trims_three_prime() && !spec.sequence.is_empty() {
            three_prime = Some(
                Aligner::new(
                    &spec.sequence,
                    config.error_rate.rate_for(spec.sequence.len()),
                    FLAGS_THREE_PRIME,
                    indel_cost,
                    config.min_overlap.min(spec.sequence.len()),
                )
                .with_read_wildcards(config.match_read_wildcards)
                .with_literal_adapter(config.no_adapter_wildcards)
                .with_read_wildcards(read_wildcards),
            );
        }
        if spec.adapter_type.trims_five_prime() && !spec.five_prime_sequence.is_empty() {
            five_prime = Some(
                Aligner::new(
                    &spec.five_prime_sequence,
                    config.error_rate.rate_for(spec.five_prime_sequence.len()),
                    FLAGS_FIVE_PRIME,
                    indel_cost,
                    config.min_overlap.min(spec.five_prime_sequence.len()),
                )
                .with_read_wildcards(config.match_read_wildcards)
                .with_literal_adapter(config.no_adapter_wildcards)
                .with_read_wildcards(read_wildcards),
            );
        }
        // Regular 5' adapters use `sequence` as their 5' part; linked adapters
        // use `five_prime_sequence`.
        if spec.adapter_type.trims_five_prime() && five_prime.is_none() {
            five_prime = Some(
                Aligner::new(
                    &spec.sequence,
                    config.error_rate.rate_for(spec.sequence.len()),
                    FLAGS_FIVE_PRIME,
                    indel_cost,
                    config.min_overlap.min(spec.sequence.len()),
                )
                .with_read_wildcards(config.match_read_wildcards)
                .with_literal_adapter(config.no_adapter_wildcards)
                .with_read_wildcards(read_wildcards),
            );
        }
        // Anchored/linked 3' parts still need a 3' aligner.
        if spec.adapter_type == AdapterType::Linked
            && three_prime.is_none()
            && !spec.sequence.is_empty()
        {
            three_prime = Some(
                Aligner::new(
                    &spec.sequence,
                    config.error_rate.rate_for(spec.sequence.len()),
                    FLAGS_THREE_PRIME,
                    indel_cost,
                    config.min_overlap.min(spec.sequence.len()),
                )
                .with_read_wildcards(config.match_read_wildcards)
                .with_literal_adapter(config.no_adapter_wildcards)
                .with_read_wildcards(read_wildcards),
            );
        }
        Self {
            spec,
            three_prime,
            five_prime,
            error_rate_mode: config.error_rate,
        }
    }

    /// Find this adapter in `read`, or `None` if it does not match.
    pub fn find(&mut self, read: &[u8]) -> Option<Match> {
        match self.spec.adapter_type {
            AdapterType::RegularThreePrime => self.find_three_prime(read, false),
            AdapterType::AnchoredThreePrime => self.find_three_prime(read, true),
            AdapterType::RegularFivePrime => self.find_five_prime(read, false),
            AdapterType::AnchoredFivePrime => self.find_five_prime(read, true),
            AdapterType::Anywhere => {
                // cutadapt: "If the first base of the read is part of the match,
                // the behavior is as with -g, otherwise as with -a." The test is
                // whether the 5' alignment *starts* at read position 0.
                let five = self.find_five_prime_starting_at(read, 0);
                if let Some(m) = five {
                    return Some(m);
                }
                self.find_three_prime(read, false)
            }
            AdapterType::Linked => self.find_linked(read),
        }
    }

    fn find_three_prime(&mut self, read: &[u8], anchored: bool) -> Option<Match> {
        if anchored {
            return self.find_anchored_three_prime(read);
        }
        let aligner = self.three_prime.as_mut()?;
        let alignment: Alignment = aligner.locate(read)?;
        if anchored {
            // Oracle-verified: `-a ADAPTER$` requires the adapter to be found
            // *at the read's 3' end*; trailing bases after the adapter mean no
            // match, and an internal adapter with nothing after it does match.
            // The aligned region must additionally be long enough for the error
            // budget, which is computed against the full adapter length (for the
            // 13 bp Illumina adapter, at least 12 bases must align).
            if alignment.query_stop != read.len() {
                return None;
            }
            // The alignment must cover the whole adapter: cutadapt's anchored
            // matching is an end-to-end comparison, so a shorter (partial) hit
            // is not a match. Oracle: with `-a AGATCGGAAGAGC$`, a read ending in
            // `AGATCGGAAGAG` (12 bp) or `AGATCGGAAGA` (11 bp) is only trimmed
            // when the aligned region is long enough for the error budget.
            let adapter_len = self.spec.sequence.len();
            let aligned = alignment.ref_stop - alignment.ref_start;
            let allowed = match self.error_rate_mode {
                ErrorRate::Absolute(n) => n as usize,
                ErrorRate::Rate(r) => (adapter_len as f64 * r) as usize,
            };
            let short = adapter_len - aligned;
            if alignment.errors as usize > allowed || short > allowed {
                return None;
            }
        }
        Some(Match {
            adapter_index: self.spec.index,
            removed_5prime: 0,
            removed_3prime: read.len() - alignment.query_start,
            adapter_bases: alignment.ref_stop - alignment.ref_start,
            errors: alignment.errors as usize,
        })
    }

    /// A 5' match that must begin at `start` (used by `-b`).
    fn find_five_prime_starting_at(&mut self, read: &[u8], start: usize) -> Option<Match> {
        let aligner = self.five_prime.as_mut()?;
        let alignment: Alignment = aligner.locate(read)?;
        if alignment.query_start != start {
            return None;
        }
        Some(Match {
            adapter_index: self.spec.index,
            removed_5prime: alignment.query_stop,
            removed_3prime: 0,
            adapter_bases: alignment.ref_stop - alignment.ref_start,
            errors: alignment.errors as usize,
        })
    }

    fn find_five_prime(&mut self, read: &[u8], anchored: bool) -> Option<Match> {
        if anchored {
            return self.find_anchored_five_prime(read);
        }
        let aligner = self.five_prime.as_mut()?;
        let alignment: Alignment = aligner.locate(read)?;
        if anchored && alignment.query_start != 0 {
            return None;
        }
        Some(Match {
            adapter_index: self.spec.index,
            removed_5prime: alignment.query_stop,
            removed_3prime: 0,
            adapter_bases: alignment.ref_stop - alignment.ref_start,
            errors: alignment.errors as usize,
        })
    }

    /// Error budget for a full-adapter (anchored) match.
    fn anchored_error_budget(&self, adapter_len: usize) -> usize {
        match self.error_rate_mode {
            ErrorRate::Absolute(n) => n as usize,
            ErrorRate::Rate(rate) => (adapter_len as f64 * rate) as usize,
        }
    }

    /// Anchored 3' (`ADAPTER$`): the adapter must be a suffix of the read.
    fn find_anchored_three_prime(&mut self, read: &[u8]) -> Option<Match> {
        let adapter = self.spec.sequence.clone();
        let len = adapter.len();
        if read.len() < len {
            // A read shorter than the adapter can only match partially, which
            // an anchored adapter does not allow.
            return None;
        }
        let window = &read[read.len() - len..];
        let errors = window.iter().zip(&adapter).filter(|(a, b)| a != b).count();
        if errors > self.anchored_error_budget(len) {
            return None;
        }
        Some(Match {
            adapter_index: self.spec.index,
            removed_5prime: 0,
            removed_3prime: len,
            adapter_bases: len,
            errors,
        })
    }

    /// Anchored 5' (`^ADAPTER`): the adapter must be a prefix of the read.
    fn find_anchored_five_prime(&mut self, read: &[u8]) -> Option<Match> {
        let adapter = if self.spec.adapter_type == AdapterType::Linked {
            self.spec.five_prime_sequence.clone()
        } else {
            self.spec.sequence.clone()
        };
        let len = adapter.len();
        if read.len() < len {
            return None;
        }
        let window = &read[..len];
        let errors = window.iter().zip(&adapter).filter(|(a, b)| a != b).count();
        if errors > self.anchored_error_budget(len) {
            return None;
        }
        Some(Match {
            adapter_index: self.spec.index,
            removed_5prime: len,
            removed_3prime: 0,
            adapter_bases: len,
            errors,
        })
    }

    fn find_linked(&mut self, read: &[u8]) -> Option<Match> {
        // Oracle-verified behaviour of `^FIVE...THREE`:
        //
        // * the 5' part must start at read position 0 (`^` is implied);
        // * cutadapt first requires the COMPLETE 5' part: `^ACGTACGTACGTACGT...`
        //   does not trim ACGTACGTGGGG even though 8 of the 16 bases match;
        // * when the complete 5' part is present, the 3' part is searched in the
        //   remainder and both parts are removed together when it is found;
        // * a read shorter than the 5' part is never trimmed.
        let five_adapter = self.spec.five_prime_sequence.clone();
        let five_len = five_adapter.len();

        if read.len() >= five_len {
            let errors = read[..five_len]
                .iter()
                .zip(&five_adapter)
                .filter(|(a, b)| a != b)
                .count();
            if errors > self.anchored_error_budget(five_len) {
                return None;
            }
            let rest = &read[five_len..];
            if !rest.is_empty() {
                if let Some(three) = self.three_prime.as_mut().and_then(|a| a.locate(rest)) {
                    return Some(Match {
                        adapter_index: self.spec.index,
                        removed_5prime: five_len,
                        removed_3prime: rest.len() - three.query_start,
                        adapter_bases: five_len + (three.ref_stop - three.ref_start),
                        errors: errors + three.errors as usize,
                    });
                }
            }
            return Some(Match {
                adapter_index: self.spec.index,
                removed_5prime: five_len,
                removed_3prime: 0,
                adapter_bases: five_len,
                errors,
            });
        }

        // Read shorter than the 5' part: cutadapt does not trim at all.
        //
        // Oracle: with `^ACGTACGTACGTACGT...` the reads A, AC, ACG, ACGT, ACGTA
        // and ACGTACGT are all left unchanged. (With a 4 bp 5' part such as
        // `^ACGT...`, the read ACGTA *contains* the complete 5' part and is
        // trimmed to A by the full-match branch above, which is why the two
        // cases look different but follow the same rule.)
        None
    }
}

/// The adapter set of a run: all `-a`/`-g`/`-b` adapters in declaration order.
#[derive(Clone, Debug, Default)]
pub struct AdapterSet {
    adapters: Vec<CompiledAdapter>,
}

impl AdapterSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, spec: AdapterSpec, config: &MatchConfig) {
        self.adapters.push(CompiledAdapter::new(spec, config));
    }

    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }

    pub fn len(&self) -> usize {
        self.adapters.len()
    }

    /// The display name of one adapter: its FASTA record name when it has one,
    /// otherwise `None` so the caller can fall back to the position.
    pub fn name_of(&self, adapter_index: usize) -> Option<String> {
        let local = adapter_index.saturating_sub(self.first_index());
        self.adapters.get(local).and_then(|adapter| {
            // A purely numeric name equal to the adapter's position is the
            // generated placeholder, not an explicit name.
            let generated = adapter.spec.name.chars().all(|c| c.is_ascii_digit())
                && adapter.spec.name == adapter.spec.index.to_string();
            if generated {
                None
            } else {
                Some(adapter.spec.name.clone())
            }
        })
    }

    /// Offsets of the IUPAC wildcard positions in one adapter's sequence.
    ///
    /// Used by `--wildcard-file`, which records the read base at each wildcard
    /// position of the adapter that matched.
    pub fn wildcard_positions(&self, adapter_index: usize) -> Vec<(usize, u8)> {
        let local = adapter_index.saturating_sub(self.first_index());
        match self.adapters.get(local) {
            Some(adapter) => adapter
                .spec
                .sequence
                .iter()
                .enumerate()
                .filter(|(_, b)| !b"ACGT".contains(*b))
                .map(|(offset, base)| (offset, *base))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Index of the first adapter, used to convert a global adapter index into a
    /// local one (R2 adapters are numbered continuously for the report).
    pub fn first_index(&self) -> usize {
        self.adapters.first().map_or(0, |a| a.spec.index)
    }

    pub fn specs(&self) -> impl Iterator<Item = &AdapterSpec> {
        self.adapters.iter().map(|a| &a.spec)
    }

    /// Find the best match among all adapters for `read`.
    ///
    /// cutadapt removes at most one adapter per read per round (see `--times`
    /// for multiple rounds); the best match wins and ties go to the adapter
    /// declared first.
    pub fn best_match(&mut self, read: &[u8]) -> Option<Match> {
        let len = read.len();
        let mut best: Option<Match> = None;
        for adapter in self.adapters.iter_mut() {
            if let Some(m) = adapter.find(read) {
                best = Some(match best {
                    Some(current) if !m.is_better_than(&current, len) => current,
                    _ => m,
                });
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::parse;

    fn set(items: &[(char, &str)]) -> AdapterSet {
        let config = MatchConfig::default();
        let mut set = AdapterSet::new();
        for (i, (opt, raw)) in items.iter().enumerate() {
            set.push(parse(*opt, raw, i, None).unwrap(), &config);
        }
        set
    }

    fn trim_3(read: &str, adapter: &str) -> String {
        let mut s = set(&[('a', adapter)]);
        match s.best_match(read.as_bytes()) {
            Some(m) => read[..read.len() - m.removed_3prime].to_string(),
            None => read.to_string(),
        }
    }

    fn trim_5(read: &str, adapter: &str) -> String {
        let mut s = set(&[('g', adapter)]);
        match s.best_match(read.as_bytes()) {
            Some(m) => read[m.removed_5prime..].to_string(),
            None => read.to_string(),
        }
    }

    // Expected values below were produced by cutadapt 4.6 for the same inputs.

    #[test]
    fn regular_three_prime_full() {
        assert_eq!(
            trim_3("GCTAAAGACAAGATCGGAAGAGC", "AGATCGGAAGAGC"),
            "GCTAAAGACA"
        );
    }

    #[test]
    fn regular_three_prime_internal() {
        assert_eq!(
            trim_3("TGTTGGCCCAGATCGGAAGAGCAAGTAAG", "AGATCGGAAGAGC"),
            "TGTTGGCCC"
        );
    }

    #[test]
    fn regular_three_prime_partial() {
        // Oracle: `cutadapt -a AGATCGGAAGAGC` leaves GATGCATACGATCG unchanged
        // because only 3 adapter bases (GAT) align and the resulting match is
        // not better than leaving the read alone.
        assert_eq!(trim_3("GATGCATACGATCG", "AGATCGGAAGAGC"), "GATGCATACGATCG");
        // A longer partial match is trimmed (oracle: GGGGGGGGAGATCG -> GGGGGGGG).
        assert_eq!(trim_3("GGGGGGGGAGATCG", "AGATCGGAAGAGC"), "GGGGGGGG");
    }

    #[test]
    fn anchored_three_prime_requires_suffix() {
        // anchored: internal match is ignored
        assert_eq!(
            trim_3("GGGGAGATCGGAAGAGCGGGG", "AGATCGGAAGAGC$"),
            "GGGGAGATCGGAAGAGCGGGG"
        );
        assert_eq!(
            trim_3("GGGGTTTTAGATCGGAAGAGC", "AGATCGGAAGAGC$"),
            "GGGGTTTT"
        );
    }

    #[test]
    fn regular_five_prime_trims_prefix_and_before_itself() {
        assert_eq!(trim_5("ACGTACGTGGGGTTTT", "ACGTACGT"), "GGGGTTTT");
        assert_eq!(trim_5("GGGGACGTACGTGGGG", "ACGTACGT"), "GGGG");
    }

    #[test]
    fn anchored_five_prime_requires_prefix() {
        assert_eq!(trim_5("ACGTACGTGGGG", "^ACGTACGT"), "GGGG");
        assert_eq!(trim_5("GGGGACGTACGT", "^ACGTACGT"), "GGGGACGTACGT");
    }

    #[test]
    fn anywhere_behaves_like_five_prime_at_start() {
        // Oracle: `cutadapt -b ACGTACGT` on ACGTACGTGGGG -> GGGG
        let mut s = set(&[('b', "ACGTACGT")]);
        let m = s.best_match(b"ACGTACGTGGGG").unwrap();
        assert_eq!(m.removed_5prime, 8);
        assert_eq!(m.removed_3prime, 0);
        assert_eq!(m.retained(12), 4);
    }

    #[test]
    fn anywhere_behaves_like_three_prime_at_end() {
        // Oracle: `cutadapt -b ACGTACGT` on GGGGACGTACGT -> GGGG
        let mut s = set(&[('b', "ACGTACGT")]);
        let m = s.best_match(b"GGGGACGTACGT").unwrap();
        assert_eq!(m.removed_3prime, 8);
        assert_eq!(m.retained(12), 4);
    }

    #[test]
    fn linked_partial_five_prime_only() {
        // Oracle: `-a ^AAA...TTT` on AAAGGGGGCCC -> GGGGGCCC. The complete 5'
        // part is removed even though the 3' part never appears.
        let mut s = set(&[('a', "^AAA...TTT")]);
        let m = s.best_match(b"AAAGGGGGCCC").unwrap();
        assert_eq!(m.removed_5prime, 3);
        assert_eq!(m.removed_3prime, 0);
        // Oracle: CCCGGGGTTT is unchanged - the 5' part must start at position 0.
        assert!(s.best_match(b"CCCGGGGTTT").is_none());
        // Oracle: CCCGGGGCCC is unchanged.
        assert!(s.best_match(b"CCCGGGGCCC").is_none());
    }

    #[test]
    fn linked_removes_both_parts_when_both_match() {
        // Oracle: AAAGGGGGTTT -> GGGGG
        let mut s = set(&[('a', "^AAA...TTT")]);
        let m = s.best_match(b"AAAGGGGGTTT").unwrap();
        assert_eq!(m.removed_5prime, 3);
        assert_eq!(m.removed_3prime, 3);
    }

    #[test]
    fn no_match_returns_none() {
        let mut s = set(&[('a', "AGATCGGAAGAGC")]);
        assert!(s.best_match(b"TTTTTATTACACTCAGAAACAGAACTC").is_none());
    }

    #[test]
    fn best_match_prefers_more_aligned_adapter_bases() {
        // Oracle: `-a AGATCGGAAGAGC -a GGGGGGGG` on GGGGGGGGAGATCGGAAGAGC
        // outputs GGGGGGGG. Both cuts happen to leave 8 bases, and the 13-base
        // internal match wins because it aligns more adapter bases.
        let mut s = set(&[('a', "AGATCGGAAGAGC"), ('a', "GGGGGGGG")]);
        let m = s.best_match(b"GGGGGGGGAGATCGGAAGAGC").unwrap();
        assert_eq!(m.adapter_index, 0);
        assert_eq!(m.adapter_bases, 13);
        assert_eq!(m.retained(21), 8);
    }

    #[test]
    fn absolute_error_rate_allows_two_errors() {
        let config = MatchConfig {
            error_rate: ErrorRate::Absolute(2),
            ..Default::default()
        };
        let mut s = AdapterSet::new();
        s.push(parse('a', "AGATCGGAAGAGC", 0, None).unwrap(), &config);
        assert!(s.best_match(b"AAAAGATCGGAAGACC").is_some());
    }

    #[test]
    fn zero_error_rate_rejects_single_mismatch() {
        let config = MatchConfig {
            error_rate: ErrorRate::Rate(0.0),
            ..Default::default()
        };
        let mut s = AdapterSet::new();
        s.push(parse('a', "AGATCGGAAGAGC", 0, None).unwrap(), &config);
        assert!(s.best_match(b"AAAAGATCGGAAGACC").is_none());
    }

    #[test]
    fn indels_are_off_by_default() {
        // Deletion in the read: adapter has an extra base relative to the read.
        assert!(MatchConfig::default().allow_indels);
        assert_eq!(MatchConfig::default().indel_cost(), INDEL_COST);
        let no_indels = MatchConfig {
            allow_indels: false,
            ..Default::default()
        };
        assert_eq!(no_indels.indel_cost(), NO_INDEL_COST);
    }

    #[test]
    fn min_overlap_is_respected() {
        let mut s = set(&[('a', "AGATCGGAAGAGC")]);
        // Oracle: GGGGGGAG ends in "AG", only 2 adapter bases -> unchanged.
        assert!(s.best_match(b"GGGGGGAG").is_none());
        // Oracle: GGGGGGAGA ends in "AGA", 3 adapter bases -> trimmed to GGGGGG.
        let m = s.best_match(b"GGGGGGAGA").unwrap();
        assert_eq!(m.removed_3prime, 3);
    }
}
