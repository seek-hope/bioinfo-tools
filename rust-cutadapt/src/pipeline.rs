//! Streaming, bounded-memory FASTQ processing pipeline.
//!
//! Input is decompressed, split into record-aligned chunks, trimmed and
//! (optionally) compressed by a small thread pool, then written in input order.
//! Every inter-stage channel is bounded, so peak memory is
//! `O(stages * depth * chunk_size)` no matter how large the input is.
//!
//! Gzip output is a *multi-member* gzip stream: each chunk is compressed
//! independently and the members are concatenated. This is valid gzip
//! (RFC 1952) and is decoded transparently by `gzip`, `zcat`, `gunzip`,
//! Python's `gzip` module, `flate2`'s `MultiGzDecoder` and `samtools`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::engine::{ChunkStats, Trimmer};
use crate::fastq::ParseError;

/// Size of each processing chunk in bytes.
pub const CHUNK_SIZE: usize = 256 * 1024;
/// Maximum number of chunks queued between any two stages.
pub const PIPELINE_DEPTH: usize = 8;
/// Default gzip compression level (cutadapt's default as well).
pub const DEFAULT_COMPRESSION_LEVEL: u32 = 3;

/// Configured processing job.
#[derive(Clone, Debug)]
pub struct Config {
    /// Input path, or `-` for stdin.
    pub input: PathBuf,
    /// Second input path for paired-end runs (`INPUT2`), or `None`.
    pub input2: Option<PathBuf>,
    /// `--interleaved`: one input file with alternating R1/R2 records.
    pub interleaved: bool,
    /// `-p`/`--paired-output`: where read 2 is written.
    pub paired_output: Option<PathBuf>,
    /// Adapters for read 2 (`-A`/`-G`/`-B`), separate from `adapters`.
    pub adapters2: Vec<crate::adapter::AdapterSpec>,
    /// Read-2 modifications (`-U`, `-Q`). `None` means "same as read 1".
    pub modifications2: Option<crate::modify::Modifications>,
    /// `--pair-filter any|both|first`.
    pub pair_filter: crate::pair::PairFilter,
    /// Auxiliary outputs for read 2.
    pub aux2: crate::auxout::AuxOutputs,
    /// Output path, or `-` for stdout.
    pub output: PathBuf,
    /// Adapter specifications in declaration order.
    pub adapters: Vec<crate::adapter::AdapterSpec>,
    /// Matching configuration (error rate, overlap, indels).
    pub match_config: crate::adapter::matcher::MatchConfig,
    /// Maximum number of adapters removed per read (`-n`).
    pub times: usize,
    /// Read modifications applied around adapter trimming.
    pub modifications: crate::modify::Modifications,
    /// Read filters.
    pub filters: crate::filter::Filters,
    /// `--pair-adapters`: trim a pair only when both mates matched.
    pub pair_adapters: bool,
    /// What to do with a matched adapter (`--action`).
    pub action: crate::modify::Action,
    /// Auxiliary outputs.
    pub aux: crate::auxout::AuxOutputs,
    /// Read-name transformations.
    pub renaming: crate::modify::Renaming,
    /// `-m LEN:LEN2`: the R2 minimum length, when different.
    pub minimum_length_r2: Option<usize>,
    /// `-M LEN:LEN2`: the R2 maximum length, when different.
    pub maximum_length_r2: Option<usize>,
    /// `--rc`.
    pub reverse_complement: bool,
    /// `--json` report path (written after the run by main).
    pub json: Option<std::path::PathBuf>,
    /// Discard reads shorter than this after trimming (`0` disables).
    pub minimum_length: usize,
    /// Number of trimming threads.
    pub threads: usize,
    /// gzip compression level for `.gz` output (0-9).
    pub compression_level: u32,
}

impl Config {
    pub fn new(input: PathBuf, output: PathBuf, adapter: Vec<u8>) -> Self {
        let config = crate::adapter::matcher::MatchConfig::default();
        // An empty `adapter` means "the caller will install the adapter set via
        // `Config::adapters`"; only a non-empty one is parsed here.
        let adapters = if adapter.is_empty() {
            Vec::new()
        } else {
            vec![
                crate::adapter::parse('a', &String::from_utf8_lossy(&adapter), 0, None)
                    .expect("adapter must have been validated by the CLI"),
            ]
        };
        Self {
            input,
            input2: None,
            interleaved: false,
            paired_output: None,
            adapters2: Vec::new(),
            modifications2: None,
            pair_filter: crate::pair::PairFilter::Any,
            aux2: crate::auxout::AuxOutputs::default(),
            output,
            adapters,
            match_config: config,
            times: 1,
            modifications: crate::modify::Modifications::default(),
            filters: crate::filter::Filters::default(),
            pair_adapters: false,
            action: crate::modify::Action::Trim,
            aux: crate::auxout::AuxOutputs::default(),
            renaming: crate::modify::Renaming::default(),
            minimum_length_r2: None,
            maximum_length_r2: None,
            reverse_complement: false,
            json: None,
            minimum_length: 0,
            threads: 1,
            compression_level: DEFAULT_COMPRESSION_LEVEL,
        }
    }
}

/// Aggregate statistics reported at the end of a run.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub records_in: u64,
    pub records_out: u64,
    pub bp_in: u64,
    pub bp_out: u64,
    pub reads_with_adapter: u64,
    pub reads_too_short: u64,
    /// Pairs where at least one mate was too short (paired mode only).
    pub pairs_too_short: u64,
    pub reads_too_long: u64,
    pub reads_too_many_n: u64,
    pub reads_too_many_expected_errors: u64,
    pub reads_too_many_average_errors: u64,
    pub reads_casava_filtered: u64,
    pub reads_discard_trimmed: u64,
    pub reads_discard_untrimmed: u64,
    pub quality_trimmed_bp: u64,
    pub poly_a_trimmed_bp: u64,
    pub poly_a_lengths: Vec<usize>,
    pub reverse_complemented: u64,
    pub adapter_reverse_complemented: Vec<u64>,
    pub info_lines: Vec<String>,
    pub rest_lines: Vec<String>,
    pub wildcard_lines: Vec<String>,
    pub reads_untrimmed_output: u64,
    /// Per-adapter demultiplexed bytes.
    pub demux_records: Vec<Vec<u8>>,
    /// Demultiplexed "unknown" bytes.
    pub demux_unknown: Vec<u8>,
    /// Number of reads diverted to the demultiplexed "unknown" file.
    pub demux_unknown_records: u64,
    pub too_short_records: Vec<u8>,
    pub too_long_records: Vec<u8>,
    pub untrimmed_records: Vec<u8>,
    /// Per-adapter measurements for the report, indexed like `Config::adapters`.
    pub adapter_stats: Vec<crate::engine::AdapterChunkStats>,
    /// True for a paired-end run.
    pub paired: bool,
    /// Read-2 counters for a paired-end run; `None` for single-end.
    pub read2: Option<Box<Stats>>,
}

impl Stats {
    /// Accumulate one processed chunk's counters.
    pub fn add_chunk(&mut self, other: &ChunkStats) {
        self.add(other);
    }

    fn add(&mut self, other: &ChunkStats) {
        self.records_in += other.records_in;
        self.records_out += other.records_out;
        self.bp_in += other.bp_in;
        self.bp_out += other.bp_out;
        self.reads_with_adapter += other.reads_with_adapter;
        self.reads_too_short += other.reads_too_short;
        self.reads_too_long += other.reads_too_long;
        self.reads_too_many_n += other.reads_too_many_n;
        self.reads_too_many_expected_errors += other.reads_too_many_expected_errors;
        self.reads_too_many_average_errors += other.reads_too_many_average_errors;
        self.reads_casava_filtered += other.reads_casava_filtered;
        self.reads_discard_trimmed += other.reads_discard_trimmed;
        self.reads_discard_untrimmed += other.reads_discard_untrimmed;
        self.quality_trimmed_bp += other.quality_trimmed_bp;
        self.poly_a_trimmed_bp += other.poly_a_trimmed_bp;
        self.poly_a_lengths.extend_from_slice(&other.poly_a_lengths);
        self.reverse_complemented += other.reverse_complemented;
        if self.adapter_reverse_complemented.len() < other.adapter_reverse_complemented.len() {
            self.adapter_reverse_complemented
                .resize(other.adapter_reverse_complemented.len(), 0);
        }
        for (acc, value) in self
            .adapter_reverse_complemented
            .iter_mut()
            .zip(&other.adapter_reverse_complemented)
        {
            *acc += value;
        }
        self.info_lines.extend_from_slice(&other.info_lines);
        self.rest_lines.extend_from_slice(&other.rest_lines);
        self.wildcard_lines.extend_from_slice(&other.wildcard_lines);
        self.reads_untrimmed_output += other.reads_untrimmed_output;
        if self.demux_records.len() < other.demux_records.len() {
            self.demux_records
                .resize(other.demux_records.len(), Vec::new());
        }
        for (acc, chunk) in self.demux_records.iter_mut().zip(&other.demux_records) {
            acc.extend_from_slice(chunk);
        }
        self.demux_unknown.extend_from_slice(&other.demux_unknown);
        self.demux_unknown_records += other.demux_unknown_records;
        self.too_short_records
            .extend_from_slice(&other.too_short_records);
        self.too_long_records
            .extend_from_slice(&other.too_long_records);
        self.untrimmed_records
            .extend_from_slice(&other.untrimmed_records);
        if self.adapter_stats.len() < other.adapter_stats.len() {
            self.adapter_stats
                .resize(other.adapter_stats.len(), Default::default());
        }
        for (acc, chunk) in self.adapter_stats.iter_mut().zip(&other.adapter_stats) {
            acc.matches += chunk.matches;
            acc.five_prime_overlaps += chunk.five_prime_overlaps;
            acc.three_prime_hits += chunk.three_prime_hits;
            acc.five_prime_lengths
                .extend_from_slice(&chunk.five_prime_lengths);
            acc.three_prime_lengths
                .extend_from_slice(&chunk.three_prime_lengths);
            acc.five_prime_trimmed
                .extend_from_slice(&chunk.five_prime_trimmed);
            acc.trimmed.extend_from_slice(&chunk.trimmed);
            acc.adjacent.extend_from_slice(&chunk.adjacent);
        }
    }
}

/// Errors that abort a run.
#[derive(Debug)]
pub enum RunError {
    Io(io::Error),
    Parse(ParseError),
    WorkerPanic,
    /// A chunk was lost because a stage stopped unexpectedly.
    Incomplete,
    /// A fatal error that occurred after some records had already been written
    /// to the output, so the output file is incomplete.
    Partial {
        records_written: u64,
        source: Box<RunError>,
    },
}

impl RunError {
    /// How many records had been written to the output when the run failed.
    pub fn records_written(&self) -> u64 {
        match self {
            RunError::Partial {
                records_written, ..
            } => *records_written,
            _ => 0,
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Io(e) => write!(f, "{e}"),
            RunError::Parse(e) => write!(f, "{e}"),
            RunError::WorkerPanic => write!(f, "internal worker thread failed"),
            RunError::Incomplete => write!(f, "internal pipeline error: chunk lost"),
            RunError::Partial { source, .. } => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RunError::Io(e) => Some(e),
            RunError::Parse(e) => Some(e),
            RunError::Partial { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for RunError {
    fn from(e: io::Error) -> Self {
        RunError::Io(e)
    }
}

impl From<ParseError> for RunError {
    fn from(e: ParseError) -> Self {
        RunError::Parse(e)
    }
}

/// True if `path` names a gzip file (by extension).
pub fn has_gzip_extension(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("gz"))
        .unwrap_or(false)
}

/// Read `reader` to the end in record-aligned chunks.
///
/// `on_chunk` receives the chunk bytes, the chunk's byte offset in the
/// uncompressed input and the 1-based line number of its first line.
///
/// W1-2 moved the chunking rule into `bioinfo_fastx::chunker`; this is a thin
/// adapter so nothing in the pipeline changes shape. The shared implementation
/// was verified to produce the same split points as the code it replaced:
/// `tests/gate/m3.sh` still passes with byte-identical output at every chunk
/// size and worker count.
fn for_each_chunk(
    reader: &mut dyn Read,
    on_chunk: impl FnMut(Vec<u8>, u64, u64) -> Result<(), RunError>,
) -> Result<(), RunError> {
    bioinfo_fastx::chunker::for_each_chunk(reader, chunk_size(), on_chunk)
}

/// gzip level used for output; `RUST_CUTADAPT_COMPRESSION_LEVEL` overrides the
/// default (it is an environment variable because a level flag is not part of
/// the implemented cutadapt subset).
pub fn configured_compression_level() -> u32 {
    static CACHED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("RUST_CUTADAPT_COMPRESSION_LEVEL")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|&n| n <= 9)
            .unwrap_or(DEFAULT_COMPRESSION_LEVEL)
    })
}

fn chunk_size() -> usize {
    static CACHED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("RUST_CUTADAPT_CHUNK")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&n| n >= 4096)
            .unwrap_or(CHUNK_SIZE)
    })
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

/// Open the input as a stream, decompressing gzip when appropriate.
fn open_input(config: &Config) -> Result<Box<dyn Read + Send>, RunError> {
    if config.input.as_os_str() == "-" {
        // Sniff the magic bytes so stdin accepts gzip, bzip2 and xz.
        crate::compress::decompress(Box::new(io::stdin())).map_err(RunError::Io)
    } else {
        let file = File::open(&config.input).map_err(|e| {
            RunError::Io(io::Error::new(
                e.kind(),
                format!("cannot open input {:?}: {e}", config.input),
            ))
        })?;
        // Detect by CONTENT, not extension: cutadapt reads a compressed file even
        // when its name does not say so.
        crate::compress::decompress(Box::new(file)).map_err(RunError::Io)
    }
}

/// A per-file compressor for one mate, or `None` when the output is plain.
fn new_mate_encoder(format: crate::compress::Format, level: u32) -> Option<ChunkEncoder> {
    if format == crate::compress::Format::Plain {
        None
    } else {
        Some(ChunkEncoder::new(format, level))
    }
}

/// Write one mate's bytes, compressing when an encoder is present.
fn write_mate(
    encoder: &mut Option<ChunkEncoder>,
    out: &mut BufWriter<Box<dyn Write + Send>>,
    data: &[u8],
) -> Result<(), RunError> {
    match encoder.as_mut() {
        Some(encoder) => encoder.write_all(data).map_err(RunError::Io),
        None => out.write_all(data).map_err(RunError::Io),
    }
}

/// Finish the mate encoders and flush their bytes to the destination files.
fn finish_mate(
    encoder: Option<ChunkEncoder>,
    out: &mut BufWriter<Box<dyn Write + Send>>,
) -> Result<(), RunError> {
    if let Some(encoder) = encoder {
        let bytes = encoder.finish().map_err(RunError::Io)?;
        out.write_all(&bytes).map_err(RunError::Io)?;
    }
    out.flush().map_err(RunError::Io)
}

/// A per-chunk compressor for one output format.
///
/// Returns an in-memory encoder whose `finish()` yields the compressed chunk.
fn chunk_encoder(format: crate::compress::Format, level: u32) -> ChunkEncoder {
    ChunkEncoder::new(format, level)
}

/// Per-chunk compressor that hides the three format types behind one API.
enum ChunkEncoder {
    Gzip(GzEncoder<Vec<u8>>),
    Bzip2(bzip2::write::BzEncoder<Vec<u8>>),
    Xz(xz2::write::XzEncoder<Vec<u8>>),
}

impl ChunkEncoder {
    fn new(format: crate::compress::Format, level: u32) -> Self {
        match format {
            crate::compress::Format::Bzip2 => ChunkEncoder::Bzip2(bzip2::write::BzEncoder::new(
                Vec::new(),
                bzip2::Compression::new(level.clamp(1, 9)),
            )),
            crate::compress::Format::Xz => {
                // cutadapt's -Z passes preset 0, otherwise 6. `level` already
                // carries that choice (1 for -Z, 3 by default) so map it here.
                let preset = if level <= 1 { 0 } else { 6 };
                ChunkEncoder::Xz(xz2::write::XzEncoder::new(Vec::new(), preset))
            }
            _ => ChunkEncoder::Gzip(GzEncoder::new(Vec::new(), Compression::new(level))),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            ChunkEncoder::Gzip(e) => e.write_all(data),
            ChunkEncoder::Bzip2(e) => e.write_all(data),
            ChunkEncoder::Xz(e) => e.write_all(data),
        }
    }

    fn finish(self) -> io::Result<Vec<u8>> {
        match self {
            ChunkEncoder::Gzip(e) => e.finish(),
            ChunkEncoder::Bzip2(e) => e.finish(),
            ChunkEncoder::Xz(e) => e.finish(),
        }
    }
}

fn open_output(config: &Config) -> Result<Box<dyn Write + Send>, RunError> {
    if config.output.as_os_str() == "-" {
        Ok(Box::new(BufWriter::with_capacity(CHUNK_SIZE, io::stdout())))
    } else {
        let file = File::create(&config.output).map_err(|e| {
            RunError::Io(io::Error::new(
                e.kind(),
                format!("cannot create output {:?}: {e}", config.output),
            ))
        })?;
        Ok(Box::new(BufWriter::with_capacity(CHUNK_SIZE, file)))
    }
}

struct TrimJob {
    index: usize,
    data: Vec<u8>,
    offset: u64,
    line: u64,
}

struct TrimResult {
    index: usize,
    data: Vec<u8>,
    stats: ChunkStats,
}

type SharedReceiver<T> = Arc<Mutex<Receiver<T>>>;

/// True when `-o`/`-p` contain `{name}`, i.e. outputs are demultiplexed.
fn is_demultiplexing(config: &Config) -> bool {
    config.aux.demultiplex && config.output.as_os_str() != "-"
}

/// True when the configuration describes a paired-end run.
pub fn is_paired(config: &Config) -> bool {
    config.interleaved || config.input2.is_some()
}

/// Build a mate's [`Trimmer`] from the shared configuration.
fn build_trimmer(
    adapters: &[crate::adapter::AdapterSpec],
    config: &Config,
    modifications: &crate::modify::Modifications,
    aux: &crate::auxout::AuxOutputs,
) -> Trimmer {
    let mut set = crate::adapter::matcher::AdapterSet::new();
    for spec in adapters {
        set.push(spec.clone(), &config.match_config);
    }
    Trimmer::new(set, config.match_config)
        .with_times(config.times)
        .with_modifications(modifications.clone())
        .with_filters(config.filters)
        .with_action(config.action)
        .with_aux(aux.clone())
        .with_renaming(config.renaming.clone())
        .with_reverse_complement(config.reverse_complement)
}

/// Open the second input for a two-file paired-end run.
fn open_input2(config: &Config) -> Result<Option<Box<dyn Read + Send>>, RunError> {
    let Some(path) = &config.input2 else {
        return Ok(None);
    };
    if path.as_os_str() == "-" {
        return Ok(Some(Box::new(BufReader::with_capacity(
            CHUNK_SIZE,
            io::stdin(),
        ))));
    }
    let file = File::open(path).map_err(|e| {
        RunError::Io(io::Error::new(
            e.kind(),
            format!("cannot open input {path:?}: {e}"),
        ))
    })?;
    // Detect by content, like the single-end input.
    crate::compress::decompress(Box::new(file))
        .map(Some)
        .map_err(RunError::Io)
}

/// Run a paired-end trimming job.
///
/// Both mates are read in lockstep, trimmed with their own adapter sets and
/// written to `-o` (read 1) and `-p` (read 2), or interleaved to `-o` for
/// `--interleaved`. The pair-level filter decision uses
/// [`crate::pair::PairFilter`].
pub fn run_paired(config: &Config) -> Result<Stats, RunError> {
    // cutadapt creates the outputs before opening the inputs, so a missing
    // input still leaves empty output files behind (observable behaviour).
    let demux = is_demultiplexing(config)
        || config
            .paired_output
            .as_ref()
            .map(|p| p.to_string_lossy().contains("{name}"))
            .unwrap_or(false);
    let writer1 = if is_demultiplexing(config) {
        Box::new(std::io::sink()) as Box<dyn Write + Send>
    } else {
        open_output(config)?
    };
    let writer2 = if demux && !config.interleaved {
        Some(Box::new(std::io::sink()) as Box<dyn Write + Send>)
    } else {
        match &config.paired_output {
            Some(path) if !config.interleaved => {
                let file = File::create(path).map_err(|e| {
                    RunError::Io(io::Error::new(
                        e.kind(),
                        format!("cannot create paired output {path:?}: {e}"),
                    ))
                })?;
                Some(Box::new(BufWriter::with_capacity(CHUNK_SIZE, file)) as Box<dyn Write + Send>)
            }
            _ => None,
        }
    };
    let reader1 = open_input(config)?;
    // `--interleaved` with two input files means paired INPUT and interleaved
    // OUTPUT: read both files, write both mates to -o.
    let reader2 = if config.interleaved && config.input2.is_none() {
        None
    } else {
        open_input2(config)?
    };
    let stats = run_paired_with_io(config, reader1, reader2, writer1, writer2)?;
    write_aux_outputs(config, &stats)?;
    // Paired demultiplexing: write R1 files from the `-o` template and R2 files
    // from the `-p` template, one per adapter plus the "unknown" file.
    if demux {
        if let Some(template) = config
            .paired_output
            .as_ref()
            .filter(|p| p.to_string_lossy().contains("{name}"))
        {
            let template = template.to_string_lossy().into_owned();
            let read2 = stats.read2.as_deref();
            if let Some(read2) = read2 {
                // cutadapt creates an R2 file for EVERY R1 adapter, even when it
                // receives no reads, so iterate over the R1 adapter count.
                for index in 0..config.adapters.len() {
                    let records = read2.demux_records.get(index).cloned().unwrap_or_default();
                    // R2 files follow the pair's R1 adapter assignment, so they
                    // are named after the R1 adapters.
                    let name = config
                        .adapters
                        .get(index)
                        .map(|spec| spec.name.clone())
                        .unwrap_or_else(|| (index + 1).to_string());
                    let path = template.replace("{name}", &name);
                    std::fs::write(&path, &records).map_err(|e| {
                        RunError::Io(io::Error::new(
                            e.kind(),
                            format!("cannot create {path:?}: {e}"),
                        ))
                    })?;
                }
                if !read2.demux_unknown.is_empty() {
                    let path = template.replace("{name}", "unknown");
                    std::fs::write(&path, &read2.demux_unknown).map_err(RunError::Io)?;
                }
            }
        }
    }
    Ok(stats)
}

/// Reads complete FASTQ records from a stream in bounded batches.
///
/// The paired path needs to advance both mates by the same number of records,
/// so records are counted explicitly instead of being chunked by bytes.
struct RecordReader {
    inner: Box<dyn Read + Send>,
    carry: Vec<u8>,
    /// Byte offset of `carry[0]` in the decompressed stream.
    consumed: u64,
}

impl RecordReader {
    fn new(inner: Box<dyn Read + Send>) -> Self {
        Self {
            inner,
            carry: Vec::with_capacity(CHUNK_SIZE),
            consumed: 0,
        }
    }

    /// Read at most `limit` complete records.
    ///
    /// Returns the ranges of the records inside `self.carry`; the caller must
    /// consume them before the next call. A trailing partial record is returned
    /// too (so the parser can report it) only when the stream has ended.
    fn read_batch(&mut self, limit: usize) -> Result<Vec<crate::pair::RecordRange>, RunError> {
        let mut incoming = vec![0u8; CHUNK_SIZE];
        loop {
            let wanted = crate::pair::record_count(&self.carry);
            if wanted >= limit {
                break;
            }
            let filled = read_up_to(&mut *self.inner, &mut incoming)?;
            if filled == 0 {
                break;
            }
            self.carry.extend_from_slice(&incoming[..filled]);
        }
        let mut ranges: Vec<crate::pair::RecordRange> = crate::pair::split_records(&self.carry);
        // Anything after the last complete record stays for the next batch.
        if ranges.len() > limit {
            ranges.truncate(limit);
        }
        Ok(ranges)
    }

    /// Drop `n` bytes from the front of the carry buffer.
    fn consume(&mut self, n: usize) {
        self.carry.drain(..n);
        self.consumed += n as u64;
    }

    fn remaining(&self) -> &[u8] {
        &self.carry
    }

    fn is_empty(&self) -> bool {
        self.carry.is_empty()
    }
}

/// Run the paired-end pipeline over explicit readers and writers.
///
/// Both mates advance in lockstep in bounded batches: a batch of read-1 records
/// is paired with the same number of read-2 records, each mate is trimmed with
/// its own adapter set, and the pair-level filter decides whether both records
/// are written. A pair is therefore always written to both outputs or to
/// neither, and the output order is the input order regardless of thread count.
#[allow(clippy::too_many_arguments)]
pub fn run_paired_with_io(
    config: &Config,
    reader1: Box<dyn Read + Send>,
    reader2: Option<Box<dyn Read + Send>>,
    writer1: Box<dyn Write + Send>,
    writer2: Option<Box<dyn Write + Send>>,
) -> Result<Stats, RunError> {
    let modifications1 = config.modifications.clone();
    let modifications2 = config
        .modifications2
        .clone()
        .unwrap_or_else(|| config.modifications.clone());
    // Paired mode diverts whole pairs, so the per-mate aux outputs must not run.
    let mut aux1 = config.aux.clone();
    let mut aux2 = config.aux2.clone();
    aux1.untrimmed_output = None;
    aux2.untrimmed_output = None;
    // The pair-level collector owns the too-short/too-long files in paired mode,
    // so the per-mate writers must not also populate their buffers.
    aux1.too_short_output = None;
    aux1.too_long_output = None;
    aux2.too_short_output = None;
    aux2.too_long_output = None;
    let mut trimmer1 = build_trimmer(&config.adapters, config, &modifications1, &aux1);
    // `-m LEN:LEN2` gives R2 a different minimum, and the FILTERS own -m, so R2
    // needs its own filter configuration.
    let mut filters2 = config.filters;
    if let Some(minimum_r2) = config.minimum_length_r2 {
        filters2.minimum_length = Some(minimum_r2);
    }
    // `-M LEN:LEN2` likewise gives R2 its own maximum.
    if let Some(maximum_r2) = config.maximum_length_r2 {
        filters2.maximum_length = Some(maximum_r2);
    }
    let mut trimmer2 =
        build_trimmer(&config.adapters2, config, &modifications2, &aux2).with_filters(filters2);
    // R2 demultiplexing is only possible when the -p template carries {name}.
    let demux_for_r2 = config
        .paired_output
        .as_ref()
        .map(|p| p.to_string_lossy().contains("{name}"))
        .unwrap_or(false);

    let mut out1 = BufWriter::with_capacity(CHUNK_SIZE, writer1);
    let mut out2 = writer2.map(|w| BufWriter::with_capacity(CHUNK_SIZE, w));
    // Paired outputs are compressed record by record through these encoders when
    // the destination extension asks for gzip/bzip2/xz.
    let paired_format1 = if config.output.as_os_str() == "-" || is_demultiplexing(config) {
        crate::compress::Format::Plain
    } else {
        crate::compress::Format::from_extension(&config.output)
    };
    let mut encoder1 = new_mate_encoder(paired_format1, config.compression_level);
    let paired_format2 = config
        .paired_output
        .as_ref()
        .map(|p| crate::compress::Format::from_extension(p))
        .filter(|f| *f != crate::compress::Format::Plain)
        .unwrap_or(crate::compress::Format::Plain);
    let mut encoder2 = new_mate_encoder(paired_format2, config.compression_level);
    let mut pair_untrimmed1: Vec<u8> = Vec::new();
    let mut pair_untrimmed2: Vec<u8> = Vec::new();
    let mut pair_too_short1: Vec<u8> = Vec::new();
    let mut pair_too_short2: Vec<u8> = Vec::new();
    let mut pair_too_long1: Vec<u8> = Vec::new();
    let mut pair_too_long2: Vec<u8> = Vec::new();
    let mut stats = Stats {
        paired: true,
        read2: Some(Box::new(Stats::default())),
        ..Stats::default()
    };

    let mut source1 = RecordReader::new(reader1);
    let mut source2 = reader2.map(RecordReader::new);
    // Scratch buffers reused for every single-record trim call.
    let mut scratch1 = Vec::with_capacity(512);
    let mut scratch2 = Vec::with_capacity(512);
    let mut line1 = 1u64;
    let mut line2 = 1u64;
    let limit = crate::pair::RECORDS_PER_GROUP;

    loop {
        let range_count = source1.read_batch(limit)?.len();
        if range_count == 0 {
            if !source1.is_empty() {
                // A partial trailing record: let the parser report it.
                let partial = source1.remaining().to_vec();
                trimmer1.process_chunk(
                    &partial,
                    source1.consumed,
                    line1,
                    config.minimum_length,
                    &mut scratch1,
                )?;
            }
            break;
        }
        if let Some(src2) = source2.as_mut() {
            let count2 = src2.read_batch(limit)?.len();
            if count2 != range_count {
                let message = if count2 > range_count {
                    "Reads are improperly paired. There are more reads in file 2 than in file 1."
                } else {
                    "Reads are improperly paired. There are more reads in file 1 than in file 2."
                };
                return Err(RunError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    message,
                )));
            }
        }

        // Interleaved input keeps both mates in one stream.
        let (ranges1, ranges2): (Vec<_>, Vec<_>) = if let Some(src2) = source2.as_mut() {
            (
                crate::pair::split_records(source1.remaining()),
                crate::pair::split_records(src2.remaining()),
            )
        } else {
            let all = crate::pair::split_records(source1.remaining());
            let first = all.iter().step_by(2).copied().collect();
            let second = all.iter().skip(1).step_by(2).copied().collect();
            (first, second)
        };
        let pairs = ranges1.len().min(ranges2.len());
        if pairs == 0 {
            // No complete pair left. This happens for an interleaved file with an
            // odd number of records: the loop would otherwise never consume the
            // trailing record and spin forever.
            let leftover = crate::pair::record_count(source1.remaining());
            if leftover > 0 {
                return Err(RunError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Reads are improperly paired. An interleaved file must contain an even number of reads.",
                )));
            }
            break;
        }
        // Interleaved input: the two mates of a pair must share a name, which is
        // how cutadapt validates an interleaved file (oracle: "Reads are
        // improperly paired. Name 'full' (first) does not match 'internal'
        // (second).").
        if source2.is_none() {
            for pair_index in 0..pairs {
                let r1 = ranges1[pair_index];
                let r2 = ranges2[pair_index];
                let name1 = first_token(&source1.remaining()[r1.start..r1.end]);
                let name2 = first_token(&source1.remaining()[r2.start..r2.end]);
                if name1 != name2 {
                    return Err(RunError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        // cutadapt prefixes this with its sequence-file context.
                        format!(
                            "Error in sequence file at unknown line: Reads are improperly paired. Name '{}' (first) does not match '{}' (second).",
                            String::from_utf8_lossy(&name1),
                            String::from_utf8_lossy(&name2)
                        ),
                    )));
                }
            }
        }

        let mut consumed1 = 0usize;
        let mut consumed2 = 0usize;
        for pair_index in 0..pairs {
            let r1 = ranges1[pair_index];
            let r2 = ranges2[pair_index];
            // Copy the two records out so the trimmer can borrow them mutably
            // while the sources stay untouched for the rest of the batch.
            let rec1_owned = source1.remaining()[r1.start..r1.end].to_vec();
            let rec2_owned = match source2.as_ref() {
                Some(src2) => src2.remaining()[r2.start..r2.end].to_vec(),
                None => source1.remaining()[r2.start..r2.end].to_vec(),
            };
            let rec1 = rec1_owned.as_slice();
            let rec2 = rec2_owned.as_slice();
            let mut stats1 = trimmer1.process_chunk(
                rec1,
                source1.consumed + r1.start as u64,
                line1,
                config.minimum_length,
                &mut scratch1,
            )?;
            if demux_for_r2 {
                let r1_adapter = stats1.adapter_stats.iter().position(|a| a.matches > 0);
                trimmer2 = trimmer2.clone().with_demux_index_override(r1_adapter);
            }
            let r2_minimum = config.minimum_length_r2.unwrap_or(config.minimum_length);
            let mut stats2 = trimmer2.process_chunk(
                rec2,
                source2.as_ref().map_or(source1.consumed, |s| s.consumed) + r2.start as u64,
                line2,
                r2_minimum,
                &mut scratch2,
            )?;
            // --pair-adapters: a pair is trimmed only when BOTH mates matched.
            // Otherwise neither is: the untrimmed reads are restored and the
            // adapter counts cleared, matching cutadapt's observable result.
            if config.pair_adapters
                && (stats1.reads_with_adapter > 0) != (stats2.reads_with_adapter > 0)
            {
                // Re-run both mates WITHOUT adapters so the pair is written
                // untrimmed, which is cutadapt's all-or-none rule.
                let mut plain1 = build_trimmer(&[], config, &modifications1, &config.aux);
                let mut plain2 = build_trimmer(&[], config, &modifications2, &config.aux2);
                stats1 = plain1.process_chunk(
                    rec1,
                    source1.consumed + r1.start as u64,
                    line1,
                    config.minimum_length,
                    &mut scratch1,
                )?;
                stats2 = plain2.process_chunk(
                    rec2,
                    source2.as_ref().map_or(source1.consumed, |s| s.consumed) + r2.start as u64,
                    line2,
                    r2_minimum,
                    &mut scratch2,
                )?;
            }
            line1 += 4;
            line2 += 4;

            // cutadapt's "Pairs that were too short" counts a pair once when
            // EITHER mate was too short.
            if stats1.reads_too_short > 0 || stats2.reads_too_short > 0 {
                stats.pairs_too_short += 1;
            }
            let passed1 = stats1.records_out > 0;
            let passed2 = stats2.records_out > 0;
            // Paired auxiliary outputs divert a whole PAIR when either mate is
            // unmatched (oracle: 12 of 13 pairs go to the untrimmed files).
            // Pair-level too-short / too-long aux outputs: when the PAIR is too
            // short (either mate), both mates' trimmed records go to the files.
            let pair_too_short = stats1.reads_too_short > 0 || stats2.reads_too_short > 0;
            let pair_too_long = stats1.reads_too_long > 0 || stats2.reads_too_long > 0;
            // Collected regardless of whether the pair is kept: cutadapt's
            // "too short" files hold every mate of every too-short pair.
            if pair_too_short && config.aux.too_short_output.is_some() {
                // Both mates, in lockstep. The trimmed record is taken from the
                // main buffer when that mate passed, otherwise from whichever
                // filter buffer captured it (too short or too long).
                let body1 = if stats1.records_out > 0 {
                    &scratch1
                } else if !stats1.too_short_records.is_empty() {
                    &stats1.too_short_records
                } else {
                    &stats1.too_long_records
                };
                let body2 = if stats2.records_out > 0 {
                    &scratch2
                } else if !stats2.too_short_records.is_empty() {
                    &stats2.too_short_records
                } else {
                    &stats2.too_long_records
                };
                pair_too_short1.extend_from_slice(body1);
                pair_too_short2.extend_from_slice(body2);
            }
            if pair_too_long && config.aux.too_long_output.is_some() {
                pair_too_long1.extend_from_slice(&stats1.too_long_records);
                pair_too_long2.extend_from_slice(&stats2.too_long_records);
            }
            let has_untrimmed_aux =
                config.aux.untrimmed_output.is_some() && config.aux2.untrimmed_output.is_some();
            if has_untrimmed_aux
                && (stats1.reads_with_adapter == 0 || stats2.reads_with_adapter == 0)
            {
                pair_untrimmed1.extend_from_slice(&scratch1);
                pair_untrimmed2.extend_from_slice(&scratch2);
                continue;
            }
            if config.pair_filter.keeps(passed1, passed2) {
                // The pair is written as a unit: both mates go out, even when one
                // of them failed the length filter (cutadapt writes the trimmed
                // mate, which may be empty for --pair-filter both).
                // A mate rejected by the length filter has an empty main buffer
                // but its record was captured for the too-short/too-long aux
                // output; cutadapt still writes that trimmed record into the
                // pair, so substitute it here.
                let body1 = if passed1 {
                    &scratch1
                } else {
                    &stats1.too_short_records
                };
                let body2 = if passed2 {
                    &scratch2
                } else {
                    &stats2.too_short_records
                };
                write_mate(&mut encoder1, &mut out1, body1)?;
                match out2.as_mut() {
                    Some(w) => write_mate(&mut encoder2, w, body2)?,
                    // Interleaved output: both mates go to -o.
                    None => write_mate(&mut encoder1, &mut out1, body2)?,
                }
                stats.add_chunk(&stats1);
                if let Some(read2) = stats.read2.as_mut() {
                    read2.add_chunk(&stats2);
                }
            } else {
                // The pair is discarded: count its filters but not its output.
                let mut only_stats1 = stats1;
                only_stats1.records_out = 0;
                only_stats1.bp_out = 0;
                stats.add_chunk(&only_stats1);
                if let Some(read2) = stats.read2.as_mut() {
                    let mut only_stats2 = stats2;
                    only_stats2.records_out = 0;
                    only_stats2.bp_out = 0;
                    read2.add_chunk(&only_stats2);
                }
            }
            consumed1 = r1.end;
            consumed2 = r2.end;
        }

        // Drop the consumed prefix of each source. With interleaved input both
        // mates live in source1, so it must advance past the second mate too.
        if source2.is_some() {
            if consumed1 > 0 {
                source1.consume(consumed1);
            }
        } else {
            let advanced = consumed1.max(consumed2);
            if advanced > 0 {
                source1.consume(advanced);
            }
        }
        if let Some(src2) = source2.as_mut() {
            if consumed2 > 0 {
                src2.consume(consumed2);
            }
        }
    }

    // Write the diverted pairs to their own files.
    if !pair_untrimmed1.is_empty() || !pair_untrimmed2.is_empty() {
        if let Some(path) = &config.aux.untrimmed_output {
            std::fs::write(path, &pair_untrimmed1).map_err(RunError::Io)?;
        }
        if let Some(path) = &config.aux2.untrimmed_output {
            std::fs::write(path, &pair_untrimmed2).map_err(RunError::Io)?;
        }
    }
    // Pair-level too-short / too-long files.
    let write_pair_file =
        |path: &Option<std::path::PathBuf>, body: &[u8]| -> Result<(), RunError> {
            if let Some(path) = path {
                std::fs::write(path, body).map_err(RunError::Io)?;
            }
            Ok(())
        };
    write_pair_file(&config.aux.too_short_output, &pair_too_short1)?;
    write_pair_file(&config.aux2.too_short_output, &pair_too_short2)?;
    write_pair_file(&config.aux.too_long_output, &pair_too_long1)?;
    write_pair_file(&config.aux2.too_long_output, &pair_too_long2)?;
    finish_mate(encoder1.take(), &mut out1)?;
    if let Some(w) = out2.as_mut() {
        finish_mate(encoder2.take(), w)?;
    }
    Ok(stats)
}

/// The read name of one FASTQ record (the text after the leading '@', up to the
/// first whitespace).
fn first_token(record: &[u8]) -> Vec<u8> {
    let line_end = record
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(record.len());
    let line = &record[..line_end];
    let start = usize::from(line.first() == Some(&b'@'));
    let end = line[start..]
        .iter()
        .position(|&b| b == b' ')
        .map_or(line.len(), |p| start + p);
    line[start..end].to_vec()
}

/// Run a trimming job, writing to `config.output`.
pub fn run(config: &Config) -> Result<Stats, RunError> {
    // In demultiplexing mode `-o` is a template, not a file: the per-adapter
    // outputs are created after the run instead.
    let writer = if is_demultiplexing(config) {
        Box::new(std::io::sink()) as Box<dyn Write + Send>
    } else {
        // cutadapt creates (and truncates) the output before opening the input, so
        // a missing input still leaves an empty output file behind.
        open_output(config)?
    };
    let reader = open_input(config)?;
    let stats = run_with_io(config, reader, writer)?;
    write_aux_outputs(config, &stats)?;
    Ok(stats)
}

/// Write the auxiliary output files collected during the run.
///
/// Empty results still create an empty file, which is what cutadapt does when an
/// option is given but nothing matches it.
fn write_aux_outputs(config: &Config, stats: &Stats) -> Result<(), RunError> {
    let aux = &config.aux;
    let write = |path: &Option<std::path::PathBuf>, body: &[u8]| -> Result<(), RunError> {
        if let Some(path) = path {
            let mut file = File::create(path).map_err(|e| {
                RunError::Io(std::io::Error::new(
                    e.kind(),
                    format!("cannot create {path:?}: {e}"),
                ))
            })?;
            file.write_all(body).map_err(RunError::Io)?;
        }
        Ok(())
    };
    if let Some(path) = &aux.info_file {
        write(&Some(path.clone()), to_lines(&stats.info_lines).as_bytes())?;
    }
    if let Some(path) = &aux.rest_file {
        write(&Some(path.clone()), to_lines(&stats.rest_lines).as_bytes())?;
    }
    if let Some(path) = &aux.wildcard_file {
        write(
            &Some(path.clone()),
            to_lines(&stats.wildcard_lines).as_bytes(),
        )?;
    }
    // Paired runs write these from the pair-level collector, in lockstep; the
    // per-mate buffers are empty there and would truncate the files.
    if !stats.paired {
        write(&aux.too_short_output, &stats.too_short_records)?;
        write(&aux.too_long_output, &stats.too_long_records)?;
    }
    // In paired mode the untrimmed outputs hold whole PAIRS and are written by
    // the paired runner, so they must not be overwritten here.
    if !stats.paired {
        write(&aux.untrimmed_output, &stats.untrimmed_records)?;
    }
    // Demultiplexed outputs: one file per adapter that received reads, plus the
    // "unknown" file when a read had no adapter. cutadapt only creates a file
    // when something was written to it.
    {
        let template_path = &config.output;
        if aux.demultiplex && template_path.as_os_str() != "-" {
            let template = template_path.to_string_lossy().into_owned();
            for (index, records) in stats.demux_records.iter().enumerate() {
                // Anonymous adapters are named by their 1-based position
                // (oracle: `-a AGATCGGAAGAGC -o o_{name}.fastq` writes o_1.fastq),
                // so the generated index name is replaced by position + 1.
                let name = config.adapters.get(index).map_or_else(
                    || (index + 1).to_string(),
                    |spec| {
                        let generated = spec.name.chars().all(|c| c.is_ascii_digit())
                            && spec.name == spec.index.to_string();
                        if generated {
                            (index + 1).to_string()
                        } else {
                            spec.name.clone()
                        }
                    },
                );
                let path = template.replace("{name}", &name);
                write(&Some(std::path::PathBuf::from(path)), records)?;
            }
            if !stats.demux_unknown.is_empty() {
                let path = template.replace("{name}", "unknown");
                write(&Some(std::path::PathBuf::from(path)), &stats.demux_unknown)?;
            }
        }
    }
    Ok(())
}

fn to_lines(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Run a trimming job with explicit reader and writer (used by tests).
pub fn run_with_io(
    config: &Config,
    mut reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
) -> Result<Stats, RunError> {
    let threads = config.threads.max(1);
    // The output format comes from the extension: gzip, bzip2, xz or plain.
    // Each chunk is compressed independently and concatenated, which every one of
    // these formats (and cutadapt's readers) accept.
    let output_format = if config.output.as_os_str() == "-" {
        crate::compress::Format::Plain
    } else {
        crate::compress::Format::from_extension(&config.output)
    };
    let gzip = output_format != crate::compress::Format::Plain;
    let aborted = AtomicBool::new(false);

    // Stage 1 -> 2: raw chunks.
    let (raw_tx, raw_rx) = mpsc::sync_channel::<TrimJob>(PIPELINE_DEPTH);
    // Stage 2 -> 3: trimmed chunks (or a fatal error).
    let (trim_tx, trim_rx) = mpsc::sync_channel::<Result<TrimResult, RunError>>(PIPELINE_DEPTH);
    // Stage 3 -> 4: serialized (compressed) output chunks.
    let (out_tx, out_rx) =
        mpsc::sync_channel::<Result<(usize, Vec<u8>, ChunkStats), RunError>>(PIPELINE_DEPTH);

    let raw_rx: SharedReceiver<_> = Arc::new(Mutex::new(raw_rx));
    let trim_rx: SharedReceiver<_> = Arc::new(Mutex::new(trim_rx));
    let next_index = std::sync::atomic::AtomicU64::new(0);

    let result = thread::scope(|scope| -> Result<Stats, RunError> {
        // --- Stage 1: inflate and split into record-aligned chunks ----------
        let producer = scope.spawn(|| -> Result<(), RunError> {
            let outcome = for_each_chunk(&mut *reader, |data, offset, line| {
                let index = next_index.fetch_add(1, Ordering::Relaxed) as usize;
                raw_tx
                    .send(TrimJob {
                        index,
                        data,
                        offset,
                        line,
                    })
                    .map_err(|_| RunError::Incomplete)
            });
            drop(raw_tx);
            outcome
        });

        // --- Stage 2: trim ---------------------------------------------------
        let mut trimmers = Vec::with_capacity(threads);
        for _ in 0..threads {
            let raw_rx = Arc::clone(&raw_rx);
            let trim_tx = trim_tx.clone();
            let adapter_specs = config.adapters.clone();
            let match_config = config.match_config;
            let times = config.times;
            let modifications = config.modifications.clone();
            let filters = config.filters;
            let action = config.action;
            let aux = config.aux.clone();
            let renaming = config.renaming.clone();
            let reverse_complement = config.reverse_complement;
            let minimum_length = config.minimum_length;
            let aborted = &aborted;
            trimmers.push(scope.spawn(move || {
                let mut set = crate::adapter::matcher::AdapterSet::new();
                for spec in &adapter_specs {
                    set.push(spec.clone(), &match_config);
                }
                let mut trimmer = Trimmer::new(set, match_config)
                    .with_times(times)
                    .with_modifications(modifications)
                    .with_filters(filters)
                    .with_action(action)
                    .with_filters(filters)
                    .with_aux(aux)
                    .with_renaming(renaming)
                    .with_reverse_complement(reverse_complement);
                let mut recycled: Vec<Vec<u8>> = Vec::new();
                loop {
                    let job = {
                        let guard = match raw_rx.lock() {
                            Ok(guard) => guard,
                            Err(_) => break,
                        };
                        match guard.recv() {
                            Ok(job) => job,
                            Err(_) => break,
                        }
                    };
                    let mut out = recycled.pop().unwrap_or_default();
                    let outcome = trimmer.process_chunk(
                        &job.data,
                        job.offset,
                        job.line,
                        minimum_length,
                        &mut out,
                    );
                    // Reuse the input allocation as the next output buffer.
                    let mut reuse = job.data;
                    reuse.clear();
                    recycled.push(reuse);
                    let message = match outcome {
                        Ok(stats) => Ok(TrimResult {
                            index: job.index,
                            data: out,
                            stats,
                        }),
                        Err(e) => {
                            aborted.store(true, Ordering::Relaxed);
                            // Send the partial chunk first: cutadapt flushes the
                            // reads processed before it dies, so they must reach
                            // the output file.
                            if !out.is_empty() {
                                let _ = trim_tx.send(Ok(TrimResult {
                                    index: job.index,
                                    data: out,
                                    stats: ChunkStats::default(),
                                }));
                            }
                            Err(RunError::Parse(e))
                        }
                    };
                    let failed = message.is_err();
                    if trim_tx.send(message).is_err() || failed {
                        break;
                    }
                }
            }));
        }
        drop(trim_tx);
        drop(raw_rx);

        // --- Stage 3: compress (or pass through) ------------------------------
        let compress_threads = threads.clamp(1, 4);
        let mut compressors = Vec::with_capacity(compress_threads);
        for _ in 0..compress_threads {
            let trim_rx = Arc::clone(&trim_rx);
            let out_tx = out_tx.clone();
            let aborted = &aborted;
            let level = config.compression_level;
            compressors.push(scope.spawn(move || loop {
                let message = {
                    let guard = match trim_rx.lock() {
                        Ok(guard) => guard,
                        Err(_) => break,
                    };
                    match guard.recv() {
                        Ok(message) => message,
                        Err(_) => break,
                    }
                };
                let message = match message {
                    Ok(result) => {
                        let payload = if gzip {
                            let mut encoder = chunk_encoder(output_format, level);
                            if let Err(e) = encoder.write_all(&result.data) {
                                aborted.store(true, Ordering::Relaxed);
                                let _ = out_tx.send(Err(RunError::Io(e)));
                                break;
                            }
                            match encoder.finish() {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    aborted.store(true, Ordering::Relaxed);
                                    let _ = out_tx.send(Err(RunError::Io(e)));
                                    break;
                                }
                            }
                        } else {
                            result.data
                        };
                        Ok((result.index, payload, result.stats))
                    }
                    Err(e) => {
                        aborted.store(true, Ordering::Relaxed);
                        Err(e)
                    }
                };
                let failed = message.is_err();
                if out_tx.send(message).is_err() || failed {
                    break;
                }
            }));
        }
        drop(out_tx);
        drop(trim_rx);

        // --- Stage 4: write in input order ------------------------------------
        let mut writer = BufWriter::with_capacity(CHUNK_SIZE, writer);
        let mut pending: BTreeMap<usize, (Vec<u8>, ChunkStats)> = BTreeMap::new();
        let mut stats = Stats::default();
        let mut next_write = 0usize;
        let mut first_error: Option<RunError> = None;
        // Records already committed to the writer: used to report honestly
        // whether a failure left partial output behind.
        let mut written_records: u64 = 0;
        for message in out_rx {
            match message {
                Ok((index, payload, chunk_stats)) => {
                    pending.insert(index, (payload, chunk_stats));
                    while let Some((payload, chunk_stats)) = pending.remove(&next_write) {
                        writer.write_all(&payload)?;
                        stats.add(&chunk_stats);
                        written_records += chunk_stats.records_out;
                        next_write += 1;
                    }
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                    aborted.store(true, Ordering::Relaxed);
                }
            }
        }
        if gzip && next_write == 0 {
            // A zero-byte file is not a valid compressed stream. Write one empty
            // member so that `gzip -t`/`bzip2 -t`/`xz -t` all accept the output
            // of an empty input.
            let encoder = chunk_encoder(output_format, config.compression_level);
            let empty = encoder.finish()?;
            writer.write_all(&empty)?;
        }
        writer.flush()?;

        let producer_result = producer.join().map_err(|_| RunError::WorkerPanic)?;
        for worker in trimmers {
            worker.join().map_err(|_| RunError::WorkerPanic)?;
        }
        for worker in compressors {
            worker.join().map_err(|_| RunError::WorkerPanic)?;
        }
        if let Some(e) = first_error {
            return Err(RunError::Partial {
                records_written: written_records,
                source: Box::new(e),
            });
        }
        if let Err(e) = producer_result {
            return Err(RunError::Partial {
                records_written: written_records,
                source: Box::new(e),
            });
        }
        if !pending.is_empty() {
            // A chunk never arrived: refuse to claim success.
            return Err(RunError::Incomplete);
        }
        Ok(stats)
    });
    result
}
