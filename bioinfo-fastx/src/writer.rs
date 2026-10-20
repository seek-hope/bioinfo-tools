//! Serialising records as FASTQ or FASTA.
//!
//! The byte layout is a contract: migrating a caller must not change one output
//! byte. It is exactly what `rust-cutadapt` already writes, including the
//! detail that a FASTQ record's separator line is echoed verbatim (so `+r1` in
//! the input stays `+r1` in the output) and that FASTA output drops the
//! separator and quality lines.

use std::io::{self, Write};

/// Which layout to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Layout {
    /// Four lines per record: `@header`, sequence, `+plus`, qualities.
    #[default]
    Fastq,
    /// Two lines per record: `>header`, sequence.
    Fasta,
}

impl Layout {
    /// True when the layout carries qualities.
    pub fn has_qualities(self) -> bool {
        matches!(self, Layout::Fastq)
    }
}

/// Write one record.
///
/// `header` is the record's header line as parsed, i.e. including its `@` (or
/// `>`) marker, and `plus` includes its `+`. [`write_record`] rewrites the
/// header marker to match `layout`, which is what `rust-cutadapt --fasta` does:
/// a FASTQ record `@r1` becomes the FASTA record `>r1`. Verified against the
/// pre-migration binary.
pub fn write_record<W: Write + ?Sized>(
    out: &mut W,
    header: &[u8],
    sequence: &[u8],
    plus: &[u8],
    qualities: &[u8],
    layout: Layout,
) -> io::Result<()> {
    write_header(out, header, layout)?;
    out.write_all(b"\n")?;
    write_line(out, sequence)?;
    if layout == Layout::Fasta {
        return Ok(());
    }
    // A trailing `\r` would be read back as part of the line terminator, so
    // writing it verbatim would not round-trip. The parser rejects such lines, so
    // this cannot normally fire; stripping keeps a hand-built record from
    // producing an ambiguous stream.
    write_line(out, plus)?;
    write_line(out, qualities)?;
    Ok(())
}

/// Write `line` followed by a bare LF, without any trailing CR.
fn write_line<W: Write + ?Sized>(out: &mut W, line: &[u8]) -> io::Result<()> {
    match line {
        [rest @ .., b'\r'] => out.write_all(rest)?,
        _ => out.write_all(line)?,
    }
    out.write_all(b"\n")
}

/// Write a header line with the marker `layout` requires.
///
/// The marker is normalised rather than trusted: `--fasta` output must start each
/// record with `>`, even when the input record was FASTQ and its header therefore
/// started with `@`. Any existing marker is dropped and the right one written, so
/// a caller that passes `@r1`, `>r1` or a bare `r1` gets the same correct line.
fn write_header<W: Write + ?Sized>(out: &mut W, header: &[u8], layout: Layout) -> io::Result<()> {
    let marker = match layout {
        Layout::Fasta => b'>',
        Layout::Fastq => b'@',
    };
    let name = match header.first() {
        Some(b'@') | Some(b'>') => &header[1..],
        _ => header,
    };
    out.write_all(&[marker])?;
    // Same reason as `write_line`: a trailing CR belongs to the terminator.
    let name = match name {
        [rest @ .., b'\r'] => rest,
        _ => name,
    };
    out.write_all(name)
}

/// Write one record from borrowed parts, naming the sequence by header text.
///
/// A convenience for synthesised output (`>` for FASTA, `@` for FASTQ).
pub fn write_named_record<W: Write + ?Sized>(
    out: &mut W,
    name: &[u8],
    sequence: &[u8],
    qualities: &[u8],
    layout: Layout,
) -> io::Result<()> {
    match layout {
        Layout::Fasta => {
            out.write_all(b">")?;
            out.write_all(name)?;
            out.write_all(b"\n")?;
            out.write_all(sequence)?;
            out.write_all(b"\n")
        }
        Layout::Fastq => {
            out.write_all(b"@")?;
            out.write_all(name)?;
            out.write_all(b"\n")?;
            out.write_all(sequence)?;
            out.write_all(b"\n+\n")?;
            out.write_all(qualities)?;
            out.write_all(b"\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(
        header: &[u8],
        sequence: &[u8],
        plus: &[u8],
        qualities: &[u8],
        layout: Layout,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        write_record(&mut out, header, sequence, plus, qualities, layout).unwrap();
        out
    }

    #[test]
    fn fastq_layout_is_four_lines() {
        assert_eq!(
            render(b"@r1", b"ACGT", b"+", b"IIII", Layout::Fastq),
            b"@r1\nACGT\n+\nIIII\n"
        );
    }

    // The separator line is echoed, not normalised to a bare '+'.
    #[test]
    fn the_separator_line_is_preserved_verbatim() {
        assert_eq!(
            render(b"@r1 extra", b"ACGT", b"+r1 extra", b"IIII", Layout::Fastq),
            b"@r1 extra\nACGT\n+r1 extra\nIIII\n"
        );
    }

    // `--fasta` re-labels a FASTQ record: `@r1` becomes `>r1`. Verified against
    // the pre-migration binary, which writes `>r1\nACGTACGTACGT\n` for a FASTQ
    // input.
    #[test]
    fn fasta_layout_drops_the_separator_and_relabels_the_header() {
        assert_eq!(
            render(b"@r1", b"ACGT", b"+", b"IIII", Layout::Fasta),
            b">r1\nACGT\n"
        );
        assert_eq!(
            render(b">r1", b"ACGT", b"+", b"IIII", Layout::Fasta),
            b">r1\nACGT\n"
        );
    }

    #[test]
    fn fastq_layout_relabels_a_fasta_header() {
        assert_eq!(
            render(b">r1", b"ACGT", b"+", b"IIII", Layout::Fastq),
            b"@r1\nACGT\n+\nIIII\n"
        );
    }

    #[test]
    fn a_header_without_a_marker_gets_one() {
        assert_eq!(
            render(b"r1", b"ACGT", b"+", b"IIII", Layout::Fastq),
            b"@r1\nACGT\n+\nIIII\n"
        );
        assert_eq!(
            render(b"r1", b"ACGT", b"", b"", Layout::Fasta),
            b">r1\nACGT\n"
        );
    }

    #[test]
    fn only_the_first_marker_is_stripped() {
        // A name that legitimately starts with `@`/`>` keeps its second copy.
        assert_eq!(
            render(b"@@odd", b"ACGT", b"+", b"IIII", Layout::Fastq),
            b"@@odd\nACGT\n+\nIIII\n"
        );
    }

    #[test]
    fn an_empty_sequence_still_writes_both_lines() {
        assert_eq!(
            render(b"@r1", b"", b"+", b"", Layout::Fastq),
            b"@r1\n\n+\n\n"
        );
        assert_eq!(render(b">r1", b"", b"", b"", Layout::Fasta), b">r1\n\n");
    }

    #[test]
    fn a_record_without_qualities_renders_as_empty_lines() {
        // This is what cutadapt writes for a read trimmed to nothing.
        assert_eq!(
            render(b"@r1", b"", b"+", b"", Layout::Fastq),
            b"@r1\n\n+\n\n"
        );
    }

    #[test]
    fn named_records_get_the_right_marker() {
        let mut fastq = Vec::new();
        write_named_record(&mut fastq, b"r1", b"ACGT", b"IIII", Layout::Fastq).unwrap();
        assert_eq!(fastq, b"@r1\nACGT\n+\nIIII\n");

        let mut fasta = Vec::new();
        write_named_record(&mut fasta, b"r1", b"ACGT", b"", Layout::Fasta).unwrap();
        assert_eq!(fasta, b">r1\nACGT\n");
    }

    #[test]
    fn layout_capabilities() {
        assert!(Layout::Fastq.has_qualities());
        assert!(!Layout::Fasta.has_qualities());
        assert_eq!(Layout::default(), Layout::Fastq);
    }

    // A record written and re-parsed must be identical: the round-trip contract.
    #[test]
    fn write_then_parse_round_trips() {
        use crate::parser::Parser;
        let original = b"@r1 desc\nACGTACGT\n+r1 desc\nIIIIIIII\n@r2\nTTTT\n+\n!!!!\n";
        let mut parser = Parser::new(original, 0, 1);
        let mut out = Vec::new();
        while let Some(record) = parser.next_record().unwrap() {
            write_record(
                &mut out,
                record.header,
                record.sequence,
                record.plus,
                record.qualities,
                Layout::Fastq,
            )
            .unwrap();
        }
        assert_eq!(out, original);
    }
}
