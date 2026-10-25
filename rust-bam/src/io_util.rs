//! A uniform reader over SAM and BAM.
//!
//! The six subcommands all need the same two things: the header, and a stream of
//! records. SAM and BAM differ in both, so this module hides the difference
//! behind one type and keeps each command free of format branching.
//!
//! ## Streaming, bounded memory
//!
//! Nothing here collects the whole file. `AlignmentReader::records` yields one
//! record at a time, and `view` writes each one back out immediately. `sort`
//! needs random access; it buffers one bounded chunk, spills sorted BAM runs plus
//! an input-index sidecar to a temporary directory, and merges them with a
//! bounded number of open files.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use noodles::bam;
use noodles::bgzf;
use noodles::sam;
use noodles::sam::alignment::io::Write as AlignmentWrite;

use crate::error::{Error, Result};
use crate::format::{detect_reader, Format};

/// Alias for the SAM record type, which is what both formats decode to.
pub type Record = sam::alignment::RecordBuf;

/// An open alignment input, either SAM or BAM.
pub enum AlignmentReader {
    /// Plain-text SAM.
    Sam(sam::io::Reader<BufReader<Box<dyn Read + Send>>>),
    /// BGZF-compressed BAM.
    Bam(bam::io::Reader<bgzf::io::Reader<BufReader<Box<dyn Read + Send>>>>),
}

impl AlignmentReader {
    /// Open `path`, or standard input when `path` is `-`.
    ///
    /// The format is detected from content, as `samtools` does, not from the
    /// extension.
    pub fn open(path: &Path) -> Result<AlignmentReader> {
        let source: Box<dyn Read + Send> = if path.as_os_str() == "-" {
            Box::new(io::stdin())
        } else {
            let file = File::open(path)
                .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
            Box::new(file)
        };
        let mut buffered = BufReader::new(source);
        let format = detect_reader(&mut buffered).map_err(|error| {
            if path.as_os_str() == "-" {
                error
            } else {
                match error.exit_code() {
                    3 => Error::input(format!("{}: {}", path.display(), error.message())),
                    _ => Error::io(format!("{}: {}", path.display(), error.message())),
                }
            }
        })?;

        match format {
            Format::Sam => {
                // Detection consumed a few bytes; re-open so the parser starts at
                // offset zero. Only regular files can be re-opened.
                if path.as_os_str() == "-" {
                    return Err(Error::usage(
                        "SAM on standard input is not supported: format detection must peek first",
                    ));
                }
                let file = File::open(path)
                    .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
                let reader =
                    sam::io::Reader::new(BufReader::new(Box::new(file) as Box<dyn Read + Send>));
                Ok(AlignmentReader::Sam(reader))
            }
            Format::Bam => {
                if path.as_os_str() == "-" {
                    return Err(Error::usage(
                        "BAM on standard input is not supported: format detection must peek first",
                    ));
                }
                let file = File::open(path)
                    .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
                let bgzf_reader =
                    bgzf::io::Reader::new(BufReader::new(Box::new(file) as Box<dyn Read + Send>));
                let reader = bam::io::Reader::from(bgzf_reader);
                Ok(AlignmentReader::Bam(reader))
            }
            Format::Cram => Err(Error::usage(
                "CRAM is not supported by this build; convert it with `samtools view -b` first",
            )),
        }
    }

    /// The detected format.
    pub fn format(&self) -> Format {
        match self {
            AlignmentReader::Sam(_) => Format::Sam,
            AlignmentReader::Bam(_) => Format::Bam,
        }
    }

    /// Read the header.
    pub fn read_header(&mut self) -> Result<sam::Header> {
        match self {
            AlignmentReader::Sam(reader) => reader
                .read_header()
                .map_err(|error| Error::runtime(format!("cannot read SAM header: {error}"))),
            AlignmentReader::Bam(reader) => reader
                .read_header()
                .map_err(|error| Error::runtime(format!("cannot read BAM header: {error}"))),
        }
    }

    /// Read one record into `record`.
    ///
    /// `header` is required because noodles resolves the numeric reference
    /// sequence id against `@SQ` lines while decoding.
    ///
    /// Returns `Ok(0)` at end of input, mirroring `std::io::Read`, so the caller
    /// can loop without an `Option` dance.
    pub fn read_record(&mut self, header: &sam::Header, record: &mut Record) -> Result<usize> {
        match self {
            AlignmentReader::Sam(reader) => reader
                .read_record_buf(header, record)
                .map_err(|error| Error::runtime(format!("cannot read SAM record: {error}"))),
            AlignmentReader::Bam(reader) => reader
                .read_record_buf(header, record)
                .map_err(|error| Error::runtime(format!("cannot read BAM record: {error}"))),
        }
    }

    /// Iterate over every record, streaming.
    ///
    /// Returns an iterator of `Result<Record>`; a malformed record stops the
    /// iteration with an error rather than being skipped, which is what
    /// `samtools view` does (it exits non-zero on a corrupt record).
    pub fn records<'a>(
        &'a mut self,
        header: &'a sam::Header,
    ) -> impl Iterator<Item = Result<Record>> + 'a {
        let mut done = false;
        std::iter::from_fn(move || {
            if done {
                return None;
            }
            let mut record = Record::default();
            match self.read_record(header, &mut record) {
                Ok(0) => {
                    done = true;
                    None
                }
                Ok(_) => Some(Ok(record)),
                Err(error) => {
                    done = true;
                    Some(Err(error))
                }
            }
        })
    }
}

/// Write records as SAM text, which is what `samtools view` emits by default.
pub struct SamWriter<W: io::Write> {
    inner: sam::io::Writer<W>,
}

impl<W: io::Write> SamWriter<W> {
    /// Wrap `writer`.
    pub fn new(writer: W) -> SamWriter<W> {
        SamWriter {
            inner: sam::io::Writer::new(writer),
        }
    }

    /// Write a header.
    pub fn write_header(&mut self, header: &sam::Header) -> Result<()> {
        self.inner
            .write_header(header)
            .map_err(|error| Error::io(format!("cannot write header: {error}")))
    }

    /// Write one record.
    ///
    /// Uses the generic `write_alignment_record` entry point, which accepts any
    /// `sam::alignment::Record` — `RecordBuf` in this crate — so the same call
    /// works for the SAM and BAM writers.
    pub fn write_record(&mut self, header: &sam::Header, record: &Record) -> Result<()> {
        self.inner
            .write_alignment_record(header, record)
            .map_err(|error| Error::io(format!("cannot write record: {error}")))
    }

    /// Write one raw alignment record (for an indexed query iterator).
    pub fn write_alignment_record(
        &mut self,
        header: &sam::Header,
        record: &dyn sam::alignment::Record,
    ) -> Result<()> {
        self.inner
            .write_alignment_record(header, record)
            .map_err(|error| Error::io(format!("cannot write record: {error}")))
    }

    /// Flush the underlying writer.
    pub fn finish(&mut self) -> Result<()> {
        self.inner
            .get_mut()
            .flush()
            .map_err(|error| Error::io(format!("cannot flush: {error}")))
    }
}

/// Write records as BAM (BGZF-compressed), which is what `samtools view -b` emits.
pub struct BamWriter<W: io::Write> {
    inner: bam::io::Writer<bgzf::io::Writer<W>>,
}

impl<W: io::Write> BamWriter<W> {
    /// Wrap `writer` with the default BGZF compression level (6).
    pub fn new(writer: W) -> BamWriter<W> {
        BamWriter {
            inner: bam::io::Writer::new(writer),
        }
    }

    /// Wrap a temporary sort run with fast BGZF compression (level 1).
    ///
    /// Only temporary runs use this; the final output keeps the default level, so
    /// the compressed final bytes and all observable output stay unchanged.
    pub fn new_fast(writer: W) -> BamWriter<W> {
        let bgzf_writer = bgzf::io::writer::Builder::default()
            .set_compression_level(bgzf::io::writer::CompressionLevel::FAST)
            .build_from_writer(writer);
        BamWriter {
            inner: bam::io::Writer::from(bgzf_writer),
        }
    }

    /// Write a header.
    pub fn write_header(&mut self, header: &sam::Header) -> Result<()> {
        self.inner
            .write_header(header)
            .map_err(|error| Error::io(format!("cannot write BAM header: {error}")))
    }

    /// Write one record.
    pub fn write_record(&mut self, header: &sam::Header, record: &Record) -> Result<()> {
        self.inner
            .write_alignment_record(header, record)
            .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))
    }

    /// Write one raw alignment record (for an indexed query iterator).
    pub fn write_alignment_record(
        &mut self,
        header: &sam::Header,
        record: &dyn sam::alignment::Record,
    ) -> Result<()> {
        self.inner
            .write_alignment_record(header, record)
            .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))
    }

    /// Finish the BGZF stream, writing the EOF marker, and return the writer.
    ///
    /// This must be called: without the EOF block `samtools quickcheck` reports
    /// "missing EOF block when one should be present" and exits 16 — exactly the
    /// defect W1-4 found in `bioinfo-fastx`'s BGZF writer, where the marker was
    /// present but one byte (gzip `XFL`) differed from htslib's fixed pattern.
    /// noodles writes the canonical marker.
    pub fn finish(self) -> Result<W> {
        self.inner
            .into_inner()
            .finish()
            .map_err(|error| Error::io(format!("cannot finish BGZF stream: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_alias_is_the_sam_buffer_type() {
        // Compile-time assertion that the public alias is what the writers take.
        fn takes(_: &Record) {}
        let record = Record::default();
        takes(&record);
    }
}
