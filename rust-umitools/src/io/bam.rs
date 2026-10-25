//! A small BAM/SAM compatibility layer backed by noodles.
//!
//! The rest of `rust-umitools` was written against the limited rust-htslib
//! surface it needed. Keeping those method shapes here makes the migration
//! reviewable: the business logic still reads a record, asks for its CIGAR and
//! tags, and writes it to an alignment sink, while this module owns the
//! noodles-specific representation and BGZF lifecycle.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use noodles::bam;
use noodles::bgzf;
use noodles::cram;
use noodles::fasta;
use noodles::sam;
use noodles::sam::alignment::io::Write as AlignmentWrite;
use noodles::sam::alignment::record::data::field::Tag;
use noodles::sam::header::record::value::map::{self, program, Program};

/// Output container. CRAM is recognised but intentionally rejected until a
/// CRAM codec is added; the previous htslib build had a CRAM path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bam,
    Sam,
    Cram,
}

/// Compression level for the BGZF output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionLevel {
    Uncompressed,
    Level(u32),
    Fastest,
}

impl CompressionLevel {
    fn bgzf(self) -> bgzf::io::writer::CompressionLevel {
        let level = match self {
            Self::Uncompressed => 0,
            Self::Fastest => 1,
            Self::Level(level) => level,
        };
        bgzf::io::writer::CompressionLevel::new(level as u8).unwrap_or_default()
    }
}

/// A CIGAR operation with rust-htslib's public shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cigar {
    Match(u32),
    Ins(u32),
    Del(u32),
    RefSkip(u32),
    SoftClip(u32),
    HardClip(u32),
    Pad(u32),
    Equal(u32),
    Diff(u32),
}

/// Borrowed CIGAR view. Iteration does not allocate.
pub struct CigarView<'a>(&'a sam::alignment::record_buf::Cigar);

impl<'a> CigarView<'a> {
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Cigar> + 'a {
        self.0.as_ref().iter().copied().map(|op| match op.kind() {
            sam::alignment::record::cigar::op::Kind::Match => Cigar::Match(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::Insertion => Cigar::Ins(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::Deletion => Cigar::Del(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::Skip => Cigar::RefSkip(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::SoftClip => Cigar::SoftClip(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::HardClip => Cigar::HardClip(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::Pad => Cigar::Pad(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::SequenceMatch => Cigar::Equal(op.len() as u32),
            sam::alignment::record::cigar::op::Kind::SequenceMismatch => {
                Cigar::Diff(op.len() as u32)
            }
        })
    }

    pub fn len(&self) -> usize {
        self.0.as_ref().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_empty()
    }
}

/// Auxiliary tag value. Strings are owned so a record can be stored after the
/// parser has moved on, matching the lifetime-free rust-htslib API used here.
#[derive(Clone, Debug, PartialEq)]
pub enum Aux {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    Float(f32),
    Double(f64),
    String(String),
    Char(char),
}

#[derive(Clone, Debug)]
pub struct AuxError(String);

impl fmt::Display for AuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AuxError {}

impl Aux {
    fn from_value(value: &sam::alignment::record_buf::data::field::Value) -> Aux {
        use sam::alignment::record_buf::data::field::Value;
        match value {
            Value::Int8(v) => Aux::I8(*v),
            Value::UInt8(v) => Aux::U8(*v),
            Value::Int16(v) => Aux::I16(*v),
            Value::UInt16(v) => Aux::U16(*v),
            Value::Int32(v) => Aux::I32(*v),
            Value::UInt32(v) => Aux::U32(*v),
            Value::Float(v) => Aux::Float(*v),
            Value::String(v) | Value::Hex(v) => {
                Aux::String(String::from_utf8_lossy(v.as_ref()).into_owned())
            }
            Value::Character(v) => Aux::Char(*v as char),
            Value::Array(_) => Aux::String(String::new()),
        }
    }

    fn into_value(self) -> sam::alignment::record_buf::data::field::Value {
        use sam::alignment::record_buf::data::field::Value;
        match self {
            Aux::I8(v) => Value::Int8(v),
            Aux::U8(v) => Value::UInt8(v),
            Aux::I16(v) => Value::Int16(v),
            Aux::U16(v) => Value::UInt16(v),
            Aux::I32(v) => Value::Int32(v),
            Aux::U32(v) => Value::UInt32(v),
            Aux::Float(v) => Value::Float(v),
            Aux::Double(v) => Value::Float(v as f32),
            Aux::String(v) => Value::String(v.into()),
            Aux::Char(v) => Value::Character(v as u8),
        }
    }
}

/// An owned alignment record.
#[derive(Clone, Debug, Default)]
pub struct Record {
    inner: sam::alignment::RecordBuf,
}

impl Record {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn qname(&self) -> &[u8] {
        self.inner.name().map_or(b"", |name| name.as_ref())
    }

    pub fn seq_len(&self) -> usize {
        self.inner.sequence().len()
    }

    pub fn cigar(&self) -> CigarView<'_> {
        CigarView(self.inner.cigar())
    }

    pub fn tid(&self) -> i32 {
        self.inner
            .reference_sequence_id()
            .map_or(-1, |id| id as i32)
    }

    pub fn pos(&self) -> i64 {
        self.inner
            .alignment_start()
            .map_or(-1, |position| position.get() as i64 - 1)
    }

    pub fn mapq(&self) -> u8 {
        self.inner
            .mapping_quality()
            .map_or(0, |quality| quality.get())
    }

    pub fn insert_size(&self) -> i64 {
        i64::from(self.inner.template_length())
    }

    pub fn mtid(&self) -> i32 {
        self.inner
            .mate_reference_sequence_id()
            .map_or(-1, |id| id as i32)
    }

    pub fn mpos(&self) -> i64 {
        self.inner
            .mate_alignment_start()
            .map_or(-1, |position| position.get() as i64 - 1)
    }

    pub fn is_reverse(&self) -> bool {
        self.inner.flags().is_reverse_complemented()
    }

    pub fn is_unmapped(&self) -> bool {
        self.inner.flags().is_unmapped()
    }

    pub fn is_paired(&self) -> bool {
        self.inner.flags().is_segmented()
    }

    pub fn is_mate_unmapped(&self) -> bool {
        self.inner.flags().is_mate_unmapped()
    }

    pub fn is_first_in_template(&self) -> bool {
        self.inner.flags().is_first_segment()
    }

    pub fn is_last_in_template(&self) -> bool {
        self.inner.flags().is_last_segment()
    }

    pub fn aux(&self, tag: &[u8; 2]) -> Result<Aux, AuxError> {
        let tag = Tag::new(tag[0], tag[1]);
        self.inner
            .data()
            .get(&tag)
            .map(Aux::from_value)
            .ok_or_else(|| {
                AuxError(format!(
                    "tag '{}' is absent",
                    String::from_utf8_lossy(&tag_to_bytes(tag))
                ))
            })
    }

    pub fn push_aux(&mut self, tag: &[u8], value: Aux) -> Result<(), AuxError> {
        if tag.len() != 2 {
            return Err(AuxError("tag must have exactly two bytes".to_string()));
        }
        let tag = Tag::new(tag[0], tag[1]);
        self.inner.data_mut().insert(tag, value.into_value());
        Ok(())
    }

    pub fn remove_aux(&mut self, tag: &[u8; 2]) -> Option<Aux> {
        let tag = Tag::new(tag[0], tag[1]);
        self.inner
            .data_mut()
            .remove(&tag)
            .map(|(_, value)| Aux::from_value(&value))
    }
}

fn tag_to_bytes(tag: Tag) -> [u8; 2] {
    let bytes: [u8; 2] = tag.into();
    bytes
}

/// A SAM/BAM header wrapper.
#[derive(Clone, Debug, Default)]
pub struct Header {
    inner: sam::Header,
}

impl Header {
    pub fn from_template(header: &Header) -> Header {
        Header {
            inner: header.inner.clone(),
        }
    }

    pub fn target_count(&self) -> usize {
        self.inner.reference_sequences().len()
    }

    pub fn tid2name(&self, tid: usize) -> &[u8] {
        self.inner
            .reference_sequences()
            .get_index(tid)
            .map_or(b"", |(name, _)| name.as_ref())
    }

    /// Append an `@PG` record. The caller constructs the tag order.
    pub fn push_record(&mut self, record: &HeaderRecord) {
        if record.tag != *b"PG" {
            return;
        }
        let mut id = "rust-umitools";
        let mut builder = map::Map::<Program>::builder();
        for (tag, value) in &record.tags {
            match tag.as_slice() {
                b"ID" => id = value.as_str(),
                b"PN" => {
                    builder = builder.insert(program::tag::NAME, value.as_str());
                }
                b"VN" => {
                    builder = builder.insert(program::tag::VERSION, value.as_str());
                }
                b"CL" => {
                    builder = builder.insert(program::tag::COMMAND_LINE, value.as_str());
                }
                b"PP" => {
                    builder = builder.insert(program::tag::PREVIOUS_PROGRAM_ID, value.as_str());
                }
                b"DS" => {
                    builder = builder.insert(program::tag::DESCRIPTION, value.as_str());
                }
                _ => {}
            }
        }
        if let Ok(map) = builder.build() {
            // `Programs::add` automatically links the new program to every
            // current leaf via PP. htslib's HeaderRecord::push_record does not;
            // insert directly so an @PG added by this crate has no implicit PP.
            self.inner.programs_mut().as_mut().insert(id.into(), map);
        }
    }
}

/// A small `@PG` record builder, matching the subset used by this crate.
#[derive(Clone, Debug)]
pub struct HeaderRecord {
    tag: [u8; 2],
    tags: Vec<(Vec<u8>, String)>,
}

impl HeaderRecord {
    pub fn new(tag: &[u8]) -> HeaderRecord {
        let mut fixed = [0u8; 2];
        if tag.len() == 2 {
            fixed.copy_from_slice(tag);
        }
        HeaderRecord {
            tag: fixed,
            tags: Vec::new(),
        }
    }

    pub fn push_tag(&mut self, tag: &[u8], value: &str) {
        self.tags.push((tag.to_vec(), value.to_string()));
    }
}

enum ReaderInner {
    Bam(bam::io::Reader<bgzf::io::Reader<BufReader<File>>>),
    Sam(sam::io::Reader<BufReader<File>>),
    Cram(cram::io::Reader<File>),
}

/// An open BAM/SAM input.
pub struct Reader {
    path: PathBuf,
    reference: Option<PathBuf>,
    inner: ReaderInner,
    header: Header,
    cram_records: Option<std::vec::IntoIter<Record>>,
}

impl Reader {
    pub fn from_path(path: &Path) -> io::Result<Reader> {
        let mut probe = File::open(path)?;
        let mut magic = [0u8; 4];
        let read = probe.read(&mut magic)?;
        let is_cram = read == 4 && magic == *b"CRAM";
        let is_bam = !is_cram
            && read >= 2
            && (magic[..2] == [0x1f, 0x8b] || (read == 4 && magic == [b'B', b'A', b'M', 1]));
        drop(probe);
        if is_cram {
            let mut reader = cram::io::reader::Builder::default().build_from_path(path)?;
            let header = Header {
                inner: reader.read_header()?,
            };
            Ok(Reader {
                path: path.to_path_buf(),
                reference: None,
                inner: ReaderInner::Cram(reader),
                header,
                cram_records: None,
            })
        } else if is_bam {
            let file = File::open(path)?;
            let bgzf_reader = bgzf::io::Reader::new(BufReader::new(file));
            let mut reader = bam::io::Reader::from(bgzf_reader);
            let header = Header {
                inner: reader.read_header()?,
            };
            Ok(Reader {
                path: path.to_path_buf(),
                reference: None,
                inner: ReaderInner::Bam(reader),
                header,
                cram_records: None,
            })
        } else {
            let file = File::open(path)?;
            let mut reader = sam::io::Reader::new(BufReader::new(file));
            let header = Header {
                inner: reader.read_header()?,
            };
            Ok(Reader {
                path: path.to_path_buf(),
                reference: None,
                inner: ReaderInner::Sam(reader),
                header,
                cram_records: None,
            })
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn set_threads(&mut self, _threads: usize) -> io::Result<()> {
        // noodles' BGZF reader is single-threaded; keep the API and the
        // caller's thread accounting intact until a worker pool is wired.
        Ok(())
    }

    pub fn set_reference(&mut self, reference: &Path) -> io::Result<()> {
        self.reference = Some(reference.to_path_buf());
        if matches!(self.inner, ReaderInner::Cram(_)) {
            let repository = reference_repository(reference)?;
            let mut reader = cram::io::reader::Builder::default()
                .set_reference_sequence_repository(repository)
                .build_from_path(&self.path)?;
            self.header = Header {
                inner: reader.read_header()?,
            };
            self.inner = ReaderInner::Cram(reader);
            self.cram_records = None;
        }
        Ok(())
    }

    pub fn read(&mut self, record: &mut Record) -> Option<io::Result<()>> {
        if let ReaderInner::Cram(reader) = &mut self.inner {
            if self.cram_records.is_none() {
                let mut buffered = Vec::new();
                for result in reader.records(&self.header.inner) {
                    match result {
                        Ok(inner) => buffered.push(Record { inner }),
                        Err(error) => return Some(Err(error)),
                    }
                }
                self.cram_records = Some(buffered.into_iter());
            }
            return match self.cram_records.as_mut().and_then(Iterator::next) {
                Some(inner) => {
                    *record = inner;
                    Some(Ok(()))
                }
                None => None,
            };
        }
        let Self { inner, header, .. } = self;
        let result = match inner {
            ReaderInner::Bam(reader) => reader.read_record_buf(&header.inner, &mut record.inner),
            ReaderInner::Sam(reader) => reader.read_record_buf(&header.inner, &mut record.inner),
            ReaderInner::Cram(_) => unreachable!(),
        };
        match result {
            Ok(0) => None,
            Ok(_) => Some(Ok(())),
            Err(error) => Some(Err(error)),
        }
    }

    pub fn records(&mut self) -> Records<'_> {
        Records {
            reader: self,
            done: false,
        }
    }
}

pub struct Records<'a> {
    reader: &'a mut Reader,
    done: bool,
}

impl Iterator for Records<'_> {
    type Item = io::Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut record = Record::new();
        match self.reader.read(&mut record) {
            Some(Ok(())) => Some(Ok(record)),
            Some(Err(error)) => {
                self.done = true;
                Some(Err(error))
            }
            None => {
                self.done = true;
                None
            }
        }
    }
}

enum WriterInner {
    Bam(bam::io::Writer<bgzf::io::Writer<File>>),
    Sam(sam::io::Writer<File>),
    Cram(Box<cram::io::Writer<File>>),
}

/// A lazily opened BAM/SAM writer. Lazy opening lets the caller set the
/// compression level after construction, as the htslib API did.
pub struct Writer {
    path: PathBuf,
    header: Header,
    format: Format,
    reference: Option<PathBuf>,
    level: CompressionLevel,
    inner: Option<WriterInner>,
    finished: bool,
}

impl Writer {
    pub fn from_path(path: &Path, header: &Header, format: Format) -> io::Result<Writer> {
        Ok(Writer {
            path: path.to_path_buf(),
            header: header.clone(),
            format,
            reference: None,
            level: CompressionLevel::Level(6),
            inner: None,
            finished: false,
        })
    }

    pub fn set_threads(&mut self, _threads: usize) -> io::Result<()> {
        // The BGZF writer currently has no worker pool in this adapter.
        Ok(())
    }

    pub fn set_reference(&mut self, reference: &Path) -> io::Result<()> {
        self.reference = Some(reference.to_path_buf());
        Ok(())
    }

    pub fn set_compression_level(&mut self, level: CompressionLevel) -> io::Result<()> {
        self.level = level;
        Ok(())
    }

    fn ensure_open(&mut self) -> io::Result<()> {
        if self.inner.is_some() {
            return Ok(());
        }
        let file = File::create(&self.path)?;
        let inner = match self.format {
            Format::Bam => {
                let bgzf = bgzf::io::writer::Builder::default()
                    .set_compression_level(self.level.bgzf())
                    .build_from_writer(file);
                let mut writer = bam::io::Writer::from(bgzf);
                writer.write_header(&self.header.inner)?;
                WriterInner::Bam(writer)
            }
            Format::Sam => {
                let mut writer = sam::io::Writer::new(file);
                writer.write_header(&self.header.inner)?;
                WriterInner::Sam(writer)
            }
            Format::Cram => {
                let reference = self.reference.as_deref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "CRAM output needs --reference")
                })?;
                let repository = reference_repository(reference)?;
                let mut writer = cram::io::writer::Builder::default()
                    .set_reference_sequence_repository(repository)
                    .build_from_writer(file);
                writer.write_header(&self.header.inner)?;
                WriterInner::Cram(Box::new(writer))
            }
        };
        self.inner = Some(inner);
        Ok(())
    }

    pub fn write(&mut self, record: &Record) -> io::Result<()> {
        self.ensure_open()?;
        match self.inner.as_mut().expect("writer opened above") {
            WriterInner::Bam(writer) => {
                writer.write_alignment_record(&self.header.inner, &record.inner)
            }
            WriterInner::Sam(writer) => {
                writer.write_alignment_record(&self.header.inner, &record.inner)
            }
            WriterInner::Cram(writer) => {
                writer.write_alignment_record(&self.header.inner, &record.inner)
            }
        }
    }

    fn finish_inner(&mut self) -> io::Result<()> {
        // Explicit finish consumes the Writer, but the value is still dropped
        // afterwards. Do not reopen/truncate an already finished output.
        if self.finished {
            return Ok(());
        }
        // A writer that received zero records still needs a header and a
        // complete BGZF/SAM stream. Open it before taking the inner writer.
        self.ensure_open()?;
        let Some(inner) = self.inner.take() else {
            return Ok(());
        };
        self.finished = true;
        match inner {
            WriterInner::Bam(writer) => {
                writer.into_inner().finish()?;
            }
            WriterInner::Sam(mut writer) => {
                writer.get_mut().flush()?;
            }
            WriterInner::Cram(mut writer) => {
                writer.try_finish(&self.header.inner)?;
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.finish_inner()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.finish_inner();
    }
}

fn reference_repository(path: &Path) -> io::Result<fasta::Repository> {
    let indexed = fasta::io::indexed_reader::Builder::default().build_from_path(path)?;
    let adapter = fasta::repository::adapters::IndexedReader::new(indexed);
    Ok(fasta::Repository::new(adapter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_propagates_open_errors() {
        let writer = Writer::from_path(&std::env::temp_dir(), &Header::default(), Format::Bam)
            .expect("construct writer");
        assert!(writer.finish().is_err(), "finish must report an open error");
    }
}
