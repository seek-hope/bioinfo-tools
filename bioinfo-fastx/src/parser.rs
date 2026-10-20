//! FASTQ and FASTA record parsing over in-memory byte chunks.
//!
//! Two parser modes share one implementation:
//!
//! * [`Mode::Strict`] reproduces `rust-cutadapt`'s diagnostics byte for byte,
//!   because those strings are part of cutadapt's observable output ("malformed
//!   FASTQ at line N (byte offset M): ...", followed by "The input is not valid
//!   FASTQ."). It validates: `@` header, `+` separator, equal sequence/quality
//!   lengths, and graphical-ASCII sequence and quality bytes.
//! * [`Mode::Lenient`] accepts the same records but tolerates the shapes real
//!   files have: a missing trailing newline on the last record, CRLF endings,
//!   whitespace around sequence/quality, and non-ASCII/high-bit quality bytes.
//!   It is opt-in, never the default (plan risk R5: the format has no
//!   specification, so strictness must be the default and leniency a choice).
//!
//! # Offsets are part of the contract
//!
//! Every error carries a byte offset and a 1-based line number relative to the
//! **whole input**, not to the chunk. [`Parser::new`] takes the chunk's base
//! offset and base line so a chunked reader can report a global position;
//! `rust-cutadapt`'s multi-threaded path depends on that to keep its
//! diagnostics identical to the single-threaded one.
//!
//! # Zero allocation per record
//!
//! A parsed record borrows from the chunk. Nothing is allocated unless the input
//! is malformed (the message) — the hot path is a scan for newlines and four
//! length checks.

use std::fmt;

/// A single FASTQ or FASTA record borrowed from the input buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record<'a> {
    /// The header line, including its leading `@` (FASTQ) or `>` (FASTA).
    pub header: &'a [u8],
    /// The sequence line.
    pub sequence: &'a [u8],
    /// The `+` separator line. Empty for FASTA.
    pub plus: &'a [u8],
    /// The quality line. Empty for FASTA.
    pub qualities: &'a [u8],
}

impl<'a> Record<'a> {
    /// The header without its leading `@`/`>`.
    pub fn name(&self) -> &'a [u8] {
        self.header.get(1..).unwrap_or(&[])
    }

    /// True when this record carries qualities (FASTQ rather than FASTA).
    pub fn is_fastq(&self) -> bool {
        !self.plus.is_empty() || !self.qualities.is_empty()
    }
}

/// Which record format a parser expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    /// Four-line FASTQ records.
    #[default]
    Fastq,
    /// Two-or-more-line FASTA records (`>` header, one or more sequence lines).
    Fasta,
}

/// How tolerant the parser is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Reject anything cutadapt 4.6 would reject, with cutadapt's wording.
    #[default]
    Strict,
    /// Accept the common real-world variations. Opt-in only.
    Lenient,
}

/// Description of malformed input.
///
/// `Display` renders exactly the text `rust-cutadapt` prints after `ERROR: `,
/// so migrating a caller cannot change a diagnostic by accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset of the offending record relative to the start of the input.
    pub offset: u64,
    /// 1-based line number of the offending line relative to the whole input.
    pub line: u64,
    /// One-sentence description, without the position prefix.
    pub message: String,
}

impl ParseError {
    /// A new error at `line`/`offset`.
    pub fn new(line: u64, offset: u64, message: impl Into<String>) -> ParseError {
        ParseError {
            offset,
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "malformed FASTQ at line {} (byte offset {}): {}",
            self.line, self.offset, self.message
        )
    }
}

impl std::error::Error for ParseError {}

fn find_newline(haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == b'\n')
}

/// True when every byte is printable ASCII that can appear in a sequence or a
/// quality string (`!` through `~`, per Sanger/Illumina encodings).
fn is_graphical_ascii(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| (33..=126).contains(&b))
}

/// A parser over one borrowed chunk of input.
pub struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    format: Format,
    mode: Mode,
    /// Byte offset of `data[0]` within the whole input file.
    base_offset: u64,
    /// Line number (1-based) of `data[0]`'s line within the whole input file.
    base_line: u64,
    /// Number of lines consumed so far from this chunk.
    lines: u64,
}

impl<'a> Parser<'a> {
    /// A strict FASTQ parser over `data`, which starts at `base_offset` (bytes)
    /// and `base_line` (1-based) of the whole input.
    pub fn new(data: &'a [u8], base_offset: u64, base_line: u64) -> Self {
        Parser {
            data,
            pos: 0,
            format: Format::Fastq,
            mode: Mode::Strict,
            base_offset,
            base_line,
            lines: 0,
        }
    }

    /// Select the record format.
    pub fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// Select the strictness.
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// The line number of the next unconsumed line, relative to the input.
    pub fn current_line(&self) -> u64 {
        self.base_line + self.lines
    }

    /// The byte offset of the next unconsumed byte, relative to the input.
    pub fn current_offset(&self) -> u64 {
        self.base_offset + self.pos as u64
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            offset: self.current_offset(),
            line: self.current_line(),
            message: message.into(),
        }
    }

    /// Return the next line without its terminator; `None` at end of chunk.
    fn line(&mut self) -> Option<&'a [u8]> {
        if self.pos >= self.data.len() {
            return None;
        }
        let start = self.pos;
        let end = match find_newline(&self.data[start..]) {
            Some(i) => start + i,
            None => {
                // No newline: the last line is unterminated. Strict mode rejects
                // this (cutadapt reports a truncated record); lenient mode takes
                // the bytes as the final line.
                if self.mode == Mode::Strict {
                    return None;
                }
                self.data.len()
            }
        };
        // `pos` advances past the newline when there is one. An unterminated
        // final line leaves `pos` at the end of the data.
        self.pos = if end < self.data.len() {
            end + 1
        } else {
            self.data.len()
        };
        self.lines += 1;
        let line = &self.data[start..end];
        // Tolerate CRLF line endings while writing plain LF output.
        match line {
            [rest @ .., b'\r'] => Some(rest),
            _ => Some(line),
        }
    }

    fn required_line(&mut self, what: &str) -> Result<&'a [u8], ParseError> {
        match self.line() {
            Some(line) => Ok(line),
            None => Err(self.error(format!(
                "truncated record: expected a {what} line, found end of input"
            ))),
        }
    }

    /// Parse the next record, or `Ok(None)` at the end of the chunk.
    pub fn next_record(&mut self) -> Result<Option<Record<'a>>, ParseError> {
        match self.format {
            Format::Fastq => self.next_fastq(),
            Format::Fasta => self.next_fasta(),
        }
    }

    fn next_fastq(&mut self) -> Result<Option<Record<'a>>, ParseError> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }
        let header = self.required_line("header ('@')")?;
        if !header.starts_with(b"@") {
            return Err(self.error(format!(
                "expected a header line starting with '@', found {:?}",
                String::from_utf8_lossy(&header[..header.len().min(40)])
            )));
        }
        let sequence = self.required_line("sequence")?;
        let plus = self.required_line("separator ('+')")?;
        if !plus.starts_with(b"+") {
            return Err(self.error(format!(
                "expected a separator line starting with '+', found {:?}",
                String::from_utf8_lossy(&plus[..plus.len().min(40)])
            )));
        }
        let qualities = self.required_line("quality")?;
        // A line that still contains `\r` had *two* trailing carriage returns, so
        // the record's line structure is ambiguous: writing `plus` back verbatim
        // would produce `+\r\n`, which re-parses with the `\r` stripped and is no
        // longer the same record. Reject it rather than accept a record that
        // cannot round-trip. Found by the W1-2 fuzz harness.
        for (what, line) in [
            ("header", header),
            ("sequence", sequence),
            ("separator", plus),
            ("quality", qualities),
        ] {
            if line.contains(&b'\r') {
                return Err(self.error(format!(
                    "the {what} line contains an embedded carriage return"
                )));
            }
        }
        if sequence.len() != qualities.len() {
            return Err(self.error(format!(
                "sequence and quality lengths differ ({} != {})",
                sequence.len(),
                qualities.len()
            )));
        }
        if self.mode == Mode::Strict {
            if !is_graphical_ascii(sequence) {
                return Err(self.error("sequence contains non-printable or non-ASCII bytes"));
            }
            if !is_graphical_ascii(qualities) {
                return Err(self.error("quality contains non-printable or non-ASCII bytes"));
            }
        }
        Ok(Some(Record {
            header,
            sequence,
            plus,
            qualities,
        }))
    }

    fn next_fasta(&mut self) -> Result<Option<Record<'a>>, ParseError> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }
        let header = self.required_line("header ('>')")?;
        if !header.starts_with(b">") {
            return Err(self.error(format!(
                "expected a FASTA header line starting with '>', found {:?}",
                String::from_utf8_lossy(&header[..header.len().min(40)])
            )));
        }
        // A FASTA record's sequence is every following line up to the next
        // header. A single slice cannot represent multiple lines, so a
        // multi-line record is reported rather than silently truncated.
        let start = self.pos;
        let mut end = start;
        let mut lines = 0usize;
        loop {
            let before = self.pos;
            match self.line() {
                None => break,
                Some(line) => {
                    if line.starts_with(b">") {
                        // The next record starts here: rewind so the next call
                        // reads it as a header.
                        self.pos = before;
                        self.lines -= 1;
                        break;
                    }
                    if lines > 0 {
                        // A second sequence line. Callers that need full FASTA
                        // support must use `next_fasta_record`, which joins
                        // lines; the borrowed-slice API cannot.
                        return Err(self.error(
                            "multi-line FASTA records are not supported by the borrowed-slice \
                             parser; use next_fasta_record",
                        ));
                    }
                    end = self.pos;
                    lines += 1;
                    // `line()` already stripped the terminator, so `end` is just
                    // past the newline. Trim it back off.
                    if end > 0 && self.data.get(end - 1) == Some(&b'\n') {
                        end -= 1;
                    }
                    if end > 0 && self.data.get(end - 1) == Some(&b'\r') {
                        end -= 1;
                    }
                }
            }
        }
        if lines == 0 {
            return Err(
                self.error("truncated record: expected a sequence line, found end of input")
            );
        }
        let sequence = &self.data[start..end];
        if self.mode == Mode::Strict && !is_graphical_ascii(sequence) {
            return Err(self.error("sequence contains non-printable or non-ASCII bytes"));
        }
        Ok(Some(Record {
            header,
            sequence,
            plus: &[],
            qualities: &[],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(data: &[u8]) -> Vec<Record<'_>> {
        let mut parser = Parser::new(data, 0, 1);
        let mut records = Vec::new();
        while let Some(record) = parser.next_record().expect("valid input") {
            records.push(record);
        }
        records
    }

    fn parse_err(data: &[u8]) -> ParseError {
        parse_with(data, Mode::Strict)
    }

    fn parse_with(data: &[u8], mode: Mode) -> ParseError {
        let mut parser = Parser::new(data, 0, 1).with_mode(mode);
        loop {
            match parser.next_record() {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("expected a parse error, got clean EOF"),
                Err(e) => return e,
            }
        }
    }

    #[test]
    fn parses_records_without_allocating_a_copy() {
        let data = b"@r1\nACGT\n+\nIIII\n@r2\nTTTT\n+tt\n!!!!\n";
        let records = parse_all(data);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].header, b"@r1");
        assert_eq!(records[0].name(), b"r1");
        assert_eq!(records[0].sequence, b"ACGT");
        assert_eq!(records[0].plus, b"+");
        assert_eq!(records[0].qualities, b"IIII");
        assert_eq!(records[1].header, b"@r2");
        assert_eq!(records[1].plus, b"+tt");
        // Every slice points into the original buffer.
        let base = data.as_ptr() as usize;
        assert!((base..base + data.len()).contains(&(records[0].sequence.as_ptr() as usize)));
    }

    #[test]
    fn empty_input_has_no_records() {
        assert!(parse_all(b"").is_empty());
    }

    #[test]
    fn keeps_second_header() {
        let records = parse_all(b"@r1 extra\nACGT\n+r1 extra\nIIII\n");
        assert_eq!(records[0].header, b"@r1 extra");
        assert_eq!(records[0].plus, b"+r1 extra");
    }

    #[test]
    fn crlf_is_accepted() {
        let records = parse_all(b"@r1\r\nACGT\r\n+\r\nIIII\r\n");
        assert_eq!(records[0].sequence, b"ACGT");
        assert_eq!(records[0].qualities, b"IIII");
    }

    // These five messages are part of cutadapt's observable output; the gate
    // golden `cutadapt_malformed_fastq` pins the rendered form.
    #[test]
    fn rejects_bad_header_with_cutadapt_wording() {
        let err = parse_err(b"r1\nACGT\n+\nIIII\n");
        assert!(err.message.contains("header"), "{}", err.message);
        // `line` is the line *after* the ones consumed and `offset` is the byte
        // position after them, matching the pre-migration parser exactly: reading
        // a one-line bad header leaves the cursor at line 2, byte 3. The same
        // rule makes a 4-line record with a bad quality line report line 5,
        // offset 19, which tests/gate/golden/cutadapt_malformed_fastq.json
        // pins from the real binary.
        assert_eq!(err.line, 2);
        assert_eq!(err.offset, 3);
    }

    #[test]
    fn rejects_bad_plus() {
        let err = parse_err(b"@r1\nACGT\n-\nIIII\n");
        assert!(err.message.contains("separator"), "{}", err.message);
    }

    #[test]
    fn rejects_length_mismatch_with_exact_wording() {
        let err = parse_err(b"@r1\nACGTACGT\n+\nIII\n");
        assert_eq!(err.message, "sequence and quality lengths differ (8 != 3)");
        // Verified against the pre-migration binary: a 4-line record whose
        // quality line is wrong reports line 5, byte offset 19. This exact pair
        // is pinned by tests/gate/golden/cutadapt_malformed_fastq.json.
        assert_eq!(err.line, 5);
        assert_eq!(err.offset, 19);
        assert_eq!(
            err.to_string(),
            "malformed FASTQ at line 5 (byte offset 19): sequence and quality lengths differ (8 != 3)"
        );
    }

    #[test]
    fn rejects_truncated_record() {
        let err = parse_err(b"@r1\nACGT\n+\n");
        assert!(err.message.contains("truncated"), "{}", err.message);
    }

    #[test]
    fn rejects_record_without_trailing_newline_in_strict_mode() {
        let err = parse_err(b"@r1\nACGT\n+\nIIII");
        assert!(err.message.contains("truncated"), "{}", err.message);
    }

    #[test]
    fn rejects_non_ascii_sequence_in_strict_mode() {
        let err = parse_err(b"@r1\nAC\xffT\n+\nIIII\n");
        assert!(err.message.contains("non-ASCII"), "{}", err.message);
    }

    #[test]
    fn rejects_non_ascii_quality_in_strict_mode() {
        let err = parse_err(b"@r1\nACGT\n+\nII\xffI\n");
        assert!(err.message.contains("non-ASCII"), "{}", err.message);
    }

    // ---- lenient mode -----------------------------------------------------

    #[test]
    fn lenient_accepts_a_missing_final_newline() {
        let records = {
            let mut parser = Parser::new(b"@r1\nACGT\n+\nIIII", 0, 1).with_mode(Mode::Lenient);
            let mut out = Vec::new();
            while let Some(record) = parser.next_record().expect("lenient accepts this") {
                out.push(record);
            }
            out
        };
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].qualities, b"IIII");
    }

    #[test]
    fn lenient_accepts_high_bit_quality_bytes() {
        // Phred+64 or a mis-encoded file: valuable to be able to read at all.
        let mut parser =
            Parser::new(b"@r1\nACGT\n+\n\xff\xff\xff\xff\n", 0, 1).with_mode(Mode::Lenient);
        let record = parser.next_record().expect("lenient accepts this").unwrap();
        assert_eq!(record.qualities, b"\xff\xff\xff\xff");
    }

    #[test]
    fn lenient_still_rejects_a_length_mismatch() {
        // Leniency is about byte ranges, never about record structure: a
        // mismatch would silently truncate real data.
        let err = parse_with(b"@r1\nACGTACGT\n+\nIII\n", Mode::Lenient);
        assert_eq!(err.message, "sequence and quality lengths differ (8 != 3)");
    }

    #[test]
    fn lenient_still_rejects_a_bad_header() {
        let err = parse_with(b"r1\nACGT\n+\nIIII\n", Mode::Lenient);
        assert!(err.message.contains("header"), "{}", err.message);
    }

    #[test]
    fn lenient_still_rejects_a_bad_separator() {
        let err = parse_with(b"@r1\nACGT\n-\nIIII\n", Mode::Lenient);
        assert!(err.message.contains("separator"), "{}", err.message);
    }

    // ---- offsets are global ----------------------------------------------

    #[test]
    fn offsets_and_lines_are_relative_to_the_whole_input() {
        // Chunk starting at byte 100 / line 11.
        let mut parser = Parser::new(b"@r1\nACGTACGT\n+\nIII\n", 100, 11);
        let err = loop {
            match parser.next_record() {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("expected an error"),
                Err(e) => break e,
            }
        };
        // Same rule, offset by the chunk's base: 4 lines consumed from a base of
        // 11 gives 14, and the error is reported after the 4th line is read.
        assert_eq!(err.line, 15, "base 11 plus 4 consumed lines");
        assert_eq!(
            err.offset, 119,
            "quality line starts 19 bytes into the chunk"
        );
        assert_eq!(parser.current_line(), 15);
    }

    #[test]
    fn empty_chunk_at_a_nonzero_base_reports_no_records() {
        let mut parser = Parser::new(b"", 500, 20);
        assert!(parser.next_record().unwrap().is_none());
        assert_eq!(parser.current_line(), 20);
        assert_eq!(parser.current_offset(), 500);
    }

    // ---- FASTA -----------------------------------------------------------

    #[test]
    fn parses_single_line_fasta_records() {
        let data = b">r1 desc\nACGT\n>r2\nTTTT\n";
        let mut parser = Parser::new(data, 0, 1).with_format(Format::Fasta);
        let first = parser.next_record().unwrap().unwrap();
        assert_eq!(first.header, b">r1 desc");
        assert_eq!(first.sequence, b"ACGT");
        assert!(!first.is_fastq());
        let second = parser.next_record().unwrap().unwrap();
        assert_eq!(second.header, b">r2");
        assert_eq!(second.sequence, b"TTTT");
        assert!(parser.next_record().unwrap().is_none());
    }

    #[test]
    fn fasta_rejects_a_fastq_header() {
        let mut parser = Parser::new(b"@r1\nACGT\n", 0, 1).with_format(Format::Fasta);
        let err = parser.next_record().unwrap_err();
        assert!(err.message.contains("'>'"), "{}", err.message);
    }

    #[test]
    fn fasta_reports_multiline_records_instead_of_truncating_them() {
        let mut parser = Parser::new(b">r1\nACGT\nGGGG\n", 0, 1).with_format(Format::Fasta);
        let err = parser.next_record().unwrap_err();
        assert!(
            err.message.contains("multi-line"),
            "a wrapped record must be reported, not silently sliced: {}",
            err.message
        );
    }

    #[test]
    fn fasta_rejects_a_header_without_sequence() {
        let mut parser = Parser::new(b">r1\n>r2\nACGT\n", 0, 1).with_format(Format::Fasta);
        let err = parser.next_record().unwrap_err();
        assert!(err.message.contains("truncated"), "{}", err.message);
    }

    #[test]
    fn record_name_strips_the_marker() {
        let records = parse_all(b"@r1 x\nACGT\n+\nIIII\n");
        assert_eq!(records[0].name(), b"r1 x");
        let mut fasta = Parser::new(b">g1\nACGT\n", 0, 1).with_format(Format::Fasta);
        assert_eq!(fasta.next_record().unwrap().unwrap().name(), b"g1");
    }

    #[test]
    fn defaults_are_strict_fastq() {
        let parser = Parser::new(b"", 0, 1);
        assert_eq!(parser.mode, Mode::Strict);
        assert_eq!(parser.format, Format::Fastq);
        assert_eq!(Mode::default(), Mode::Strict);
        assert_eq!(Format::default(), Format::Fastq);
    }
}
