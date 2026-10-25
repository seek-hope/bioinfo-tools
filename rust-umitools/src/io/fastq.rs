//! FASTQ input/output for `extract` and `whitelist`.
//!
//! W1-2 moved the record model, the parser core and the compression layer into
//! `bioinfo-fastx`. What stays here is what is genuinely specific to this crate:
//!
//! * **The owned record.** [`FastqRecord`] owns its fields because the `extract`
//!   command edits a read in place (trimming a UMI pattern, masking qualities) and
//!   cannot borrow from a chunk it is about to overwrite.
//! * **Gzip selected by extension, not by content.** The shared crate sniffs the
//!   content, which is right for `rust-cutadapt`; `rust-umitools extract` has
//!   always opened a `.gz` input as gzip and a plain input as plain. The two only
//!   differ for a file whose name and content disagree, and this crate keeps its
//!   documented behaviour rather than silently changing it. See DR-1 in
//!   `docs/EVIDENCE-W1-2.md`.
//! * **The diagnostics.** They name the file and line (`path:line: message`),
//!   which is this crate's documented style and differs from cutadapt's
//!   `malformed FASTQ at line N (byte offset M)` form. Both are preserved.
//!
//! What is now shared: the four-line record structure, the `@`/`+` requirements,
//! the sequence/quality length check, and the writer byte layout.

use crate::error::{Error, Result};
use bioinfo_fastx::writer::write_record as shared_write_record;
use bioinfo_fastx::Layout;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

pub struct FastqRecord {
    pub name: String,
    pub comment: Option<String>,
    pub seq: Vec<u8>,
    pub qual: Vec<u8>,
}

impl FastqRecord {
    /// Serialise as FASTQ, through the shared writer.
    ///
    /// The header is reassembled from `name`/`comment` because this crate splits
    /// them on the first space. The shared writer adds the line terminators and
    /// the `+` separator, so the byte layout has exactly one owner.
    pub fn write_to(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let mut header = Vec::with_capacity(self.name.len() + 8);
        header.push(b'@');
        header.extend_from_slice(self.name.as_bytes());
        if let Some(comment) = &self.comment {
            header.push(b' ');
            header.extend_from_slice(comment.as_bytes());
        }
        shared_write_record(out, &header, &self.seq, b"+", &self.qual, Layout::Fastq)
    }
}

pub struct FastqReader {
    inner: Box<dyn BufRead>,
    line: u64,
    label: String,
}

impl FastqReader {
    /// `None` or `-` reads standard input; a `.gz` suffix selects gzip.
    ///
    /// Extension-based, deliberately, and documented above.
    pub fn open(path: Option<&Path>) -> Result<FastqReader> {
        let (inner, label): (Box<dyn BufRead>, String) = match path {
            None => (Box::new(BufReader::new(std::io::stdin())), "-".to_string()),
            Some(path) if path.as_os_str() == "-" => {
                (Box::new(BufReader::new(std::io::stdin())), "-".to_string())
            }
            Some(path) => {
                let file = File::open(path).map_err(|e| Error::io(path, e))?;
                let gzip = path.extension().and_then(|value| value.to_str()) == Some("gz");
                let reader: Box<dyn BufRead> = if gzip {
                    Box::new(BufReader::new(MultiGzDecoder::new(file)))
                } else {
                    Box::new(BufReader::new(file))
                };
                (reader, path.display().to_string())
            }
        };
        Ok(FastqReader {
            inner,
            line: 0,
            label,
        })
    }

    fn read_line(&mut self) -> Result<Option<String>> {
        let mut buffer = String::new();
        match self.inner.read_line(&mut buffer) {
            Ok(0) => Ok(None),
            Ok(_) => {
                self.line += 1;
                while buffer.ends_with('\n') || buffer.ends_with('\r') {
                    buffer.pop();
                }
                Ok(Some(buffer))
            }
            Err(e) => Err(Error::Format(format!("cannot read {}: {e}", self.label))),
        }
    }

    pub fn next_record(&mut self) -> Result<Option<FastqRecord>> {
        let Some(header) = self.read_line()? else {
            return Ok(None);
        };
        if header.is_empty() {
            return Ok(None);
        }
        // The record structure below is the same one `bioinfo-fastx` enforces; it
        // is checked here because this reader is line-oriented and string-typed,
        // and converting to the borrowed parser would mean buffering each record
        // twice for no gain. The *rules* are shared (documented in
        // bioinfo_fastx::parser) and the diagnostics stay this crate's own.
        if !header.starts_with('@') {
            return Err(Error::Format(format!(
                "{}:{}: FASTQ header must start with '@'",
                self.label, self.line
            )));
        }
        let rest = &header[1..];
        let (name, comment) = match rest.split_once(' ') {
            Some((name, comment)) => (name.to_string(), Some(comment.to_string())),
            None => (rest.to_string(), None),
        };
        let seq = self.read_line()?.ok_or_else(|| {
            Error::Format(format!(
                "{}:{}: truncated FASTQ record",
                self.label, self.line
            ))
        })?;
        let plus = self.read_line()?.ok_or_else(|| {
            Error::Format(format!(
                "{}:{}: truncated FASTQ record",
                self.label, self.line
            ))
        })?;
        if !plus.starts_with('+') {
            return Err(Error::Format(format!(
                "{}:{}: FASTQ separator must start with '+'",
                self.label, self.line
            )));
        }
        let qual = self.read_line()?.ok_or_else(|| {
            Error::Format(format!(
                "{}:{}: truncated FASTQ record",
                self.label, self.line
            ))
        })?;
        if seq.len() != qual.len() {
            return Err(Error::Format(format!(
                "{}:{}: sequence and quality lengths differ ({} vs {})",
                self.label,
                self.line - 1,
                seq.len(),
                qual.len()
            )));
        }
        Ok(Some(FastqRecord {
            name,
            comment,
            seq: seq.into_bytes(),
            qual: qual.into_bytes(),
        }))
    }
}

#[allow(clippy::large_enum_variant)] // boxing the encoder would obscure the small writer API
pub enum FastqWriter {
    Plain(BufWriter<Box<dyn Write>>),
    Gzip(GzEncoder<BufWriter<Box<dyn Write>>>),
}

impl FastqWriter {
    /// `None` or `-` writes standard output. `gzip` is passed explicitly so a
    /// temporary staging path does not lose the destination's compression.
    pub fn create(path: Option<&Path>, gzip: bool) -> Result<FastqWriter> {
        let sink: Box<dyn Write> = match path {
            None => Box::new(std::io::stdout()),
            Some(path) if path.as_os_str() == "-" => Box::new(std::io::stdout()),
            Some(path) => Box::new(File::create(path).map_err(|e| Error::io(path, e))?),
        };
        if gzip {
            Ok(FastqWriter::Gzip(GzEncoder::new(
                BufWriter::new(sink),
                Compression::default(),
            )))
        } else {
            Ok(FastqWriter::Plain(BufWriter::new(sink)))
        }
    }

    pub fn write_record(&mut self, record: &FastqRecord) -> Result<()> {
        match self {
            FastqWriter::Plain(writer) => record
                .write_to(writer)
                .map_err(|e| Error::Format(format!("cannot write FASTQ: {e}"))),
            FastqWriter::Gzip(writer) => record
                .write_to(writer)
                .map_err(|e| Error::Format(format!("cannot write FASTQ: {e}"))),
        }
    }

    pub fn finish(self) -> Result<()> {
        match self {
            FastqWriter::Plain(mut writer) => writer
                .flush()
                .map_err(|e| Error::Format(format!("cannot flush FASTQ: {e}"))),
            FastqWriter::Gzip(writer) => writer
                .finish()
                .map(|_| ())
                .map_err(|e| Error::Format(format!("cannot finish gzip output: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "umitools-fastq-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    /// `unwrap_err` needs `T: Debug`; `FastqRecord` deliberately has no `Debug`
    /// (it holds owned buffers and is never logged), so match instead.
    fn expect_err(reader: &mut FastqReader) -> Error {
        match reader.next_record() {
            Ok(_) => panic!("expected a parse error"),
            Err(e) => e,
        }
    }

    fn read_all(path: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        File::open(path).unwrap().read_to_end(&mut out).unwrap();
        out
    }

    fn gunzip(path: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        MultiGzDecoder::new(File::open(path).unwrap())
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    /// The byte layout this crate wrote before the migration, pinned literally.
    #[test]
    fn write_to_produces_the_pre_migration_bytes() {
        let record = FastqRecord {
            name: "r1".to_string(),
            comment: Some("extra".to_string()),
            seq: b"ACGT".to_vec(),
            qual: b"IIII".to_vec(),
        };
        let mut out = Vec::new();
        record.write_to(&mut out).unwrap();
        assert_eq!(out, b"@r1 extra\nACGT\n+\nIIII\n");
    }

    #[test]
    fn write_to_without_a_comment() {
        let record = FastqRecord {
            name: "r1".to_string(),
            comment: None,
            seq: b"ACGT".to_vec(),
            qual: b"IIII".to_vec(),
        };
        let mut out = Vec::new();
        record.write_to(&mut out).unwrap();
        assert_eq!(out, b"@r1\nACGT\n+\nIIII\n");
    }

    #[test]
    fn write_to_keeps_an_empty_record() {
        let record = FastqRecord {
            name: "r1".to_string(),
            comment: None,
            seq: Vec::new(),
            qual: Vec::new(),
        };
        let mut out = Vec::new();
        record.write_to(&mut out).unwrap();
        assert_eq!(out, b"@r1\n\n+\n\n");
    }

    #[test]
    fn reads_a_plain_file() {
        let path = temp("plain.fastq");
        std::fs::write(&path, b"@r1 extra\nACGT\n+\nIIII\n@r2\nTTTT\n+\nJJJJ\n").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let first = reader.next_record().unwrap().unwrap();
        assert_eq!(first.name, "r1");
        assert_eq!(first.comment.as_deref(), Some("extra"));
        assert_eq!(first.seq, b"ACGT");
        assert_eq!(first.qual, b"IIII");
        let second = reader.next_record().unwrap().unwrap();
        assert_eq!(second.name, "r2");
        assert!(reader.next_record().unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_a_gzip_file() {
        let path = temp("in.fastq.gz");
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"@r1\nACGT\n+\nIIII\n").unwrap();
        std::fs::write(&path, enc.finish().unwrap()).unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        assert_eq!(reader.next_record().unwrap().unwrap().seq, b"ACGT");
        let _ = std::fs::remove_file(&path);
    }

    // Extension-based gzip selection is this crate's documented behaviour: a
    // gzip stream in a file not named `.gz` is read as plain and fails. This pins
    // the difference from `bioinfo-fastx`'s content sniffing (DR-1).
    #[test]
    fn gzip_detection_is_by_extension_here() {
        let path = temp("disguised.fastq");
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"@r1\nACGT\n+\nIIII\n").unwrap();
        std::fs::write(&path, enc.finish().unwrap()).unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let result = reader.next_record();
        assert!(
            result.is_err(),
            "a gzip stream in a .fastq file must be rejected, not sniffed"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_bad_header_with_this_crates_diagnostic() {
        let path = temp("bad.fastq");
        std::fs::write(&path, b"r1\nACGT\n+\nIIII\n").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let err = expect_err(&mut reader);
        let text = err.to_string();
        assert!(text.contains("FASTQ header must start with '@'"), "{text}");
        assert!(text.contains(":1:"), "line number in {text}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_length_mismatch_with_this_crates_diagnostic() {
        let path = temp("mismatch.fastq");
        std::fs::write(&path, b"@r1\nACGTACGT\n+\nIII\n").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let err = expect_err(&mut reader);
        let text = err.to_string();
        assert!(
            text.contains("sequence and quality lengths differ (8 vs 3)"),
            "{text}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_missing_separator() {
        let path = temp("nosep.fastq");
        std::fs::write(&path, b"@r1\nACGT\n-\nIIII\n").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let err = expect_err(&mut reader);
        assert!(err.to_string().contains("separator"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_truncated_record() {
        let path = temp("trunc.fastq");
        std::fs::write(&path, b"@r1\nACGT\n+\n").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let err = expect_err(&mut reader);
        assert!(err.to_string().contains("truncated"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_input_yields_no_records() {
        let path = temp("empty.fastq");
        std::fs::write(&path, b"").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        assert!(reader.next_record().unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writer_round_trips_plain_and_gzip() {
        for gzip in [false, true] {
            let path = temp(if gzip { "o.fastq.gz" } else { "o.fastq" });
            let _ = std::fs::remove_file(&path);
            {
                let mut writer = FastqWriter::create(Some(&path), gzip).unwrap();
                let record = FastqRecord {
                    name: "r1".to_string(),
                    comment: None,
                    seq: b"ACGT".to_vec(),
                    qual: b"IIII".to_vec(),
                };
                writer.write_record(&record).unwrap();
                writer.finish().unwrap();
            }
            let bytes = if gzip { gunzip(&path) } else { read_all(&path) };
            assert_eq!(bytes, b"@r1\nACGT\n+\nIIII\n", "gzip={gzip}");
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn writer_and_reader_agree() {
        let path = temp("rt.fastq");
        let _ = std::fs::remove_file(&path);
        {
            let mut writer = FastqWriter::create(Some(&path), false).unwrap();
            for i in 0..10 {
                let record = FastqRecord {
                    name: format!("r{i}"),
                    comment: Some("c".to_string()),
                    seq: b"ACGTACGT".to_vec(),
                    qual: b"IIIIIIII".to_vec(),
                };
                writer.write_record(&record).unwrap();
            }
            writer.finish().unwrap();
        }
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let mut count = 0;
        while let Some(record) = reader.next_record().unwrap() {
            assert_eq!(record.comment.as_deref(), Some("c"));
            count += 1;
        }
        assert_eq!(count, 10);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let err = match FastqReader::open(Some(Path::new("/nonexistent/x.fastq"))) {
            Ok(_) => panic!("must fail"),
            Err(e) => e,
        };
        assert_eq!(err.exit_code(), 4, "I/O failures are class 4");
    }

    #[test]
    fn the_reader_tolerates_a_trailing_crlf() {
        let path = temp("crlf.fastq");
        std::fs::write(&path, b"@r1\r\nACGT\r\n+\r\nIIII\r\n").unwrap();
        let mut reader = FastqReader::open(Some(&path)).unwrap();
        let record = reader.next_record().unwrap().unwrap();
        assert_eq!(record.seq, b"ACGT");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn seek_is_untouched_by_reading() {
        // Guards the staging path: `extract` writes to a temp file and commits it,
        // so the reader must not hold the destination open.
        let path = temp("seek.fastq");
        std::fs::write(&path, b"@r1\nACGT\n+\nIIII\n").unwrap();
        {
            let mut reader = FastqReader::open(Some(&path)).unwrap();
            reader.next_record().unwrap();
        }
        let mut file = File::open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut head = [0u8; 3];
        file.read_exact(&mut head).unwrap();
        assert_eq!(&head, b"@r1");
        let _ = std::fs::remove_file(&path);
    }
}
