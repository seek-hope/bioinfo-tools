//! Shared input/output plumbing for every subcommand.
//!
//! The FASTX parsing and compression live in `bioinfo-fastx`; this module only
//! adapts them to the shape the commands want: an iterator of owned records, and
//! a writer that knows SeqKit's `-w/--line-width` FASTA wrapping rule.
//!
//! # Why records are owned here
//!
//! `seq` and `fx2tab` need to look at a record more than once (compute an
//! average quality, then print the sequence), and `split` must buffer a whole
//! part. Borrowing from a chunk would force those commands to re-parse or copy
//! anyway, so the adapter owns each record. The parser itself still operates
//! zero-copy on chunks; only the per-record hand-off copies.

use bioinfo_common::exit::ExitClass;
use bioinfo_fastx::parser::{Mode, ParseError};
use bioinfo_fastx::reader::Writer;
use bioinfo_fastx::writer::Layout;
use bioinfo_fastx::{format_from_extension, CompressionLevels, Format, Levels};
use std::io::{self, Write};
use std::path::Path;

/// Which record format the input turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFormat {
    /// Four-line FASTQ.
    Fastq,
    /// `>`-headed FASTA.
    Fasta,
}

impl InputFormat {
    /// The label `stats` prints in its `format` column.
    pub fn label(self) -> &'static str {
        match self {
            InputFormat::Fastq => "FASTQ",
            InputFormat::Fasta => "FASTA",
        }
    }
}

/// One record, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedRecord {
    /// The header with its `@`/`>` marker stripped.
    pub name: Vec<u8>,
    /// The full header line as it appeared, marker included.
    pub raw_header: Vec<u8>,
    /// The sequence.
    pub sequence: Vec<u8>,
    /// The `+` line, or empty for FASTA.
    pub plus: Vec<u8>,
    /// The quality line, or empty for FASTA.
    pub qualities: Vec<u8>,
    /// True when a fourth line was present.
    ///
    /// `qualities` being empty is not enough to distinguish "no quality line"
    /// from "an empty quality line", and `lint`'s `Complete` validator needs the
    /// difference.
    pub has_quality_line: bool,
    /// 1-based record number in this input.
    pub index: u64,
}

impl OwnedRecord {
    /// True when the record carries qualities.
    pub fn is_fastq(&self) -> bool {
        !self.plus.is_empty() || !self.qualities.is_empty()
    }

    /// The sequence ID: the header up to the first whitespace.
    pub fn id(&self) -> &[u8] {
        crate::transform::sequence_id(&self.name)
    }

    /// Write this record in `layout`, wrapping FASTA at `width` (0 = no wrap).
    ///
    /// SeqKit's `-w` default is 60 and applies to FASTA sequence lines. FASTQ is
    /// always four lines: wrapping it would make it unparseable.
    pub fn write_to<W: Write + ?Sized>(
        &self,
        out: &mut W,
        layout: Layout,
        width: usize,
    ) -> io::Result<()> {
        match layout {
            Layout::Fastq => bioinfo_fastx::writer::write_record(
                out,
                &self.raw_header,
                &self.sequence,
                &self.plus,
                &self.qualities,
                Layout::Fastq,
            ),
            Layout::Fasta => {
                // Header: `>` plus the name with any existing marker removed.
                let name = match self.name.first() {
                    Some(b'@') | Some(b'>') => &self.name[1..],
                    _ => &self.name[..],
                };
                out.write_all(b">")?;
                out.write_all(name)?;
                out.write_all(b"\n")?;
                if width == 0 {
                    out.write_all(&self.sequence)?;
                    out.write_all(b"\n")?;
                } else {
                    for chunk in self.sequence.chunks(width) {
                        out.write_all(chunk)?;
                        out.write_all(b"\n")?;
                    }
                    if self.sequence.is_empty() {
                        out.write_all(b"\n")?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// An error from a command, mapped onto the shared exit-code classes.
#[derive(Debug)]
pub struct CommandError {
    /// The class this failure belongs to.
    pub class: ExitClass,
    /// What went wrong, without the `ERROR: ` prefix.
    pub message: String,
    code: Option<i32>,
}

impl CommandError {
    /// A usage failure (exit 2).
    pub fn usage(message: impl Into<String>) -> CommandError {
        CommandError {
            class: ExitClass::Usage,
            message: message.into(),
            code: None,
        }
    }

    /// Attach an upstream-specific process exit code without changing the
    /// diagnostic class used for normal workspace errors.
    pub fn with_code(mut self, code: i32) -> CommandError {
        self.code = Some(code);
        self
    }

    /// A runtime failure discovered while processing (exit 1).
    ///
    /// `fq lint` reports a validation failure this way: the command line was
    /// valid and the input was readable, but the data did not pass validation.
    /// The exit code is 1, not the 3 reserved for a violated input precondition.
    pub fn runtime(message: impl Into<String>) -> CommandError {
        CommandError {
            class: ExitClass::Runtime,
            message: message.into(),
            code: None,
        }
    }

    /// A violated input precondition (exit 3).
    ///
    /// The input was readable but does not satisfy a precondition the command
    /// requires -- for example `split` given a stream where it needs a real file.
    pub fn input(message: impl Into<String>) -> CommandError {
        CommandError {
            class: ExitClass::Input,
            message: message.into(),
            code: None,
        }
    }

    /// An I/O or resource failure (exit 4).
    pub fn io(message: impl Into<String>) -> CommandError {
        CommandError {
            class: ExitClass::Io,
            message: message.into(),
            code: None,
        }
    }

    /// The numeric exit code.
    pub fn exit_code(&self) -> i32 {
        self.code.unwrap_or_else(|| self.class.code())
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<io::Error> for CommandError {
    fn from(error: io::Error) -> Self {
        CommandError::io(error.to_string())
    }
}

impl From<ParseError> for CommandError {
    fn from(error: ParseError) -> Self {
        // Malformed input is an input-precondition failure (exit 3), which is what
        // the shared contract reserves for "input data does not satisfy a
        // documented precondition".
        CommandError::input(error.to_string())
    }
}

/// A streaming, line-oriented reader over one input.
///
/// # Why line-oriented rather than chunk-oriented
///
/// `bioinfo-fastx`'s `Chunk` reader cuts at every fourth newline, which is a
/// **FASTQ** rule. A FASTA record has a variable number of lines, so under that
/// chunking a record can straddle a boundary, and reassembling it across chunks
/// is error-prone. A FASTA file is line-oriented by construction, so this reader
/// buffers one line at a time over the already-decompressed stream: FASTQ takes
/// four lines, FASTA takes a header plus every line up to the next header.
///
/// Memory is bounded by the longest single line, not by the file, so this stays
/// streaming. The parser and the decompressor are still `bioinfo-fastx`'s; only
/// the record assembly lives here.
pub struct RecordReader {
    lines: LineReader,
    format: InputFormat,
    wrapped_fastq: bool,
    /// A line already read but not yet consumed.
    peeked: Option<Vec<u8>>,
    /// Records handed out so far.
    index: u64,
    label: String,
}

impl RecordReader {
    /// Open `path` (`-` for standard input) and detect the record format.
    pub fn open(path: &str) -> Result<RecordReader, CommandError> {
        let mut lines = LineReader::open(path)?;
        // The first non-blank line decides the format. Blank lines before the
        // first record are tolerated (a trailing newline on the previous run, or
        // a comment-free header), and the deciding line must be kept, not
        // discarded.
        let mut first = lines.next()?;
        while matches!(first.as_deref(), Some(line) if line.is_empty()) {
            first = lines.next()?;
        }
        let format = match first.as_deref() {
            None => InputFormat::Fastq,
            Some(line) => match line.first() {
                Some(b'@') => InputFormat::Fastq,
                Some(b'>') => InputFormat::Fasta,
                _ => {
                    return Err(CommandError::input(format!(
                        "{path}: fastx: invalid FASTA/Q format (expected a leading '@' or '>')"
                    )))
                }
            },
        };
        Ok(RecordReader {
            lines,
            format,
            wrapped_fastq: false,
            peeked: first,
            index: 0,
            label: if path == "-" {
                "-".to_string()
            } else {
                path.to_string()
            },
        })
    }

    /// Opt into SeqKit's wrapped FASTQ reader without changing strict readers
    /// used by other commands and by `fq lint`.
    pub(crate) fn seqkit(path: &str) -> Result<RecordReader, CommandError> {
        let mut reader = Self::open(path).map_err(|error| {
            if error.message.contains("invalid FASTA/Q format") {
                CommandError::input("fastx: invalid FASTA/Q format").with_code(255)
            } else {
                error.with_code(255)
            }
        })?;
        reader.wrapped_fastq = true;
        Ok(reader)
    }

    /// The detected input format.
    pub fn format(&self) -> InputFormat {
        self.format
    }

    /// The input label, for diagnostics and `stats`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The next record, or `None` at end of input.
    pub fn next_record(&mut self) -> Option<Result<OwnedRecord, CommandError>> {
        let result = match self.format {
            InputFormat::Fastq if self.wrapped_fastq => self.next_wrapped_fastq(),
            InputFormat::Fastq => self.next_fastq(),
            InputFormat::Fasta => self.next_fasta(),
        };
        match result {
            Ok(Some(parts)) => {
                self.index += 1;
                Some(Ok(OwnedRecord {
                    name: parts.name,
                    raw_header: parts.raw_header,
                    sequence: parts.sequence,
                    plus: parts.plus,
                    qualities: parts.qualities,
                    has_quality_line: parts.has_quality_line,
                    index: self.index,
                }))
            }
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        }
    }

    /// The next line, using the look-ahead when one is buffered.
    fn next_line(&mut self) -> Result<Option<Vec<u8>>, CommandError> {
        match self.peeked.take() {
            Some(line) => Ok(Some(line)),
            None => self.lines.next(),
        }
    }

    /// The next line without consuming it.
    fn peek_line(&mut self) -> Result<Option<&[u8]>, CommandError> {
        if self.peeked.is_none() {
            match self.lines.next()? {
                Some(line) => self.peeked = Some(line),
                None => return Ok(None),
            }
        }
        Ok(self.peeked.as_deref())
    }

    fn next_fastq(&mut self) -> Result<Option<RecordParts>, CommandError> {
        let Some(raw_header) = self.next_line()? else {
            return Ok(None);
        };
        let at = self.lines.line_number();
        // Preserve tolerated trailing padding, but only after proving it is
        // actually EOF. Treating any blank as EOF silently drops later records.
        if raw_header.is_empty() {
            while let Some(line) = self.next_line()? {
                if !line.is_empty() {
                    return Err(CommandError::input(format!(
                        "malformed FASTQ at line {at}: unexpected blank line before more input"
                    )));
                }
            }
            return Ok(None);
        }
        if !raw_header.starts_with(b"@") {
            return Err(CommandError::input(format!(
                "malformed FASTQ at line {at}: expected a header line starting with '@', found {:?}",
                String::from_utf8_lossy(&raw_header[..raw_header.len().min(40)])
            )));
        }
        let sequence = self.required_line("sequence", at)?;
        let plus = self.required_line("separator ('+')", at)?;
        if !plus.starts_with(b"+") {
            return Err(CommandError::input(format!(
                "malformed FASTQ at line {}: expected a separator line starting with '+', found {:?}",
                self.lines.line_number(),
                String::from_utf8_lossy(&plus[..plus.len().min(40)])
            )));
        }
        let qualities = self.required_line("quality", at)?;
        if sequence.len() != qualities.len() {
            return Err(CommandError::input(format!(
                "malformed FASTQ at line {}: sequence and quality lengths differ ({} != {})",
                self.lines.line_number(),
                sequence.len(),
                qualities.len()
            )));
        }
        let name = raw_header[1..].to_vec();
        Ok(Some(RecordParts {
            name,
            raw_header,
            sequence,
            plus,
            qualities,
            has_quality_line: true,
        }))
    }

    fn next_wrapped_fastq(&mut self) -> Result<Option<RecordParts>, CommandError> {
        let invalid = || CommandError::input("fastx: unequal sequence and quality").with_code(255);
        let raw_header = loop {
            match self.next_line()? {
                None => return Ok(None),
                Some(line) if line.is_empty() => continue,
                Some(line) => break line,
            }
        };
        if !raw_header.starts_with(b"@") {
            return Err(invalid());
        }
        let mut sequence = Vec::new();
        loop {
            let line = self.next_line()?.ok_or_else(invalid)?;
            if line.starts_with(b"+") {
                break;
            }
            sequence.extend_from_slice(&line);
        }
        let mut qualities = Vec::new();
        // Even an empty sequence consumes its empty quality line, when present.
        loop {
            match self.next_line()? {
                Some(line) => qualities.extend_from_slice(&line),
                None if sequence.is_empty() => break,
                None => return Err(invalid()),
            }
            if qualities.len() >= sequence.len() {
                break;
            }
        }
        if qualities.len() != sequence.len() {
            return Err(invalid());
        }
        Ok(Some(RecordParts {
            name: raw_header[1..].to_vec(),
            raw_header,
            sequence,
            plus: b"+".to_vec(),
            qualities,
            has_quality_line: true,
        }))
    }

    fn required_line(&mut self, what: &str, record_line: u64) -> Result<Vec<u8>, CommandError> {
        match self.next_line()? {
            Some(line) => Ok(line),
            None => Err(CommandError::input(format!(
                "malformed FASTQ at line {}: truncated record started at line {record_line}: \
                 expected a {what} line, found end of input",
                self.lines.line_number()
            ))),
        }
    }

    fn next_fasta(&mut self) -> Result<Option<RecordParts>, CommandError> {
        // Skip blank lines between records.
        loop {
            match self.peek_line()? {
                None => return Ok(None),
                Some(line) if !line.is_empty() => break,
                Some(_) => {
                    self.next_line()?;
                }
            }
        }
        let raw_header = self.next_line()?.expect("peeked");
        if !raw_header.starts_with(b">") {
            return Err(CommandError::input(format!(
                "{}: fastx: expected a FASTA header line starting with '>', found {:?}",
                self.label,
                String::from_utf8_lossy(&raw_header[..raw_header.len().min(40)])
            )));
        }
        let name = raw_header[1..].to_vec();
        let mut sequence = Vec::new();
        // Every following line up to the next header belongs to this record. A
        // wrapped record joins its lines, which is what SeqKit does.
        while let Some(line) = self.peek_line()? {
            if line.starts_with(b">") {
                break;
            }
            let line = self.next_line()?.expect("peeked");
            sequence.extend_from_slice(&line);
        }
        Ok(Some(RecordParts {
            name,
            raw_header,
            sequence,
            plus: Vec::new(),
            qualities: Vec::new(),
            has_quality_line: false,
        }))
    }
}

/// Buffered line reader over a decompressed stream.
///
/// Compression is detected by content, so a gzip FASTA and a plain one are read
/// the same way.
struct LineReader {
    inner: Box<dyn std::io::BufRead + Send>,
    line: u64,
    buffer: Vec<u8>,
    label: String,
}

impl LineReader {
    fn open(path: &str) -> Result<LineReader, CommandError> {
        use std::io::BufRead;
        let raw: Box<dyn std::io::Read + Send> = if path == "-" {
            Box::new(std::io::stdin())
        } else {
            Box::new(
                std::fs::File::open(path)
                    .map_err(|error| CommandError::io(format!("cannot open {path}: {error}")))?,
            )
        };
        let decompressed = bioinfo_fastx::decompress(raw)
            .map_err(|error| CommandError::io(format!("cannot read {path}: {error}")))?;
        let inner: Box<dyn BufRead + Send> =
            Box::new(std::io::BufReader::with_capacity(64 * 1024, decompressed));
        Ok(LineReader {
            inner,
            line: 0,
            buffer: Vec::new(),
            label: if path == "-" {
                "-".to_string()
            } else {
                path.to_string()
            },
        })
    }

    /// The 1-based number of the most recently returned line.
    fn line_number(&self) -> u64 {
        self.line.max(1)
    }

    /// The next line without its terminator.
    fn next(&mut self) -> Result<Option<Vec<u8>>, CommandError> {
        use std::io::BufRead;
        self.buffer.clear();
        let read = self
            .inner
            .read_until(b'\n', &mut self.buffer)
            .map_err(|error| CommandError::io(format!("cannot read {}: {error}", self.label)))?;
        if read == 0 {
            return Ok(None);
        }
        self.line += 1;
        // Strip the terminator, and one CR so CRLF input is handled.
        if self.buffer.last() == Some(&b'\n') {
            self.buffer.pop();
        }
        if self.buffer.last() == Some(&b'\r') {
            self.buffer.pop();
        }
        Ok(Some(std::mem::take(&mut self.buffer)))
    }
}

/// A reader for `lint`, which must inspect records the strict reader rejects.
///
/// `lint` exists to report a sequence/quality length mismatch or a header without
/// `@`; a generic "malformed FASTQ" message from the strict reader would hide the
/// very validator that names the problem. This reader groups four lines at a time
/// and enforces nothing, so every candidate reaches the validators. A group with
/// fewer than four lines is still described accurately: `has_quality_line` is
/// false, which is what S004 reports.
pub struct LintReader {
    lines: LineReader,
    index: u64,
}

impl LintReader {
    /// Open `path` for validation.
    pub fn open(path: &str) -> Result<LintReader, CommandError> {
        Ok(LintReader {
            lines: LineReader::open(path)?,
            index: 0,
        })
    }

    /// The next record-shaped group of up to four lines.
    pub fn next_record(&mut self) -> Option<Result<OwnedRecord, CommandError>> {
        let mut lines: Vec<Vec<u8>> = Vec::with_capacity(4);
        for _ in 0..4 {
            match self.lines.next() {
                Ok(Some(line)) => lines.push(line),
                Ok(None) => break,
                Err(error) => return Some(Err(error)),
            }
        }
        if lines.is_empty() {
            return None;
        }
        self.index += 1;
        let raw_header = lines[0].clone();
        let name = match raw_header.first() {
            Some(b'@') | Some(b'>') => raw_header[1..].to_vec(),
            _ => raw_header.clone(),
        };
        Some(Ok(OwnedRecord {
            name,
            raw_header,
            sequence: lines.get(1).cloned().unwrap_or_default(),
            plus: lines.get(2).cloned().unwrap_or_default(),
            qualities: lines.get(3).cloned().unwrap_or_default(),
            has_quality_line: lines.len() == 4,
            index: self.index,
        }))
    }
}

/// One record's fields, before the record index is attached.
struct RecordParts {
    name: Vec<u8>,
    raw_header: Vec<u8>,
    sequence: Vec<u8>,
    plus: Vec<u8>,
    qualities: Vec<u8>,
    has_quality_line: bool,
}

/// An output sink: a file (optionally compressed by extension) or stdout.
pub struct Output {
    inner: OutputInner,
}

enum OutputInner {
    Writer(Writer),
    Stdout(io::BufWriter<io::Stdout>),
}

fn check_compression_level(
    level: i32,
    min: i32,
    max: i32,
    format: &str,
) -> Result<(), CommandError> {
    if (min..=max).contains(&level) {
        Ok(())
    } else {
        Err(CommandError::runtime(format!(
            "compression level out of range for {format} format ({min}-{max}): {level}"
        ))
        .with_code(255))
    }
}

pub(crate) fn compression_levels(
    format: Format,
    level: Option<i32>,
) -> Result<CompressionLevels, CommandError> {
    let mut overrides = CompressionLevels::default();
    if let Some(level) = level {
        match format {
            Format::Gzip | Format::Bgzf => {
                check_compression_level(level, 1, 9, "gzip")?;
                overrides.gzip = Some(level as u32);
            }
            Format::Bzip2 => {
                check_compression_level(level, 1, 9, "bzip2")?;
                overrides.bzip2 = Some(level as u32);
            }
            Format::Zstd => {
                check_compression_level(level, 1, 4, "zstd")?;
                overrides.zstd = Some([1, 3, 7, 11][(level - 1) as usize]);
            }
            Format::Xz | Format::Plain => {}
        }
    }
    Ok(overrides)
}

impl Output {
    /// Create the sink for `path` (`-` or absent means stdout).
    pub fn create(path: Option<&str>) -> Result<Output, CommandError> {
        match path {
            None | Some("-") => Ok(Output {
                inner: OutputInner::Stdout(io::BufWriter::new(io::stdout())),
            }),
            Some(path) => {
                let writer = Writer::create(path, Levels::DEFAULT)
                    .map_err(|error| CommandError::io(format!("cannot create {path}: {error}")))?;
                Ok(Output {
                    inner: OutputInner::Writer(writer),
                })
            }
        }
    }

    /// Create the sink with an optional SeqKit-style numeric compression level.
    ///
    /// `None` (the CLI's `-1`) preserves the existing writer defaults. xz and
    /// plain output ignore the setting, as SeqKit does. gzip/bzip2 accept 1–9;
    /// zstd accepts 1–4 and maps those presets to 1, 3, 7 and 11.
    pub fn create_with_level(
        path: Option<&str>,
        level: Option<i32>,
    ) -> Result<Output, CommandError> {
        let Some(level) = level else {
            return Self::create(path);
        };
        let Some(path) = path.filter(|path| *path != "-") else {
            return Self::create(path);
        };
        let format = format_from_extension(Path::new(path));
        let overrides = compression_levels(format, Some(level))?;
        let writer = Writer::create_with_levels(path, overrides)
            .map_err(|error| CommandError::io(format!("cannot create {path}: {error}")))?;
        Ok(Output {
            inner: OutputInner::Writer(writer),
        })
    }

    /// Finish the stream. Must be called: a gzip output without its footer does
    /// not decode.
    pub fn finish(self) -> Result<(), CommandError> {
        match self.inner {
            OutputInner::Writer(writer) => writer.finish().map_err(|error| {
                CommandError::io(format!("cannot finish the output stream: {error}"))
            }),
            OutputInner::Stdout(mut stdout) => stdout
                .flush()
                .map_err(|error| CommandError::io(format!("cannot flush stdout: {error}"))),
        }
    }
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            OutputInner::Writer(writer) => writer.write(buf),
            OutputInner::Stdout(stdout) => stdout.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            OutputInner::Writer(writer) => writer.flush(),
            OutputInner::Stdout(stdout) => stdout.flush(),
        }
    }
}

/// The parser mode every command uses.
///
/// Strict, because SeqKit rejects malformed FASTX and silently accepting it would
/// change results. `bioinfo_fastx::Mode::Lenient` exists for tools that need it;
/// `rust-fx` does not opt in.
pub const MODE: Mode = Mode::Strict;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rust-fx-io-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn label_mapping() {
        assert_eq!(InputFormat::Fastq.label(), "FASTQ");
        assert_eq!(InputFormat::Fasta.label(), "FASTA");
    }

    #[test]
    fn detects_fastq_and_fasta_from_the_first_line() {
        let fastq = temp("det.fastq");
        std::fs::write(&fastq, b"@r1\nACGT\n+\nIIII\n").unwrap();
        assert_eq!(
            RecordReader::open(fastq.to_str().unwrap())
                .unwrap()
                .format(),
            InputFormat::Fastq
        );

        let fasta = temp("det.fasta");
        std::fs::write(&fasta, b">r1\nACGT\n").unwrap();
        assert_eq!(
            RecordReader::open(fasta.to_str().unwrap())
                .unwrap()
                .format(),
            InputFormat::Fasta
        );

        // Leading blank lines before the first record are tolerated.
        let padded = temp("det_pad.fasta");
        std::fs::write(&padded, b"\n>r1\nACGT\n").unwrap();
        assert_eq!(
            RecordReader::open(padded.to_str().unwrap())
                .unwrap()
                .format(),
            InputFormat::Fasta
        );

        let _ = std::fs::remove_file(&fastq);
        let _ = std::fs::remove_file(&fasta);
        let _ = std::fs::remove_file(&padded);
    }

    // A FASTA record may be far longer than one read buffer; joining must not
    // depend on where any internal boundary falls.
    #[test]
    fn reads_a_fasta_record_with_many_sequence_lines() {
        let path = temp("long.fasta");
        let mut text = String::from(">r1\n");
        for _ in 0..5000 {
            text.push_str("ACGTACGTAC\n");
        }
        text.push_str(">r2\nTTTT\n");
        std::fs::write(&path, &text).unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        let first = reader.next_record().unwrap().unwrap();
        assert_eq!(first.sequence.len(), 5000 * 10);
        let second = reader.next_record().unwrap().unwrap();
        assert_eq!(second.sequence, b"TTTT");
        assert!(reader.next_record().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fasta_tolerates_blank_lines_between_records() {
        let path = temp("blank.fasta");
        std::fs::write(&path, b">r1\nACGT\n\n>r2\nTTTT\n").unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        assert_eq!(reader.next_record().unwrap().unwrap().sequence, b"ACGT");
        assert_eq!(reader.next_record().unwrap().unwrap().sequence, b"TTTT");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fasta_accepts_a_header_without_a_sequence() {
        let path = temp("empty_seq.fasta");
        std::fs::write(&path, b">r1\n>r2\nACGT\n").unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        let first = reader.next_record().unwrap().unwrap();
        assert_eq!(first.sequence, b"");
        assert_eq!(reader.next_record().unwrap().unwrap().sequence, b"ACGT");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_records_from_a_plain_file() {
        let path = temp("in.fastq");
        std::fs::write(&path, b"@r1 a\nACGT\n+\nIIII\n@r2\nTTTT\n+\nJJJJ\n").unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        assert_eq!(reader.format(), InputFormat::Fastq);
        let first = reader.next_record().unwrap().unwrap();
        assert_eq!(first.name, b"r1 a");
        assert_eq!(first.id(), b"r1");
        assert_eq!(first.sequence, b"ACGT");
        assert_eq!(first.index, 1);
        let second = reader.next_record().unwrap().unwrap();
        assert_eq!(second.name, b"r2");
        assert_eq!(second.index, 2);
        assert!(reader.next_record().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_multiline_fasta_records() {
        let path = temp("in.fasta");
        std::fs::write(&path, b">r1\nACGT\nGGGG\n>r2\nTTTT\n").unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        assert_eq!(reader.format(), InputFormat::Fasta);
        let first = reader.next_record().unwrap().unwrap();
        // A wrapped FASTA record joins its lines, which is the documented
        // behaviour of `seqkit seq` reading a multi-line FASTA.
        assert_eq!(first.sequence, b"ACGTGGGG");
        let second = reader.next_record().unwrap().unwrap();
        assert_eq!(second.sequence, b"TTTT");
        assert!(reader.next_record().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_across_a_chunk_boundary() {
        // Enough records that the reader must refill at least once.
        let mut text = String::new();
        for i in 0..5000 {
            text.push_str(&format!("@r{i}\nACGTACGT\n+\nIIIIIIII\n"));
        }
        let path = temp("many.fastq");
        std::fs::write(&path, &text).unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        let mut count = 0;
        while let Some(record) = reader.next_record() {
            let record = record.unwrap();
            count += 1;
            assert_eq!(record.index, count);
        }
        assert_eq!(count, 5000);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn malformed_input_is_an_input_error() {
        let path = temp("bad.fastq");
        std::fs::write(&path, b"@r1\nACGTACGT\n+\nIII\n").unwrap();
        let mut reader = RecordReader::open(path.to_str().unwrap()).unwrap();
        let error = match reader.next_record() {
            Some(Err(error)) => error,
            other => panic!("expected an error, got {other:?}"),
        };
        assert_eq!(error.exit_code(), 3, "malformed input is class 3");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_invalid_magic_is_rejected() {
        let path = temp("notfastx.txt");
        std::fs::write(&path, b"hello world\n").unwrap();
        let error = match RecordReader::open(path.to_str().unwrap()) {
            Ok(_) => panic!("must reject a non-FASTX file"),
            Err(error) => error,
        };
        assert_eq!(error.exit_code(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writing_fastq_is_four_lines() {
        let record = OwnedRecord {
            name: b"r1".to_vec(),
            raw_header: b"@r1".to_vec(),
            sequence: b"ACGT".to_vec(),
            plus: b"+".to_vec(),
            qualities: b"IIII".to_vec(),
            has_quality_line: true,
            index: 1,
        };
        let mut out = Vec::new();
        record.write_to(&mut out, Layout::Fastq, 60).unwrap();
        assert_eq!(out, b"@r1\nACGT\n+\nIIII\n");
    }

    #[test]
    fn writing_fasta_relabels_and_drops_qualities() {
        let record = OwnedRecord {
            name: b"r1".to_vec(),
            raw_header: b"@r1".to_vec(),
            sequence: b"ACGT".to_vec(),
            plus: b"+".to_vec(),
            qualities: b"IIII".to_vec(),
            has_quality_line: true,
            index: 1,
        };
        let mut out = Vec::new();
        record.write_to(&mut out, Layout::Fasta, 60).unwrap();
        assert_eq!(out, b">r1\nACGT\n");
    }

    #[test]
    fn fasta_wrapping_uses_the_line_width() {
        let record = OwnedRecord {
            name: b"r1".to_vec(),
            raw_header: b">r1".to_vec(),
            sequence: b"ACGTACGTAC".to_vec(),
            plus: Vec::new(),
            qualities: Vec::new(),
            has_quality_line: false,
            index: 1,
        };
        let mut wrapped = Vec::new();
        record.write_to(&mut wrapped, Layout::Fasta, 4).unwrap();
        assert_eq!(wrapped, b">r1\nACGT\nACGT\nAC\n");

        // Width 0 disables wrapping.
        let mut unwrapped = Vec::new();
        record.write_to(&mut unwrapped, Layout::Fasta, 0).unwrap();
        assert_eq!(unwrapped, b">r1\nACGTACGTAC\n");
    }

    #[test]
    fn an_empty_sequence_still_writes_a_line() {
        let record = OwnedRecord {
            name: b"r1".to_vec(),
            raw_header: b"@r1".to_vec(),
            sequence: Vec::new(),
            plus: b"+".to_vec(),
            qualities: Vec::new(),
            has_quality_line: true,
            index: 1,
        };
        let mut fastq = Vec::new();
        record.write_to(&mut fastq, Layout::Fastq, 60).unwrap();
        assert_eq!(fastq, b"@r1\n\n+\n\n");

        let mut fasta = Vec::new();
        record.write_to(&mut fasta, Layout::Fasta, 60).unwrap();
        assert_eq!(fasta, b">r1\n\n");
    }

    #[test]
    fn output_writes_and_finishes_a_plain_file() {
        let path = temp("out.fastq");
        let _ = std::fs::remove_file(&path);
        {
            let mut output = Output::create(Some(path.to_str().unwrap())).unwrap();
            output.write_all(b"@r1\nACGT\n+\nIIII\n").unwrap();
            output.finish().unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"@r1\nACGT\n+\nIIII\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn output_compresses_by_extension() {
        let path = temp("out.fastq.gz");
        let _ = std::fs::remove_file(&path);
        {
            let mut output = Output::create(Some(path.to_str().unwrap())).unwrap();
            output.write_all(b"@r1\nACGT\n+\nIIII\n").unwrap();
            output.finish().unwrap();
        }
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[..2], &[0x1f, 0x8b], "gzip magic");
        let mut decoder = flate2_decode(&raw);
        let mut text = Vec::new();
        decoder.read_to_end(&mut text).unwrap();
        assert_eq!(text, b"@r1\nACGT\n+\nIIII\n");
        let _ = std::fs::remove_file(&path);
    }

    fn flate2_decode(raw: &[u8]) -> Box<dyn Read> {
        bioinfo_fastx::decompress(Box::new(io::Cursor::new(raw.to_vec()))).unwrap()
    }

    #[test]
    fn error_classes_map_to_the_shared_contract() {
        assert_eq!(CommandError::usage("x").exit_code(), 2);
        assert_eq!(CommandError::input("x").exit_code(), 3);
        assert_eq!(CommandError::io("x").exit_code(), 4);
        // A parse error is class 3.
        let parse = ParseError::new(1, 0, "boom");
        assert_eq!(CommandError::from(parse).exit_code(), 3);
        assert_eq!(CommandError::from(io::Error::other("x")).exit_code(), 4);
    }
}
