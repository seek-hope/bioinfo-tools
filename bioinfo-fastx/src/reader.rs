//! High-level streaming reader and writer facades.
//!
//! These wrap the pieces so a caller needs a few lines instead of ten:
//!
//! ```no_run
//! use bioinfo_fastx::{reader::Reader, parser::Parser};
//! # fn main() -> std::io::Result<()> {
//! let mut reader = Reader::open("reads.fastq.gz")?;
//! let mut buffer = Vec::new();
//! while let Some(chunk) = reader.next_chunk(&mut buffer, 256 * 1024)? {
//!     let mut parser = Parser::new(chunk.data, chunk.offset, chunk.line);
//!     while let Some(record) = parser.next_record().expect("valid FASTQ") {
//!         let _ = record.sequence;
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! The chunked interface is what the existing binaries' pipelines need, because
//! it lets a worker pool process record-aligned chunks in parallel while the
//! output stays byte-identical. [`Reader::read_to_end`] exists for inputs known
//! to be small.

use crate::compress::{self, compressor, CompressionLevels, Levels};
use crate::format::{format_from_extension, Format};
use crate::writer::Layout;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

/// One record-aligned chunk of the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk<'a> {
    /// The chunk's bytes. Every chunk but the last ends on a record boundary.
    pub data: &'a [u8],
    /// Byte offset of `data[0]` in the decompressed input.
    pub offset: u64,
    /// 1-based line number of the chunk's first line in the decompressed input.
    pub line: u64,
}

/// A streaming, decompressing FASTX reader.
///
/// The compression format is detected from the first bytes of the stream, so a
/// gzip file with a `.fastq` name and a compressed stdin stream both work.
pub struct Reader {
    inner: Box<dyn Read + Send>,
    label: String,
    chunk_size: usize,
    /// Bytes handed to the caller so far, in the decompressed stream.
    consumed_bytes: u64,
    /// Lines handed to the caller so far.
    consumed_lines: u64,
    /// Bytes read from `inner` but not yet handed out.
    carry: Vec<u8>,
    eof: bool,
}

impl Reader {
    /// Open `path`, or standard input when it is `-`.
    pub fn open(path: &str) -> io::Result<Reader> {
        let (raw, label): (Box<dyn Read + Send>, String) = if path == "-" {
            (Box::new(io::stdin()), "-".to_string())
        } else {
            let file = File::open(path)?;
            (Box::new(file), path.to_string())
        };
        Reader::from_stream(raw, label)
    }

    /// Wrap an already-open stream and decompress it by content.
    pub fn from_stream(
        stream: Box<dyn Read + Send>,
        label: impl Into<String>,
    ) -> io::Result<Reader> {
        Ok(Reader {
            inner: compress::decompress(stream)?,
            label: label.into(),
            chunk_size: crate::chunker::DEFAULT_CHUNK_SIZE,
            consumed_bytes: 0,
            // Line numbers are 1-based, matching the parser and cutadapt.
            consumed_lines: 1,
            carry: Vec::new(),
            eof: false,
        })
    }

    /// A reader over in-memory bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> io::Result<Reader> {
        Reader::from_stream(Box::new(io::Cursor::new(bytes)), "<memory>")
    }

    /// What this stream is called, for diagnostics.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Override the chunk size used by [`Reader::next_chunk`].
    pub fn with_chunk_size(mut self, bytes: usize) -> Reader {
        self.chunk_size = bytes.max(4096);
        self
    }

    /// Read the next record-aligned chunk, or `None` at end of input.
    ///
    /// `buffer` is cleared and refilled, so one allocation is reused across the
    /// whole stream.
    pub fn next_chunk<'b>(
        &mut self,
        buffer: &'b mut Vec<u8>,
        chunk_size: usize,
    ) -> io::Result<Option<Chunk<'b>>> {
        let chunk_size = chunk_size.max(4096);
        let mut incoming = vec![0u8; chunk_size];
        loop {
            if let Some(split) = crate::chunker::last_complete_record_end(&self.carry) {
                return Ok(Some(self.emit_upto(buffer, split)));
            }
            if self.eof {
                if self.carry.is_empty() {
                    return Ok(None);
                }
                // A trailing partial record: hand it over so the parser reports
                // it with the position it would have had unchunked.
                let len = self.carry.len();
                return Ok(Some(self.emit_upto(buffer, len)));
            }
            let filled = read_up_to(&mut self.inner, &mut incoming)?;
            if filled == 0 {
                self.eof = true;
            } else {
                self.carry.extend_from_slice(&incoming[..filled]);
            }
        }
    }

    /// Move the first `split` bytes of `carry` into `buffer` and describe them.
    fn emit_upto<'b>(&mut self, buffer: &'b mut Vec<u8>, split: usize) -> Chunk<'b> {
        // `drain` keeps the allocation of `carry` rather than splitting it.
        buffer.clear();
        buffer.extend(self.carry.drain(..split));
        let offset = self.consumed_bytes;
        let line = self.consumed_lines;
        self.consumed_bytes += split as u64;
        self.consumed_lines += buffer.iter().filter(|&&b| b == b'\n').count() as u64;
        Chunk {
            data: buffer.as_slice(),
            offset,
            line,
        }
    }

    /// Drain the whole stream into one buffer, decompressed.
    ///
    /// Only for inputs known to be small; [`Reader::next_chunk`] is the streaming
    /// interface.
    pub fn read_to_end(&mut self) -> io::Result<Vec<u8>> {
        let mut out = std::mem::take(&mut self.carry);
        self.carry = Vec::new();
        self.consumed_bytes += out.len() as u64;
        self.inner.read_to_end(&mut out)?;
        Ok(out)
    }
}

fn read_up_to(reader: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// A compressing FASTX writer whose format follows the destination name.
pub struct Writer {
    inner: Box<dyn compress::Compressor>,
    format: Format,
}

impl Writer {
    /// Create `path` (or standard output for `-`), compressing by extension.
    pub fn create(path: &str, levels: Levels) -> io::Result<Writer> {
        if path == "-" {
            let sink: Box<dyn Write + Send> = Box::new(io::stdout());
            return Ok(Writer {
                inner: compress::compressor_for(sink, Format::Plain, levels)?,
                format: Format::Plain,
            });
        }
        let target = Path::new(path);
        Ok(Writer {
            inner: compressor(target, levels)?,
            format: format_from_extension(target),
        })
    }

    /// Compress an arbitrary sink with an explicit format.
    pub fn from_sink(
        sink: Box<dyn Write + Send>,
        format: Format,
        levels: Levels,
    ) -> io::Result<Writer> {
        Ok(Writer {
            inner: compress::compressor_for(sink, format, levels)?,
            format,
        })
    }

    /// Create `path` with explicit per-format compression overrides.
    ///
    /// This is the numeric-level counterpart to [`Writer::create`]. Missing
    /// overrides use the shared defaults; callers should validate upstream
    /// ranges before opening the destination.
    pub fn create_with_levels(path: &str, overrides: CompressionLevels) -> io::Result<Writer> {
        if path == "-" {
            let sink: Box<dyn Write + Send> = Box::new(io::stdout());
            return Writer::from_sink_with_levels(sink, Format::Plain, overrides);
        }
        let target = Path::new(path);
        Ok(Writer {
            inner: compress::compressor_with_levels(target, overrides)?,
            format: format_from_extension(target),
        })
    }

    /// Compress an arbitrary sink with explicit per-format overrides.
    pub fn from_sink_with_levels(
        sink: Box<dyn Write + Send>,
        format: Format,
        overrides: CompressionLevels,
    ) -> io::Result<Writer> {
        Ok(Writer {
            inner: compress::compressor_for_with_levels(sink, format, overrides)?,
            format,
        })
    }

    /// The format this writer selected.
    pub fn format(&self) -> Format {
        self.format
    }

    /// Write one record.
    pub fn write_record(
        &mut self,
        header: &[u8],
        sequence: &[u8],
        plus: &[u8],
        qualities: &[u8],
        layout: Layout,
    ) -> io::Result<()> {
        crate::writer::write_record(&mut self.inner, header, sequence, plus, qualities, layout)
    }

    /// Finish the compressed stream. Must be called: an unfinished gzip stream
    /// has no footer and will not decode.
    pub fn finish(self) -> io::Result<()> {
        self.inner.finish()
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn fastq(n: usize) -> String {
        (0..n)
            .map(|i| format!("@r{i}\nACGTACGT\n+\nIIIIIIII\n"))
            .collect()
    }

    /// A path unique to this test thread.
    ///
    /// Tests run concurrently in one process, so a name keyed only on the PID is
    /// shared between them and two tests can delete each other's files.
    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "bioinfo-fastx-io-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn reads_and_parses_a_plain_file() {
        let path = temp_path("plain.fastq");
        std::fs::write(&path, fastq(3)).unwrap();
        let mut reader = Reader::open(path.to_str().unwrap()).unwrap();
        assert_eq!(reader.read_to_end().unwrap(), fastq(3).as_bytes());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_a_gzip_file_by_content_even_with_a_plain_name() {
        let path = temp_path("disguised.fastq");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(3));
        encoder.write_all(fastq(2).as_bytes()).unwrap();
        std::fs::write(&path, encoder.finish().unwrap()).unwrap();
        let mut reader = Reader::open(path.to_str().unwrap()).unwrap();
        assert_eq!(reader.read_to_end().unwrap(), fastq(2).as_bytes());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writes_and_reads_back_every_format() {
        for name in [
            "a.fastq",
            "a.fastq.gz",
            "a.fastq.bz2",
            "a.fastq.xz",
            "a.fastq.zst",
        ] {
            let path = temp_path(name);
            let _ = std::fs::remove_file(&path);
            {
                let mut writer = Writer::create(path.to_str().unwrap(), Levels::DEFAULT).unwrap();
                writer
                    .write_record(b"@r1", b"ACGT", b"+", b"IIII", Layout::Fastq)
                    .unwrap();
                writer.finish().unwrap();
            }
            let mut reader = Reader::open(path.to_str().unwrap()).unwrap();
            assert_eq!(
                reader.read_to_end().unwrap(),
                b"@r1\nACGT\n+\nIIII\n",
                "round trip for {name}"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn chunked_reading_preserves_every_byte_and_every_position() {
        let data = fastq(2000);
        let mut reader = Reader::from_bytes(data.clone().into_bytes()).unwrap();
        let mut collected = Vec::new();
        let mut buffer = Vec::new();
        let mut expected_offset = 0u64;
        let mut expected_line = 1u64;
        while let Some(chunk) = reader.next_chunk(&mut buffer, 4096).unwrap() {
            assert_eq!(chunk.offset, expected_offset, "byte offset");
            assert_eq!(chunk.line, expected_line, "line number");
            expected_offset += chunk.data.len() as u64;
            expected_line += chunk.data.iter().filter(|&&b| b == b'\n').count() as u64;
            collected.extend_from_slice(chunk.data);
        }
        assert_eq!(collected, data.as_bytes());
    }

    #[test]
    fn empty_input_yields_no_chunks() {
        let mut reader = Reader::from_bytes(Vec::new()).unwrap();
        let mut buffer = Vec::new();
        assert!(reader.next_chunk(&mut buffer, 4096).unwrap().is_none());
    }

    #[test]
    fn a_trailing_partial_record_is_still_delivered() {
        let mut data = fastq(10);
        data.push_str("@partial\nACGT\n");
        let mut reader = Reader::from_bytes(data.clone().into_bytes()).unwrap();
        let mut collected = Vec::new();
        let mut buffer = Vec::new();
        while let Some(chunk) = reader.next_chunk(&mut buffer, 4096).unwrap() {
            collected.extend_from_slice(chunk.data);
        }
        assert_eq!(collected, data.as_bytes());
    }

    #[test]
    fn writer_selects_format_from_the_extension() {
        for (name, format) in [
            ("a.fastq", Format::Plain),
            ("a.fastq.gz", Format::Gzip),
            ("a.fastq.bz2", Format::Bzip2),
            ("a.fastq.xz", Format::Xz),
            ("a.fastq.zst", Format::Zstd),
        ] {
            let path = temp_path(name);
            let _ = std::fs::remove_file(&path);
            let writer = Writer::create(path.to_str().unwrap(), Levels::DEFAULT).unwrap();
            assert_eq!(writer.format(), format, "format for {name}");
            drop(writer);
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let err = match Reader::open("/nonexistent/definitely-missing.fastq") {
            Ok(_) => panic!("opening a missing file must fail"),
            Err(e) => e,
        };
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn writer_and_parser_agree_on_both_layouts() {
        let path = temp_path("layouts.fastq");
        for (layout, expected) in [
            (Layout::Fastq, "@r1\nACGT\n+\nIIII\n"),
            (Layout::Fasta, ">r1\nACGT\n"),
        ] {
            let _ = std::fs::remove_file(&path);
            {
                let mut writer = Writer::create(path.to_str().unwrap(), Levels::DEFAULT).unwrap();
                writer
                    .write_record(b"@r1", b"ACGT", b"+", b"IIII", layout)
                    .unwrap();
                writer.finish().unwrap();
            }
            let raw = std::fs::read(&path).unwrap();
            assert_eq!(raw, expected.as_bytes(), "layout {layout:?}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parser_reads_what_the_reader_delivers() {
        let data = fastq(50);
        let mut reader = Reader::from_bytes(data.clone().into_bytes()).unwrap();
        let mut records = 0;
        let mut buffer = Vec::new();
        while let Some(chunk) = reader.next_chunk(&mut buffer, 8192).unwrap() {
            let mut parser = Parser::new(chunk.data, chunk.offset, chunk.line);
            while parser.next_record().unwrap().is_some() {
                records += 1;
            }
        }
        assert_eq!(records, 50);
    }
}
