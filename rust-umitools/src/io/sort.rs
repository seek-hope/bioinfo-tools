//! Bounded-memory coordinate sorting for the command outputs.
//!
//! Two entry points share one merge implementation:
//!
//! * [`SortingWriter`] consumes records directly from the pipeline, so the
//!   output is produced in a single pass without a staged intermediate BAM.
//!   Chunks are sorted in parallel and spilled as compressed runs.
//! * [`coordinate_sort`] sorts an existing BAM file (used by `group`).
//!
//! The sort key follows `samtools sort`: reference id, position, unmapped last,
//! reverse last. Ties keep emission order (stable sort, run index in the heap),
//! which is what makes the output byte-identical to UMI-tools.

use crate::error::{Error, Result};
use crate::io::spill::TempOutput;
use rayon::slice::ParallelSliceMut;
use rust_htslib::bam::{self, CompressionLevel, Format, Read, Record};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

type SortKey = (i32, i64, u8, u8);

fn sort_key(record: &Record) -> SortKey {
    let unmapped = u8::from(record.is_unmapped());
    let tid = if record.tid() < 0 {
        i32::MAX
    } else {
        record.tid()
    };
    (tid, record.pos(), unmapped, u8::from(record.is_reverse()))
}

fn estimate_bytes(record: &Record) -> u64 {
    64 + record.qname().len() as u64 + record.seq_len() as u64 + record.cigar().len() as u64 * 4
}

fn timing_enabled() -> bool {
    static TIMING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TIMING.get_or_init(|| std::env::var_os("RUST_UMITOOLS_TIMING").is_some())
}

/// Sort with an index vector: tie order is preserved (index is the final key)
/// without copying every full record, which keeps peak memory low.
fn write_sorted(buffer: &[Record], writer: &mut bam::Writer) -> Result<()> {
    let mut order: Vec<(SortKey, u32)> = buffer
        .iter()
        .enumerate()
        .map(|(index, record)| (sort_key(record), index as u32))
        .collect();
    order.par_sort_unstable();
    for (_, index) in order {
        writer
            .write(&buffer[index as usize])
            .map_err(|e| Error::Format(format!("write failed: {e}")))?;
    }
    Ok(())
}

fn open_writer(
    path: &Path,
    header: &bam::Header,
    threads: usize,
    level: CompressionLevel,
    format: Format,
    reference: Option<&Path>,
) -> Result<bam::Writer> {
    let mut writer = bam::Writer::from_path(path, header, format)
        .map_err(|e| Error::Format(format!("cannot write {}: {e}", path.display())))?;
    writer
        .set_threads(threads)
        .map_err(|e| Error::Format(format!("cannot set writer threads: {e}")))?;
    crate::io::configure_writer(&mut writer, format, reference)?;
    writer
        .set_compression_level(level)
        .map_err(|e| Error::Format(format!("cannot set compression level: {e}")))?;
    Ok(writer)
}

/// Streaming sorter: feed it records, get a coordinate-sorted BAM at the end.
pub struct SortingWriter {
    header: bam::Header,
    tmpdir: PathBuf,
    chunk_limit: u64,
    io_threads: usize,
    level: u32,
    final_format: Format,
    reference: Option<PathBuf>,
    buffer: Vec<Record>,
    buffered: u64,
    runs: Vec<TempOutput>,
    run_paths: Vec<PathBuf>,
}

impl SortingWriter {
    pub fn new(
        header: &bam::Header,
        tmpdir: &Path,
        chunk_limit: u64,
        threads: usize,
        level: u32,
        final_format: Format,
        reference: Option<&Path>,
    ) -> SortingWriter {
        // The chunk size is chosen by the caller from the hardware probe, so
        // cache size and free temporary space are part of the decision.
        let chunk_limit = chunk_limit.max(1024 * 1024);
        SortingWriter {
            header: header.clone(),
            tmpdir: tmpdir.to_path_buf(),
            chunk_limit,
            io_threads: crate::runtime::budget::io_worker_threads(threads),
            level,
            final_format,
            reference: reference.map(Path::to_path_buf),
            buffer: Vec::new(),
            buffered: 0,
            runs: Vec::new(),
            run_paths: Vec::new(),
        }
    }

    pub fn submit(&mut self, record: Record) -> Result<()> {
        self.buffered += estimate_bytes(&record);
        self.buffer.push(record);
        if self.buffered >= self.chunk_limit {
            self.spill_run()?;
        }
        Ok(())
    }

    fn spill_run(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let records = self.buffer.len();
        let bytes = self.buffered;
        // Checked here rather than at startup: a run that fits in memory never
        // needs temporary space, so an unrelated full disk must not block it.
        crate::runtime::budget::ensure_tmp_space(&self.tmpdir, bytes)?;
        let temp = TempOutput::create(&self.tmpdir, "umitools-run")?;
        {
            // Intermediate runs are read again immediately, so trade size for speed.
            let mut writer = open_writer(
                temp.path(),
                &self.header,
                self.io_threads,
                CompressionLevel::Fastest,
                Format::Bam,
                None,
            )?;
            write_sorted(&self.buffer, &mut writer)?;
        }
        self.run_paths.push(temp.path().to_path_buf());
        self.runs.push(temp);
        self.buffer.clear();
        self.buffered = 0;
        if timing_enabled() {
            eprintln!(
                "[timing] spill_run records={records} bytes={bytes} elapsed={:.3}s",
                started.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    /// Write the sorted output to `destination`.
    pub fn finish(mut self, destination: &Path) -> Result<()> {
        if self.runs.is_empty() {
            let started = std::time::Instant::now();
            let records = self.buffer.len();
            let mut writer = open_writer(
                destination,
                &self.header,
                self.io_threads,
                crate::io::compression_level(self.level)?,
                self.final_format,
                self.reference.as_deref(),
            )?;
            write_sorted(&self.buffer, &mut writer)?;
            if timing_enabled() {
                eprintln!(
                    "[timing] single_pass records={records} elapsed={:.3}s",
                    started.elapsed().as_secs_f64()
                );
            }
            return Ok(());
        }
        self.spill_run()?;
        // Keep every intermediate run alive until the final merge: each pass
        // reads the previous paths and TempOutput deletes its file on drop.
        // Bounding the fan-in also bounds open files and merge reader threads.
        let fan_in = std::env::var("RUST_UMITOOLS_MAX_FANIN")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value >= 2)
            .unwrap_or(crate::runtime::budget::MAX_MERGE_FAN_IN);
        let mut level: Vec<PathBuf> = std::mem::take(&mut self.run_paths);
        let mut owned: Vec<TempOutput> = std::mem::take(&mut self.runs);
        while level.len() > fan_in {
            let mut next = Vec::with_capacity(level.len().div_ceil(fan_in));
            for group in level.chunks(fan_in) {
                let temp = TempOutput::create(&self.tmpdir, "umitools-merge")?;
                merge_runs(
                    group,
                    temp.path(),
                    &self.header,
                    self.io_threads,
                    Format::Bam,
                    None,
                )?;
                next.push(temp.path().to_path_buf());
                owned.push(temp);
            }
            level = next;
        }
        merge_runs(
            &level,
            destination,
            &self.header,
            self.io_threads,
            self.final_format,
            self.reference.as_deref(),
        )?;
        drop(owned);
        Ok(())
    }
}

struct Run {
    reader: bam::Reader,
    current: Option<Record>,
}

impl Run {
    fn open(path: &Path) -> Result<Run> {
        // One decompression thread per run: a heap merge reads the runs
        // serially, so extra threads would only multiply worker count.
        let reader = bam::Reader::from_path(path)
            .map_err(|e| Error::Format(format!("cannot read sort run: {e}")))?;
        Ok(Run {
            reader,
            current: None,
        })
    }

    fn advance(&mut self) -> Result<()> {
        let mut record = Record::new();
        match self.reader.read(&mut record) {
            Some(Ok(())) => {
                self.current = Some(record);
                Ok(())
            }
            Some(Err(e)) => Err(Error::Format(format!("corrupt sort run: {e}"))),
            None => {
                self.current = None;
                Ok(())
            }
        }
    }
}

fn merge_runs(
    paths: &[PathBuf],
    output: &Path,
    header: &bam::Header,
    io_threads: usize,
    format: Format,
    reference: Option<&Path>,
) -> Result<()> {
    let started = std::time::Instant::now();
    let mut open: Vec<Run> = Vec::with_capacity(paths.len());
    for path in paths {
        let mut run = Run::open(path)?;
        run.advance()?;
        open.push(run);
    }

    let mut heap: BinaryHeap<Reverse<(SortKey, usize)>> = BinaryHeap::new();
    for (index, run) in open.iter().enumerate() {
        if let Some(record) = run.current.as_ref() {
            heap.push(Reverse((sort_key(record), index)));
        }
    }

    let mut writer = open_writer(
        output,
        header,
        io_threads,
        CompressionLevel::Level(6),
        format,
        reference,
    )?;
    let mut written = 0u64;
    while let Some(Reverse((_, index))) = heap.pop() {
        let record = match open[index].current.take() {
            Some(record) => record,
            None => continue,
        };
        writer
            .write(&record)
            .map_err(|e| Error::Format(format!("write failed: {e}")))?;
        written += 1;
        open[index].advance()?;
        if let Some(next) = open[index].current.as_ref() {
            heap.push(Reverse((sort_key(next), index)));
        }
    }
    if timing_enabled() {
        eprintln!(
            "[timing] merge runs={} records={} elapsed={:.3}s",
            paths.len(),
            written,
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

/// Sort an existing BAM file into `output`.
#[allow(clippy::too_many_arguments)]
pub fn coordinate_sort(
    input: &Path,
    output: &Path,
    tmpdir: &Path,
    chunk_bytes: u64,
    threads: usize,
    level: u32,
    format: Format,
    reference: Option<&Path>,
) -> Result<()> {
    let mut reader = bam::Reader::from_path(input)
        .map_err(|e| Error::Format(format!("cannot reopen {}: {e}", input.display())))?;
    let header = bam::Header::from_template(reader.header());
    let bytes_per_chunk = chunk_bytes.max(1024 * 1024);

    let mut sorter = SortingWriter::new(
        &header,
        tmpdir,
        bytes_per_chunk,
        threads,
        level,
        format,
        reference,
    );
    let mut chunk: Vec<Record> = Vec::new();
    let mut used = 0u64;
    for record in reader.records() {
        let record = record.map_err(|e| Error::Format(format!("corrupt input: {e}")))?;
        used += estimate_bytes(&record);
        chunk.push(record);
        if used >= bytes_per_chunk {
            for record in chunk.drain(..) {
                sorter.submit(record)?;
            }
            used = 0;
        }
    }
    for record in chunk.drain(..) {
        sorter.submit(record)?;
    }
    sorter.finish(output)
}
