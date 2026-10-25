//! Paired-end mate recovery.
//!
//! UMI-tools groups by read1, then recovers the mate of each selected read with
//! `TwoPassPairWriter`. We keep the same matching keys and consume each key once,
//! which is what makes the second pass equivalent.
//!
//! Large request sets are bounded: once the in-memory set reaches the budget the
//! keys are appended to a temporary file and the join is performed in chunks,
//! re-scanning the input for each chunk. The output is passed through the
//! coordinate sorter, so chunked recovery stays byte-identical; `--no-sort-output`
//! cannot be combined with a spilled request set.

use crate::error::{Error, Result};
use crate::io::bam::{self, Record};
use crate::io::spill::TempOutput;
use crate::io::RecordSink;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};

type MateKey = (Box<[u8]>, Option<i32>, i64);

fn key_for_read1(record: &Record) -> MateKey {
    (record.qname().into(), Some(record.mtid()), record.mpos())
}

fn key_for_read2(record: &Record) -> MateKey {
    (record.qname().into(), Some(record.tid()), record.pos())
}

fn write_key(writer: &mut impl IoWrite, key: &MateKey) -> std::io::Result<()> {
    writer.write_all(&(key.0.len() as u32).to_le_bytes())?;
    writer.write_all(&key.0)?;
    writer.write_all(&key.1.unwrap_or(-1).to_le_bytes())?;
    writer.write_all(&key.2.to_le_bytes())
}

fn read_key(reader: &mut impl IoRead) -> std::io::Result<Option<MateKey>> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let length = u32::from_le_bytes(length) as usize;
    let mut name = vec![0u8; length];
    reader.read_exact(&mut name)?;
    let mut tid = [0u8; 4];
    reader.read_exact(&mut tid)?;
    let tid = i32::from_le_bytes(tid);
    let mut pos = [0u8; 8];
    reader.read_exact(&mut pos)?;
    let pos = i64::from_le_bytes(pos);
    Ok(Some((
        name.into_boxed_slice(),
        if tid < 0 { None } else { Some(tid) },
        pos,
    )))
}

struct Spill {
    temp: TempOutput,
    writer: Option<BufWriter<File>>,
    keys: u64,
}

impl Spill {
    fn create(directory: &Path) -> Result<Spill> {
        let temp = TempOutput::create(directory, "umitools-mates")?;
        let file = File::options()
            .write(true)
            .truncate(true)
            .open(temp.path())
            .map_err(|e| Error::io(temp.path(), e))?;
        Ok(Spill {
            temp,
            writer: Some(BufWriter::new(file)),
            keys: 0,
        })
    }

    fn append(&mut self, keys: &HashSet<MateKey>) -> Result<()> {
        let writer = self.writer.as_mut().expect("spill writer open");
        for key in keys {
            write_key(writer, key)
                .map_err(|e| Error::Format(format!("cannot spill mate key: {e}")))?;
        }
        self.keys += keys.len() as u64;
        Ok(())
    }

    /// Close the writer and return a reader positioned at the start.
    fn reader(&mut self) -> Result<BufReader<File>> {
        if let Some(mut writer) = self.writer.take() {
            writer
                .flush()
                .map_err(|e| Error::Format(format!("cannot flush mate spill: {e}")))?;
        }
        let file = File::open(self.temp.path()).map_err(|e| Error::io(self.temp.path(), e))?;
        Ok(BufReader::new(file))
    }
}

pub struct MateRequests {
    wanted: HashSet<MateKey>,
    limit: usize,
    tmpdir: PathBuf,
    spill: Option<Spill>,
}

impl MateRequests {
    pub fn new(limit: usize, tmpdir: &Path) -> MateRequests {
        MateRequests {
            wanted: HashSet::new(),
            limit: limit.max(1),
            tmpdir: tmpdir.to_path_buf(),
            spill: None,
        }
    }

    pub fn len(&self) -> usize {
        self.wanted.len() + self.spill.as_ref().map_or(0, |spill| spill.keys as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.wanted.is_empty() && self.spill.is_none()
    }

    pub fn spilled(&self) -> bool {
        self.spill.is_some()
    }

    fn spill_full(&mut self) -> Result<()> {
        if self.wanted.len() < self.limit {
            return Ok(());
        }
        if self.spill.is_none() {
            self.spill = Some(Spill::create(&self.tmpdir)?);
        }
        let spill = self.spill.as_mut().expect("spill created");
        spill.append(&self.wanted)?;
        self.wanted.clear();
        Ok(())
    }

    /// Remember that the mate of this written read1 is still needed.
    pub fn request(&mut self, read1: &Record) -> Result<()> {
        if read1.is_unmapped() || read1.is_mate_unmapped() {
            return Ok(());
        }
        if self.wanted.insert(key_for_read1(read1)) {
            self.spill_full()?;
        }
        Ok(())
    }

    /// Consume a request if this read2 answers one.
    pub fn take(&mut self, read2: &Record) -> bool {
        if read2.is_unmapped() || read2.is_mate_unmapped() || !read2.is_last_in_template() {
            return false;
        }
        self.wanted.remove(&key_for_read2(read2))
    }
}

/// Second pass: scan the input again and append the mates that were requested
/// into the already open output writer, keeping the run to a single file.
pub fn write_mates(
    input: &Path,
    sink: &mut dyn RecordSink,
    requests: &mut MateRequests,
    reference: Option<&Path>,
    target_tid: Option<i32>,
) -> Result<u64> {
    if requests.spill.is_none() {
        let mut reader = bam::Reader::from_path(input).map_err(|e| {
            Error::Format(format!("cannot reopen {} for mates: {e}", input.display()))
        })?;
        crate::io::set_reference(&mut reader, reference)?;
        let mut written = 0u64;
        for record in reader.records() {
            let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
            if let Some(tid) = target_tid {
                if record.tid() != tid {
                    continue;
                }
            }
            if record.is_first_in_template() {
                continue;
            }
            if requests.take(&record) {
                sink.submit(record)?;
                written += 1;
            }
        }
        return Ok(written);
    }

    // Chunked path: move every remaining key to disk, then join one chunk at a
    // time so memory stays bounded by `limit` regardless of pair count.
    let spill = requests.spill.as_mut().expect("spilled");
    spill.append(&requests.wanted)?;
    requests.wanted.clear();
    let mut reader = spill.reader()?;
    let mut written = 0u64;
    loop {
        let mut chunk: HashSet<MateKey> = HashSet::new();
        for _ in 0..requests.limit {
            match read_key(&mut reader)
                .map_err(|e| Error::Format(format!("cannot read mate spill: {e}")))?
            {
                Some(key) => {
                    chunk.insert(key);
                }
                None => break,
            }
        }
        if chunk.is_empty() {
            break;
        }
        let mut reader = bam::Reader::from_path(input).map_err(|e| {
            Error::Format(format!("cannot reopen {} for mates: {e}", input.display()))
        })?;
        crate::io::set_reference(&mut reader, reference)?;
        for record in reader.records() {
            let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
            if let Some(tid) = target_tid {
                if record.tid() != tid {
                    continue;
                }
            }
            if record.is_first_in_template() || !record.is_last_in_template() {
                continue;
            }
            if chunk.remove(&key_for_read2(&record)) {
                sink.submit(record)?;
                written += 1;
                if chunk.is_empty() {
                    break;
                }
            }
        }
    }
    Ok(written)
}
