//! A uniform reader over SAM and BAM.
//!
//! All alignment subcommands need the same two things: the header, and a stream of
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
use std::io::{self, BufReader, Read, Write};
use std::num::NonZero;
use std::path::Path;

use noodles::bam;
use noodles::bgzf;
use noodles::sam;
use noodles::sam::alignment::io::Write as AlignmentWrite;

use crate::bgzf_source::BgzfSource;
use crate::error::{Error, Result};
use crate::format::{detect_reader, Format, GZIP_MAGIC};

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
        Self::open_with_regular_check(path, false).map(|(reader, _)| reader)
    }

    fn open_with_regular_check(path: &Path, inspect_regular: bool) -> Result<(Self, bool)> {
        if path.as_os_str() == "-" {
            return Self::open_stdin().map(|reader| (reader, false));
        }
        let source: Box<dyn Read + Send> = Box::new(
            File::open(path).map_err(|error| Error::io(format!("{}: {error}", path.display())))?,
        );
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
                // offset zero.
                let file = File::open(path)
                    .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
                let reader =
                    sam::io::Reader::new(BufReader::new(Box::new(file) as Box<dyn Read + Send>));
                Ok((AlignmentReader::Sam(reader), false))
            }
            Format::Bam => {
                let file = File::open(path)
                    .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
                let regular = inspect_regular && file.metadata().is_ok_and(|m| m.is_file());
                let bgzf_reader = bgzf::io::Reader::new(BufReader::new(Box::new(BgzfSource::new(
                    file,
                ))
                    as Box<dyn Read + Send>));
                let reader = bam::io::Reader::from(bgzf_reader);
                Ok((AlignmentReader::Bam(reader), regular))
            }
            Format::Cram => Err(Error::usage(
                "CRAM is not supported by this build; convert it with `samtools view -b` first",
            )),
        }
    }

    /// Open standard input without discarding the bytes used for detection.
    fn open_stdin() -> Result<AlignmentReader> {
        let mut source = io::stdin();
        let mut head = [0u8; 4];
        let mut read = 0;
        while read < head.len() {
            match source.read(&mut head[read..]) {
                Ok(0) => break,
                Ok(count) => read += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(Error::io(format!("cannot read stdin: {error}"))),
            }
        }
        if read == 0 {
            return Err(Error::input("empty input: not a SAM or BAM file"));
        }
        let prefix = head[..read].to_vec();
        // BGZF starts with gzip magic. The BAM parser validates the decompressed
        // BAM magic, so no second consuming detection pass is needed here.
        let format = if prefix.starts_with(&GZIP_MAGIC) {
            Format::Bam
        } else {
            detect_reader(&mut io::Cursor::new(prefix.clone()))?
        };
        let chained = io::Cursor::new(prefix).chain(source);
        let source: Box<dyn Read + Send> = if format == Format::Bam {
            Box::new(BgzfSource::new(chained))
        } else {
            Box::new(chained)
        };
        let buffered = BufReader::new(source);
        match format {
            Format::Sam => Ok(AlignmentReader::Sam(sam::io::Reader::new(buffered))),
            Format::Bam => Ok(AlignmentReader::Bam(bam::io::Reader::from(
                bgzf::io::Reader::new(buffered),
            ))),
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

/// Sort-only input scheduling; public record decoding is identical in both arms.
///
/// Only a regular BAM file's actual open handle can use asynchronous reads.
/// Stdin, pipes and SAM retain serial reads because joining a blocked input
/// thread during an earlier output/record error could otherwise block the caller.
pub(crate) enum SortReader {
    Serial(AlignmentReader),
    Parallel(bam::io::Reader<bgzf::io::MultithreadedReader<BufReader<Box<dyn Read + Send>>>>),
}

impl SortReader {
    pub(crate) fn open(path: &Path, threads: usize) -> Result<Self> {
        let (reader, regular) = AlignmentReader::open_with_regular_check(path, threads > 1)?;
        match (reader, regular, NonZero::new(threads.saturating_sub(1))) {
            (AlignmentReader::Bam(reader), true, Some(workers)) => {
                // No header/record has been consumed from this reopened handle.
                let input = reader.into_inner().into_inner();
                let parallel = bgzf::io::MultithreadedReader::with_worker_count(workers, input);
                Ok(Self::Parallel(bam::io::Reader::from(parallel)))
            }
            (reader, _, _) => Ok(Self::Serial(reader)),
        }
    }

    pub(crate) fn read_header(&mut self) -> Result<sam::Header> {
        match self {
            Self::Serial(reader) => reader.read_header(),
            Self::Parallel(reader) => reader
                .read_header()
                .map_err(|error| Error::runtime(format!("cannot read BAM header: {error}"))),
        }
    }

    pub(crate) fn read_record(
        &mut self,
        header: &sam::Header,
        record: &mut Record,
    ) -> Result<usize> {
        match self {
            Self::Serial(reader) => reader.read_record(header, record),
            Self::Parallel(reader) => reader
                .read_record_buf(header, record)
                .map_err(|error| Error::runtime(format!("cannot read BAM record: {error}"))),
        }
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
            .write_alignment_record(header, &crate::encoder_record::EncoderRecord(record))
            .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))
    }

    /// Write a complete, already validated uncompressed BAM frame from an
    /// internal sort encoder or run, including its four-byte block-size prefix.
    pub(crate) fn write_encoded_record(&mut self, frame: &[u8]) -> Result<()> {
        self.inner
            .get_mut()
            .write_all(frame)
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

    fn reader_result(mut reader: SortReader) -> (Result<sam::Header>, Vec<Record>, Option<Error>) {
        let header = reader.read_header();
        let mut records = Vec::new();
        let mut error = None;
        if let Ok(header) = &header {
            let mut record = Record::default();
            loop {
                match reader.read_record(header, &mut record) {
                    Ok(0) => break,
                    Ok(_) => records.push(record.clone()),
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
        }
        (header, records, error)
    }

    fn encoded_test_records() -> (Vec<u8>, usize) {
        let header = sam::Header::default();
        let mut writer = bam::io::Writer::from(Vec::new());
        writer.write_header(&header).unwrap();
        let first_record = writer.get_ref().len();
        for index in 0..1024 {
            let length = if index % 127 == 0 { 70000 } else { 150 };
            let record = Record::builder()
                .set_name(format!("r{index:04}"))
                .set_sequence(vec![b'A'; length].into())
                .set_quality_scores(vec![30; length].into())
                .build();
            writer
                .write_alignment_record(&header, &crate::encoder_record::EncoderRecord(&record))
                .unwrap();
        }
        (writer.into_inner(), first_record)
    }

    fn compress_blocks(raw: &[u8]) -> Vec<Vec<u8>> {
        raw.chunks(32768)
            .map(|chunk| {
                let mut writer = bgzf::io::Writer::new(Vec::new());
                writer.write_all(chunk).unwrap();
                let mut bytes = writer.finish().unwrap();
                bytes.truncate(bytes.len() - 28);
                bytes
            })
            .collect()
    }

    #[test]
    fn parallel_sort_reader_preserves_records_and_ordered_decoder_errors() {
        let (raw, first) = encoded_test_records();
        let blocks = compress_blocks(&raw);
        assert!(blocks.len() > 4);
        let eof = bgzf::io::Writer::new(Vec::new()).finish().unwrap();
        let mut cases = vec![("valid", [blocks.concat(), eof.clone()].concat())];
        for length in 1..18 {
            cases.push((
                "partial-final-frame-header",
                [blocks.concat(), eof[..length].to_vec()].concat(),
            ));
        }
        let mut interior_empty = blocks.clone();
        interior_empty.insert(2, eof);
        cases.push(("interior-empty", interior_empty.concat()));
        let mut faults = blocks.clone();
        let crc = faults[1].len() - 8;
        faults[1][crc] ^= 1;
        faults[2][0] ^= 1;
        cases.push(("earlier-crc-before-later-header", faults.concat()));
        let mut faults = blocks.clone();
        let crc = faults[1].len() - 8;
        faults[1][crc] ^= 1;
        faults[2][16..18].copy_from_slice(&0u16.to_le_bytes());
        cases.push(("earlier-crc-before-later-frame-read-error", faults.concat()));
        let mut faults = blocks.clone();
        let isize = faults[2].len() - 4;
        faults[2][isize..].copy_from_slice(&65537u32.to_le_bytes());
        cases.push(("oversized-isize", faults.concat()));
        cases.push((
            "truncated-body",
            [
                blocks[..2].concat(),
                blocks[2][..blocks[2].len() - 7].to_vec(),
            ]
            .concat(),
        ));
        cases.push((
            "partial-frame-header",
            [blocks[..2].concat(), blocks[2][..17].to_vec()].concat(),
        ));
        let mut bad_header = raw.clone();
        bad_header[0] = 0;
        let mut bad_header = compress_blocks(&bad_header);
        let crc = bad_header[1].len() - 8;
        bad_header[1][crc] ^= 1;
        cases.push(("bam-header-before-later-crc", bad_header.concat()));
        let mut bad_record = raw;
        // The first large record spans blocks. Append an invalid auxiliary type
        // inside its frame, followed by a later independent BGZF failure.
        let length = u32::from_le_bytes(bad_record[first..first + 4].try_into().unwrap());
        let end = first + 4 + length as usize;
        bad_record.splice(end..end, *b"XX?");
        bad_record[first..first + 4].copy_from_slice(&(length + 3).to_le_bytes());
        let mut bad_record = compress_blocks(&bad_record);
        let last = bad_record.len() - 1;
        let crc = bad_record[last].len() - 8;
        bad_record[last][crc] ^= 1;
        cases.push(("bam-record-before-later-crc", bad_record.concat()));

        for (name, data) in cases {
            let input = || {
                BufReader::new(Box::new(BgzfSource::new(io::Cursor::new(data.clone())))
                    as Box<dyn Read + Send>)
            };
            let expected = reader_result(SortReader::Serial(AlignmentReader::Bam(
                bam::io::Reader::from(bgzf::io::Reader::new(input())),
            )));
            if name == "partial-final-frame-header" {
                assert!(expected
                    .2
                    .as_ref()
                    .is_some_and(|error| error.message().contains("truncated BGZF header")));
            }
            for workers in [1, 3] {
                let actual = reader_result(SortReader::Parallel(bam::io::Reader::from(
                    bgzf::io::MultithreadedReader::with_worker_count(
                        NonZero::new(workers).unwrap(),
                        input(),
                    ),
                )));
                assert_eq!(actual, expected, "{name}, workers={workers}");
            }
        }
    }

    #[test]
    fn only_regular_bam_files_with_multiple_sort_threads_select_parallel_input() {
        let directory =
            std::env::temp_dir().join(format!("rust-bam-sort-reader-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("input");
        let (raw, _) = encoded_test_records();
        std::fs::write(&path, compress_blocks(&raw).concat()).unwrap();
        for threads in [0, 1] {
            assert!(matches!(
                SortReader::open(&path, threads).unwrap(),
                SortReader::Serial(_)
            ));
        }
        assert!(matches!(
            SortReader::open(&path, 4).unwrap(),
            SortReader::Parallel(_)
        ));
        std::fs::write(&path, b"@HD\tVN:1.6\nq\t4\t*\t0\t0\t*\t*\t0\t0\tA\tI\n").unwrap();
        assert!(matches!(
            SortReader::open(&path, 4).unwrap(),
            SortReader::Serial(_)
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn records_alias_is_the_sam_buffer_type() {
        // Compile-time assertion that the public alias is what the writers take.
        fn takes(_: &Record) {}
        let record = Record::default();
        takes(&record);
    }
}
