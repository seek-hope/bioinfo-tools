//! Auxiliary output files: `--info-file`, `--rest-file`, `--wildcard-file`,
//! `--too-short-output`, `--too-long-output`, `--untrimmed-output` and `--fasta`.
//!
//! Each writer appends one line (or one record) per read, in input order, so the
//! files are deterministic and stream with the main output.
//!
//! Formats were captured from the cutadapt 4.6 oracle; the info-file layout is
//! documented field by field in [`info_line`].

use std::io::Write;

/// Which auxiliary files a run asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuxOutputs {
    pub info_file: Option<std::path::PathBuf>,
    pub rest_file: Option<std::path::PathBuf>,
    pub wildcard_file: Option<std::path::PathBuf>,
    pub too_short_output: Option<std::path::PathBuf>,
    pub too_long_output: Option<std::path::PathBuf>,
    pub untrimmed_output: Option<std::path::PathBuf>,
    /// Write FASTA instead of FASTQ (`--fasta`).
    pub fasta: bool,
    /// True when `-o` (or `-p`) contains `{name}`: one output file per adapter
    /// plus an "unknown" file for reads with no adapter.
    pub demultiplex: bool,
}

impl AuxOutputs {
    pub fn is_empty(&self) -> bool {
        *self == AuxOutputs::default()
    }
}

/// One line of the `--info-file`.
///
/// cutadapt writes tab-separated fields, and only includes the fields that apply
/// to the read. For a 3' match (oracle-verified):
///
/// ```text
/// <name>\t<errors>\t<start>\t<end>\t<sequence-before>\t<matched>\t<rest>\t<1>\t<qualities-before>\t<qualities-matched>\t<qualities-rest>\t
/// ```
///
/// and for a read with no match only:
///
/// ```text
/// <name>\t-1\t<sequence>\t<qualities>
/// ```
///
/// `matched` is the adapter bases that aligned, `rest` the read bases after the
/// adapter, and the trailing `1` is the number of adapters removed.
#[allow(clippy::too_many_arguments)]
pub fn info_line(
    name: &[u8],
    sequence: &[u8],
    qualities: &[u8],
    matched: Option<(usize, usize, usize, usize)>, // (errors, start, end, adapter_bases)
    five_prime: bool,
) -> String {
    let name = String::from_utf8_lossy(name);
    match matched {
        None => format!(
            "{}\t-1\t{}\t{}",
            name,
            String::from_utf8_lossy(sequence),
            String::from_utf8_lossy(qualities)
        ),
        Some((errors, start, end, adapter_bases)) => {
            // `start`/`end` are the adapter's coordinates in the read (oracle:
            // a 13 bp adapter at position 40 reports 40 and 53).
            let (before, matched_seq, rest) = if five_prime {
                (
                    &sequence[..start.min(sequence.len())],
                    &sequence[start.min(sequence.len())..end.min(sequence.len())],
                    &sequence[end.min(sequence.len())..],
                )
            } else {
                let s = start.min(sequence.len());
                let e = (s + adapter_bases).min(sequence.len());
                (&sequence[..s], &sequence[s..e], &sequence[e..])
            };
            let (q_before, q_matched, q_rest) = if five_prime {
                let s = start.min(qualities.len());
                let e = end.min(qualities.len());
                (&qualities[..s], &qualities[s..e], &qualities[e..])
            } else {
                let s = start.min(qualities.len());
                let e = (s + matched_seq.len()).min(qualities.len());
                (&qualities[..s], &qualities[s..e], &qualities[e..])
            };
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t1\t{}\t{}\t{}\t",
                name,
                errors,
                start,
                end,
                String::from_utf8_lossy(before),
                String::from_utf8_lossy(matched_seq),
                String::from_utf8_lossy(rest),
                String::from_utf8_lossy(q_before),
                String::from_utf8_lossy(q_matched),
                String::from_utf8_lossy(q_rest)
            )
        }
    }
}

/// One line of the `--rest-file`: the read bases after the 3' adapter, then the
/// read name.
pub fn rest_line(name: &[u8], rest: &[u8]) -> String {
    // Oracle: "<rest> <name>" separated by a single SPACE.
    format!(
        "{} {}",
        String::from_utf8_lossy(rest),
        String::from_utf8_lossy(name)
    )
}

/// Serialise one record as FASTQ or FASTA.
///
/// Delegates to `bioinfo_fastx::writer::write_record`, which owns the byte layout
/// (W1-2). Note the historical division of labour, preserved deliberately: this
/// helper writes `header` verbatim, while the main output path in `engine.rs`
/// builds its header with the right sigil itself. The shared writer normalises the
/// sigil, so the only case where the two could differ is a header that does not
/// start with `@`/`>` — which `required_line`-style validation upstream already
/// rejects, and which no oracle case produces.
pub fn write_record<W: Write>(
    out: &mut W,
    header: &[u8],
    sequence: &[u8],
    plus: &[u8],
    qualities: &[u8],
    fasta: bool,
) -> std::io::Result<()> {
    let layout = if fasta {
        bioinfo_fastx::Layout::Fasta
    } else {
        bioinfo_fastx::Layout::Fastq
    };
    bioinfo_fastx::writer::write_record(out, header, sequence, plus, qualities, layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_line_for_unmatched_read() {
        assert_eq!(
            info_line(b"none", b"ACGT", b"IIII", None, false),
            "none\t-1\tACGT\tIIII"
        );
    }

    #[test]
    fn info_line_for_three_prime_match() {
        // Oracle: "full\t0\t40\t53\t<before>\t<adapter>\t\t1\t<qbefore>\t<qadapter>\t\t"
        let line = info_line(
            b"full",
            b"GCTAAAGACAATTACATAACATACACGTCAGCACGAAACTAGATCGGAAGAGC",
            &b"I".repeat(53),
            Some((0, 40, 53, 13)),
            false,
        );
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields[0], "full");
        assert_eq!(fields[1], "0");
        assert_eq!(fields[2], "40");
        assert_eq!(fields[3], "53");
        assert_eq!(fields[4], "GCTAAAGACAATTACATAACATACACGTCAGCACGAAACT");
        assert_eq!(fields[5], "AGATCGGAAGAGC");
        assert_eq!(fields[6], "");
        assert_eq!(fields[7], "1");
    }

    #[test]
    fn rest_line_is_rest_then_name() {
        assert_eq!(
            rest_line(b"internal", b"AAGTAAGTGT"),
            // Oracle: rest and name are separated by a single SPACE.
            "AAGTAAGTGT internal"
        );
        assert_eq!(rest_line(b"x", b""), " x");
    }

    #[test]
    fn writes_fastq_and_fasta() {
        let mut fastq = Vec::new();
        write_record(&mut fastq, b"@r", b"ACGT", b"+", b"IIII", false).unwrap();
        assert_eq!(fastq, b"@r\nACGT\n+\nIIII\n");
        let mut fasta = Vec::new();
        write_record(&mut fasta, b">r", b"ACGT", b"+", b"IIII", true).unwrap();
        assert_eq!(fasta, b">r\nACGT\n");
    }
}
