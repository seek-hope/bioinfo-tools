//! Sequence transforms used by `seq` and `fq2fa`.
//!
//! Every operation here is documented behaviour from SeqKit 2.14.0's published
//! usage page (retrieved 2026-10-20). The SeqKit binary is not available in this
//! environment, so these are documentation-derived (`docs/EVIDENCE-W1-3.md`
//! DR-1).
//!
//! The transforms are deliberately pure functions over `&[u8]` so they can be
//! unit-tested without any I/O, and so the CLI layer stays a thin wrapper.

/// Complement a single base, preserving case.
///
/// Uses the full IUPAC set so `-p/--complement` does not silently corrupt an
/// ambiguous base. Identity letters (`N`, `X`) map to themselves.
pub fn complement_base(base: u8) -> u8 {
    match base {
        b'A' => b'T',
        b'a' => b't',
        b'T' => b'A',
        b't' => b'a',
        b'U' => b'A',
        b'u' => b'a',
        b'G' => b'C',
        b'g' => b'c',
        b'C' => b'G',
        b'c' => b'g',
        // IUPAC ambiguity codes, each its own complement.
        b'R' => b'Y',
        b'r' => b'y',
        b'Y' => b'R',
        b'y' => b'r',
        b'S' => b'S',
        b's' => b's',
        b'W' => b'W',
        b'w' => b'w',
        b'K' => b'M',
        b'k' => b'm',
        b'M' => b'K',
        b'm' => b'k',
        b'B' => b'V',
        b'b' => b'v',
        b'V' => b'B',
        b'v' => b'b',
        b'D' => b'H',
        b'd' => b'h',
        b'H' => b'D',
        b'h' => b'd',
        // N, X, gaps and anything else are unchanged.
        other => other,
    }
}

/// Reverse a sequence in place order, without complementing (`-r`).
pub fn reverse(sequence: &[u8]) -> Vec<u8> {
    let mut out = sequence.to_vec();
    out.reverse();
    out
}

/// Reverse-complement a sequence (`-r -p`).
///
/// This is the standard "reverse then complement" result, which equals
/// "complement then reverse".
pub fn reverse_complement(sequence: &[u8]) -> Vec<u8> {
    sequence
        .iter()
        .rev()
        .map(|&base| complement_base(base))
        .collect()
}

/// Complement without reversing (`-p` on its own).
pub fn complement(sequence: &[u8]) -> Vec<u8> {
    sequence.iter().copied().map(complement_base).collect()
}

/// Uppercase a sequence (`-u`).
pub fn to_upper(sequence: &[u8]) -> Vec<u8> {
    sequence.to_ascii_uppercase()
}

/// Lowercase a sequence (`-l`).
pub fn to_lower(sequence: &[u8]) -> Vec<u8> {
    sequence.to_ascii_lowercase()
}

/// Remove gap characters (`-g`), where `gaps` is the `-G` set.
///
/// The default set is `"- \t."` (dash, space, tab, dot), matching SeqKit's
/// documented `-G` default. `N` is **not** a gap by default: it is an ambiguous
/// base, not an alignment gap, and removing it would change the biological
/// meaning of the read.
pub fn remove_gaps(sequence: &[u8], gaps: &[u8]) -> Vec<u8> {
    sequence
        .iter()
        .copied()
        .filter(|byte| !gaps.contains(byte))
        .collect()
}

/// DNA to RNA (`--dna2rna`): `T`/`t` become `U`/`u`.
pub fn dna_to_rna(sequence: &[u8]) -> Vec<u8> {
    sequence
        .iter()
        .map(|&b| match b {
            b'T' => b'U',
            b't' => b'u',
            other => other,
        })
        .collect()
}

/// RNA to DNA (`--rna2dna`): `U`/`u` become `T`/`t`.
pub fn rna_to_dna(sequence: &[u8]) -> Vec<u8> {
    sequence
        .iter()
        .map(|&b| match b {
            b'U' => b'T',
            b'u' => b't',
            other => other,
        })
        .collect()
}

/// The transforms `seq` can apply, in the order they are applied.
///
/// Order matters: gaps are removed *before* case changes so the `-G` set is
/// matched against the input's own case, and reverse-complement is applied last
/// so it acts on the final sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transforms {
    /// `-g/--remove-gaps`.
    pub remove_gaps: bool,
    /// `-u/--upper-case`.
    pub upper: bool,
    /// `-l/--lower-case`.
    pub lower: bool,
    /// `-r/--reverse`.
    pub reverse: bool,
    /// `-p/--complement`.
    pub complement: bool,
    /// `--dna2rna`.
    pub dna2rna: bool,
    /// `--rna2dna`.
    pub rna2dna: bool,
}

impl Transforms {
    /// True when no transform is requested.
    pub fn is_identity(&self) -> bool {
        *self == Transforms::default()
    }

    /// Apply every requested transform to `sequence`.
    pub fn apply(&self, sequence: &[u8], gap_letters: &[u8]) -> Vec<u8> {
        let mut out = sequence.to_vec();
        if self.remove_gaps {
            out = remove_gaps(&out, gap_letters);
        }
        if self.upper {
            out = to_upper(&out);
        }
        if self.lower {
            out = to_lower(&out);
        }
        if self.dna2rna {
            out = dna_to_rna(&out);
        }
        if self.rna2dna {
            out = rna_to_dna(&out);
        }
        if self.complement {
            out = complement(&out);
        }
        if self.reverse {
            out.reverse();
        }
        out
    }
}

/// Guess whether a sequence is DNA, RNA or protein from a leading sample.
///
/// Mirrors SeqKit's documented auto-detection ("guessed from the leading
/// subsequences of the first sequences"). The published page names the four
/// output types (`DNA`, `RNA`, `Protein`, `Unlimit`) but not the exact predicate,
/// so the rule below is stated explicitly and pinned by tests. It is recorded as
/// an open question in `docs/EVIDENCE-W1-3.md` DR-5.
///
/// The rule, in order:
///
/// 1. If every byte is a nucleotide (IUPAC set including `N`), the record is
///    nucleic acid. It is `RNA` when it contains `U` and no `T`, otherwise
///    `DNA`. `T` and `U` together is treated as `DNA`, because a single `U` in
///    otherwise-DNA data is a common sequencing artefact and calling the whole
///    file RNA would be the more damaging error.
/// 2. Otherwise, if every byte is an ASCII letter, `*` or `-`, it is `Protein`.
/// 3. Anything else (digits, punctuation, gaps mixed into a nucleotide string) is
///    `Unlimit`, SeqKit's catch-all.
pub fn guess_seq_type(sample: &[u8]) -> &'static str {
    // IUPAC nucleotide codes: A C G T U R Y S W K M B D H V N (either case).
    const NUCLEOTIDE: &[u8] = b"ACGTURYSWKMBDHVNacgturyswkmbdhvn";
    if sample.is_empty() {
        return "Unlimit";
    }
    if sample.iter().all(|byte| NUCLEOTIDE.contains(byte)) {
        let has_t = sample.iter().any(|&b| matches!(b, b'T' | b't'));
        let has_u = sample.iter().any(|&b| matches!(b, b'U' | b'u'));
        return if has_u && !has_t { "RNA" } else { "DNA" };
    }
    if sample
        .iter()
        .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'*' | b'-'))
    {
        return "Protein";
    }
    "Unlimit"
}

/// The sequence ID: the leading run of non-whitespace in a header.
///
/// SeqKit's documented default ID rule is `^(\S+)\s?`, so the ID is everything up
/// to the first whitespace. `header` must already have had its `@`/`>` marker
/// stripped.
pub fn sequence_id(header: &[u8]) -> &[u8] {
    let end = header
        .iter()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(header.len());
    &header[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complement_covers_the_iupac_set() {
        assert_eq!(complement(b"ACGT"), b"TGCA");
        assert_eq!(complement(b"acgt"), b"tgca");
        // IUPAC pairs: R<->Y, S<->S, W<->W, K<->M, B<->V, D<->H
        assert_eq!(complement(b"RYSWKM"), b"YRSWMK");
        assert_eq!(complement(b"BVDH"), b"VBHD");
        // Identity and pass-through
        assert_eq!(complement(b"NNXX"), b"NNXX");
        assert_eq!(complement(b"AC-GT"), b"TG-CA");
    }

    #[test]
    fn complement_is_an_involution() {
        let input = b"ACGTRYSWKMBDHVNacgtryswkmbdhvn-.";
        assert_eq!(complement(&complement(input)), input);
    }

    #[test]
    fn reverse_is_its_own_inverse() {
        assert_eq!(reverse(b"ACGT"), b"TGCA");
        assert_eq!(reverse(&reverse(b"ACGTN")), b"ACGTN");
        assert_eq!(reverse(b""), b"");
    }

    #[test]
    fn reverse_complement_matches_the_two_step_definition() {
        for input in [
            &b"ACGT"[..],
            &b"AGATCGGAAGAGC"[..],
            &b"acgtACGT"[..],
            &b"NNNN"[..],
        ] {
            let two_step = complement(&reverse(input));
            assert_eq!(reverse_complement(input), two_step, "input {input:?}");
        }
        assert_eq!(reverse_complement(b"ATGC"), b"GCAT");
    }

    #[test]
    fn gap_removal_uses_the_documented_default_set() {
        // Default `-G` is "- \t."
        assert_eq!(remove_gaps(b"AC-G T.", DEFAULT), b"ACGT");
        assert_eq!(remove_gaps(b"ACGT", DEFAULT), b"ACGT");
        // N is not a gap.
        assert_eq!(remove_gaps(b"ACNGT", DEFAULT), b"ACNGT");
    }

    const DEFAULT: &[u8] = b"- \t.";

    #[test]
    fn gap_removal_is_configurable() {
        assert_eq!(remove_gaps(b"AC-NGT", b"N"), b"AC-GT");
        assert_eq!(remove_gaps(b"AC-NGT", b"-N"), b"ACGT");
    }

    #[test]
    fn case_conversion_and_transcription() {
        assert_eq!(to_upper(b"acgt"), b"ACGT");
        assert_eq!(to_lower(b"ACGT"), b"acgt");
        assert_eq!(dna_to_rna(b"ACGT"), b"ACGU");
        assert_eq!(dna_to_rna(b"acgt"), b"acgu");
        assert_eq!(rna_to_dna(b"ACGU"), b"ACGT");
        assert_eq!(dna_to_rna(b"ACGU"), b"ACGU", "U is already RNA");
    }

    #[test]
    fn no_transforms_is_the_identity() {
        let transforms = Transforms::default();
        assert!(transforms.is_identity());
        assert_eq!(transforms.apply(b"acgt-N", DEFAULT), b"acgt-N");
    }

    #[test]
    fn gap_removal_happens_before_case_change() {
        // If case changed first, the gap set (lowercase-free) would still match,
        // but the documented order is gaps first; this pins it.
        let transforms = Transforms {
            remove_gaps: true,
            upper: true,
            ..Transforms::default()
        };
        assert_eq!(transforms.apply(b"ac-gt", DEFAULT), b"ACGT");
    }

    #[test]
    fn reverse_complement_is_reverse_after_complement() {
        let transforms = Transforms {
            reverse: true,
            complement: true,
            ..Transforms::default()
        };
        assert_eq!(transforms.apply(b"ATGC", DEFAULT), b"GCAT");
    }

    #[test]
    fn lower_case_wins_over_upper_when_both_are_set() {
        // Both flags is a contradictory request; applying upper then lower is the
        // documented pipeline order, so the result is lower case. The CLI rejects
        // the combination instead of relying on this.
        let transforms = Transforms {
            upper: true,
            lower: true,
            ..Transforms::default()
        };
        assert_eq!(transforms.apply(b"AcGt", DEFAULT), b"acgt");
    }

    #[test]
    fn guanine_rich_dna_is_detected() {
        assert_eq!(guess_seq_type(b"ACGTACGT"), "DNA");
        assert_eq!(guess_seq_type(b"acgtn"), "DNA");
        assert_eq!(guess_seq_type(b"ACGTRYSWKMBDHVN"), "DNA");
    }

    #[test]
    fn rna_requires_uracil_and_no_thymine() {
        assert_eq!(guess_seq_type(b"ACGUACGU"), "RNA");
        assert_eq!(guess_seq_type(b"acgu"), "RNA");
        // T present means DNA, not RNA.
        assert_eq!(guess_seq_type(b"ACGT"), "DNA");
        // Both T and U: the rule prefers DNA, because a stray U in DNA data is a
        // common artefact and mislabelling the whole file RNA is the worse error.
        assert_eq!(guess_seq_type(b"ACGTU"), "DNA");
    }

    #[test]
    fn protein_is_detected_from_non_nucleotide_letters() {
        // L, V, K etc. are not nucleotide codes, but they are letters.
        assert_eq!(guess_seq_type(b"MKVLAAGIVGL"), "Protein");
        assert_eq!(guess_seq_type(b"abcdefghijklmnpqrstvwyz"), "Protein");
        assert_eq!(guess_seq_type(b"MKV*L-"), "Protein");
    }

    #[test]
    fn empty_and_unknown_are_unlimit() {
        assert_eq!(guess_seq_type(b""), "Unlimit");
        assert_eq!(guess_seq_type(b"ACGT!@#"), "Unlimit");
        assert_eq!(guess_seq_type(b"ACGT123"), "Unlimit");
    }

    #[test]
    fn sequence_id_is_the_leading_non_space_run() {
        assert_eq!(sequence_id(b"r1"), b"r1");
        assert_eq!(sequence_id(b"r1 extra stuff"), b"r1");
        assert_eq!(sequence_id(b"r1\textra"), b"r1");
        assert_eq!(sequence_id(b"r1\n"), b"r1");
        assert_eq!(sequence_id(b""), b"");
        // A header whose first character is a space has an empty ID.
        assert_eq!(sequence_id(b" rest"), b"");
    }
}
