//! The SAM/BAM subcommands.
//!
//! Each function is a thin layer: parse options, call into the streaming reader /
//! writer or the counting types, and emit output in `samtools`'s exact format.
//! The counting and ordering logic lives in `crate::stats` and
//! `crate::sort_order` so it can be unit-tested without touching the filesystem.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use noodles::sam;
use noodles::sam::alignment::io::Write as _;
use noodles::sam::alignment::record::cigar::op::Kind;
use noodles::sam::alignment::record::Cigar as _;

use crate::error::{Error, Result};
use crate::format::{detect_path, Format};
use crate::index_util;
use crate::io_util::{AlignmentReader, BamWriter, Record, SamWriter};
use crate::sort_order::SortKey;
use crate::stats::{FlagStat, IdxStats};

// --------------------------------------------------------------------------- //
// helpers
// --------------------------------------------------------------------------- //

/// Resolve header reference names once, preserving the existing lossy UTF-8
/// interpretation. Record IDs then index this table without per-record strings.
fn reference_names(header: &sam::Header) -> Vec<String> {
    header
        .reference_sequences()
        .iter()
        .map(|(name, _)| String::from_utf8_lossy(name.as_ref()).into_owned())
        .collect()
}

/// Parse a `--reference` style id from a record, tolerating an error.
fn tolerant_reference_id(record: &Record) -> Option<usize> {
    record.reference_sequence_id()
}

// --------------------------------------------------------------------------- //
// flags
// --------------------------------------------------------------------------- //

const FLAG_NAMES: [(u32, &str); 12] = [
    (0x1, "PAIRED"),
    (0x2, "PROPER_PAIR"),
    (0x4, "UNMAP"),
    (0x8, "MUNMAP"),
    (0x10, "REVERSE"),
    (0x20, "MREVERSE"),
    (0x40, "READ1"),
    (0x80, "READ2"),
    (0x100, "SECONDARY"),
    (0x200, "QCFAIL"),
    (0x400, "DUP"),
    (0x800, "SUPPLEMENTARY"),
];

const FLAGS_HELP: &str = concat!(
    "About: Convert between textual and numeric flag representation\n",
    "Usage: samtools flags FLAGS...\n",
    "\n",
    "Each FLAGS argument is either an INT (in decimal/hexadecimal/octal) representing\n",
    "a combination of the following numeric flag values, or a comma-separated string\n",
    "NAME,...,NAME representing a combination of the following flag names:\n",
    "\n",
    "   0x1     1  PAIRED         paired-end / multiple-segment sequencing technology\n",
    "   0x2     2  PROPER_PAIR    each segment properly aligned according to aligner\n",
    "   0x4     4  UNMAP          segment unmapped\n",
    "   0x8     8  MUNMAP         next segment in the template unmapped\n",
    "  0x10    16  REVERSE        SEQ is reverse complemented\n",
    "  0x20    32  MREVERSE       SEQ of next segment in template is rev.complemented\n",
    "  0x40    64  READ1          the first segment in the template\n",
    "  0x80   128  READ2          the last segment in the template\n",
    " 0x100   256  SECONDARY      secondary alignment\n",
    " 0x200   512  QCFAIL         not passing quality controls or other filters\n",
    " 0x400  1024  DUP            PCR or optical duplicate\n",
    " 0x800  2048  SUPPLEMENTARY  supplementary alignment\n",
);

fn invalid_flag(value: &str) -> Error {
    Error::runtime(format!("flags: Could not parse {value:?}"))
}

fn parse_flag_argument(value: &str) -> Result<u32> {
    if value.is_empty() {
        return Ok(0);
    }
    if value.as_bytes()[0].is_ascii_digit() {
        let (digits, radix) = if let Some(rest) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            (rest, 16)
        } else if let Some(rest) = value.strip_prefix('0') {
            (rest, 8)
        } else {
            (value, 10)
        };
        let end = digits
            .find(|character: char| !character.is_digit(radix))
            .unwrap_or(digits.len());
        // strtol accepts a numeric prefix. In particular, samtools interprets
        // "08" and "0b11" as zero and "1,2" as one.
        let digits = if end == 0 && radix == 8 {
            "0"
        } else {
            &digits[..end]
        };
        if digits.is_empty() {
            return Err(invalid_flag(value));
        }
        let parsed = i64::from_str_radix(digits, radix).map_err(|_| invalid_flag(value))?;
        return u32::try_from(parsed)
            .ok()
            .filter(|number| *number <= i32::MAX as u32)
            .ok_or_else(|| invalid_flag(value));
    }

    let mut flags = 0u32;
    for name in value.split(',') {
        let bit = FLAG_NAMES
            .iter()
            .find(|(_, candidate)| name.eq_ignore_ascii_case(candidate))
            .map(|(bit, _)| *bit)
            .ok_or_else(|| invalid_flag(value))?;
        flags |= bit;
    }
    Ok(flags)
}

/// Parse a samtools flag expression for a filter option.
pub fn parse_flag_value(value: &str) -> Result<u32> {
    parse_flag_argument(value)
}

/// Convert numeric or named SAM flags to samtools' tab-separated representation.
pub fn flags(values: &[String]) -> Result<String> {
    if values.is_empty() {
        return Ok(FLAGS_HELP.to_string());
    }
    let mut output = String::new();
    for value in values {
        let flags = parse_flag_argument(value)?;
        let names = FLAG_NAMES
            .iter()
            .filter(|(bit, _)| flags & bit != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(",");
        output.push_str(&format!("{flags:#x}\t{flags}\t{names}\n"));
    }
    Ok(output)
}

// --------------------------------------------------------------------------- //
// head
// --------------------------------------------------------------------------- //

/// Print selected header lines followed by at most `records` SAM records.
pub fn head(options: HeadOptions) -> Result<()> {
    let mut reader = AlignmentReader::open(&options.input)?;
    let header = reader.read_header()?;
    let mut header_bytes = Vec::new();
    {
        let mut writer = SamWriter::new(&mut header_bytes);
        writer.write_header(&header)?;
        writer.finish()?;
    }

    let mut out = open_writer(None)?;
    match options.headers {
        None => out
            .write_all(&header_bytes)
            .map_err(|error| Error::io(format!("cannot write header: {error}")))?,
        Some(limit) => {
            for line in header_bytes
                .split_inclusive(|byte| *byte == b'\n')
                .take(limit)
            {
                out.write_all(line)
                    .map_err(|error| Error::io(format!("cannot write header: {error}")))?;
            }
        }
    }

    if options.records > 0 {
        let mut writer = SamWriter::new(&mut out);
        for _ in 0..options.records {
            let mut record = Record::default();
            if reader.read_record(&header, &mut record)? == 0 {
                break;
            }
            writer.write_record(&header, &record)?;
        }
        writer.finish()?;
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// view
// --------------------------------------------------------------------------- //

/// Options for `view`.
pub struct ViewOptions {
    /// Input path; `-` means standard input.
    pub input: PathBuf,
    /// Output path; `None` means standard output.
    pub output: Option<PathBuf>,
    /// `-h`: include the header in text output.
    pub with_header: bool,
    /// `-b`: write BAM instead of SAM text.
    pub bam_output: bool,
    /// `-H`: print only the header.
    pub header_only: bool,
    /// `-c`: print only the count of matching records.
    pub count_only: bool,
    /// `-f`: require all of these flag bits.
    pub require_flags: u16,
    /// `-F`: exclude records with any of these flag bits.
    pub exclude_flags: u16,
    /// `-q`: minimum mapping quality (0 keeps all).
    pub min_mapq: u8,
    /// `--region`: query an indexed BAM (`chr1`, `chr1:10-20`).
    pub region: Option<String>,
    /// `-@`: worker threads for BAM output compression; 1 keeps single-threaded.
    pub threads: usize,
}

/// Options for `depth`.
///
/// This implements `-a`, `-aa`, `-s`, `-H`, `-q`, `-Q`, `-l`, `-J`, `-r`,
/// `-b`, `-o` and multiple inputs. File-list/index and flag-filter options are
/// tracked in the compatibility matrix rather than silently accepted. `-r`
/// currently filters a sequential stream instead of using an index.
pub struct DepthOptions {
    /// Input SAM/BAM paths; one column per input.
    pub inputs: Vec<PathBuf>,
    /// Output path; `None` means standard output.
    pub output: Option<PathBuf>,
    /// `-a`: emit zero-depth positions through each declared reference length.
    pub all_positions: bool,
    /// `-aa`: also emit references unused by every input.
    pub absolutely_all: bool,
    /// `-s`: count only one mate at overlapping template positions.
    pub remove_overlaps: bool,
    /// `-H`: emit the `#CHROM POS sample` header.
    pub header: bool,
    /// `-q`: minimum base quality; 0 disables the filter.
    pub min_baseq: u8,
    /// `-Q`: minimum mapping quality; 0 disables the filter.
    pub min_mapq: u8,
    /// `-l`: minimum read length; 0 disables the filter.
    pub min_read_len: usize,
    /// `-J`: count CIGAR deletions as covered reference bases.
    pub include_deletions: bool,
    /// `-r`: restrict output to a reference region (`chr1` or `chr1:10-20`).
    pub region: Option<String>,
    /// `-b`: restrict output to BED intervals (0-based half-open).
    pub bed: Option<PathBuf>,
    /// Optional file containing input paths, one per line; overrides `inputs`.
    pub file_list: Option<PathBuf>,
    /// `-g`: remove these bits from the default excluded set.
    pub remove_flags: u32,
    /// `-G`: add these bits to the default excluded set.
    pub exclude_flags: u32,
    /// `--incl-flags`: require at least one of these bits.
    pub include_flags: u32,
    /// `--require-flags`: require all of these bits.
    pub require_flags: u32,
}

/// Options for `coverage`.
pub struct CoverageOptions {
    /// Input SAM/BAM paths, combined by reference position.
    pub inputs: Vec<PathBuf>,
    /// Optional file containing input paths, one per line; overrides `inputs`.
    pub bam_list: Option<PathBuf>,
    /// Output path; `None` means standard output.
    pub output: Option<PathBuf>,
    /// `-H`: suppress the header row.
    pub no_header: bool,
    /// `-r`: restrict output to a reference region.
    pub region: Option<String>,
    /// `-q`: minimum mapping quality; 0 disables the filter.
    pub min_mapq: u8,
    /// `-Q`: minimum base quality; 0 disables the filter.
    pub min_baseq: u8,
    /// `-l`: minimum read length; 0 disables the filter.
    pub min_read_len: usize,
    /// Minimum per-position depth included in coverage statistics.
    pub min_depth: u32,
    /// Maximum reads per alignment start in each input; 0 means unlimited.
    pub max_depth: usize,
    /// Show breadth-of-coverage histogram instead of the table.
    pub histogram: bool,
    /// Show depth histogram instead of the table.
    pub plot_depth: bool,
    /// Use ASCII histogram glyphs.
    pub ascii: bool,
    /// Requested histogram bins; `None` uses terminal/default width.
    pub n_bins: Option<usize>,
    /// `--rf`: require at least one of these bits.
    pub required_flags: u32,
    /// `--ff`: replace the default excluded set when provided.
    pub filter_flags: Option<u32>,
}

/// Options for `head`.
pub struct HeadOptions {
    /// Input SAM/BAM path; `-` reads standard input.
    pub input: PathBuf,
    /// Maximum header lines; `None` emits all header lines.
    pub headers: Option<usize>,
    /// Maximum alignment records after the selected headers.
    pub records: usize,
}

fn passes_filters(record: &Record, options: &ViewOptions) -> bool {
    let flags = record.flags().bits();
    if flags & options.require_flags != options.require_flags {
        return false;
    }
    if flags & options.exclude_flags != 0 {
        return false;
    }
    record
        .mapping_quality()
        .map(|quality| quality.get())
        .unwrap_or(0)
        >= options.min_mapq
}

fn open_writer_send(path: Option<&Path>) -> Result<Box<dyn Write + Send>> {
    match path {
        Some(path) => {
            let file = File::create(path)
                .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
            Ok(Box::new(file))
        }
        None => Ok(Box::new(std::io::stdout())),
    }
}

fn passes_raw_filters(record: &noodles::bam::Record, options: &ViewOptions) -> bool {
    let flags = record.flags().bits();
    if flags & options.require_flags != options.require_flags {
        return false;
    }
    if flags & options.exclude_flags != 0 {
        return false;
    }
    record
        .mapping_quality()
        .map(|quality| quality.get())
        .unwrap_or(0)
        >= options.min_mapq
}

fn view_region(options: &ViewOptions, region_text: &str) -> Result<()> {
    let region: noodles::core::Region = region_text
        .parse()
        .map_err(|error| Error::usage(format!("invalid --region: {error}")))?;
    let mut reader = noodles::bam::io::indexed_reader::Builder::default()
        .build_from_path(&options.input)
        .map_err(|error| {
            Error::input(format!(
                "{}: --region needs an indexed BAM (.bai or .csi): {error}",
                options.input.display()
            ))
        })?;
    let header = reader
        .read_header()
        .map_err(|error| Error::io(format!("cannot read BAM header: {error}")))?;

    if options.header_only {
        let mut out = open_writer(options.output.as_deref())?;
        let mut writer = SamWriter::new(&mut out);
        writer.write_header(&header)?;
        writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    let mut query = reader
        .query(&header, &region)
        .map_err(|error| Error::io(format!("cannot query region: {error}")))?;
    let mut raw = noodles::bam::Record::default();

    if options.count_only {
        let mut count: u64 = 0;
        loop {
            match query.read_record(&mut raw) {
                Ok(0) => break,
                Ok(_) => {
                    if passes_raw_filters(&raw, options) {
                        count += 1;
                    }
                }
                Err(error) => return Err(Error::io(format!("cannot read query record: {error}"))),
            }
        }
        let mut out = open_writer(options.output.as_deref())?;
        writeln!(out, "{count}")
            .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    if options.bam_output {
        let out = open_writer(options.output.as_deref())?;
        let mut writer = BamWriter::new(out);
        writer.write_header(&header)?;
        loop {
            match query.read_record(&mut raw) {
                Ok(0) => break,
                Ok(_) => {
                    if passes_raw_filters(&raw, options) {
                        writer.write_alignment_record(&header, &raw)?;
                    }
                }
                Err(error) => return Err(Error::io(format!("cannot read query record: {error}"))),
            }
        }
        let mut out = writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    let mut out = open_writer(options.output.as_deref())?;
    {
        let mut writer = SamWriter::new(&mut out);
        if options.with_header {
            writer.write_header(&header)?;
        }
        loop {
            match query.read_record(&mut raw) {
                Ok(0) => break,
                Ok(_) => {
                    if passes_raw_filters(&raw, options) {
                        writer.write_alignment_record(&header, &raw)?;
                    }
                }
                Err(error) => return Err(Error::io(format!("cannot read query record: {error}"))),
            }
        }
        writer.finish()?;
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

/// `view`: emit SAM text (default), BAM (`-b`), a header (`-H`) or a count (`-c`).
pub fn view(options: ViewOptions) -> Result<()> {
    if let Some(region) = options.region.as_deref() {
        return view_region(&options, region);
    }
    let mut reader = AlignmentReader::open(&options.input)?;
    let header = reader.read_header()?;

    // `-H` prints just the header, in SAM text, ignoring `-b` as samtools does.
    if options.header_only {
        let mut out = open_writer(options.output.as_deref())?;
        let mut writer = SamWriter::new(&mut out);
        writer.write_header(&header)?;
        writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    // `-c` counts without writing records, so it wins over `-b`.
    if options.count_only {
        let mut count: u64 = 0;
        let mut record = Record::default();
        loop {
            let bytes = reader.read_record(&header, &mut record)?;
            if bytes == 0 {
                break;
            }
            if passes_filters(&record, &options) {
                count += 1;
            }
        }
        let mut out = open_writer(options.output.as_deref())?;
        writeln!(out, "{count}")
            .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    if options.bam_output {
        let mut record = Record::default();
        if options.threads > 1 {
            let out = open_writer_send(options.output.as_deref())?;
            let workers = NonZero::new(options.threads).expect("threads > 1");
            let mt = noodles::bgzf::io::multithreaded_writer::Builder::default()
                .set_worker_count(workers)
                .build_from_writer(out);
            let mut writer = noodles::bam::io::Writer::from(mt);
            writer
                .write_header(&header)
                .map_err(|error| Error::io(format!("cannot write BAM header: {error}")))?;
            loop {
                let bytes = reader.read_record(&header, &mut record)?;
                if bytes == 0 {
                    break;
                }
                if passes_filters(&record, &options) {
                    writer
                        .write_alignment_record(&header, &record)
                        .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))?;
                }
            }
            let mut mt = writer.into_inner();
            let mut out = mt
                .finish()
                .map_err(|error| Error::io(format!("cannot finish BGZF stream: {error}")))?;
            out.flush()
                .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
            return Ok(());
        }
        let out = open_writer(options.output.as_deref())?;
        let mut writer = BamWriter::new(out);
        writer.write_header(&header)?;
        loop {
            let bytes = reader.read_record(&header, &mut record)?;
            if bytes == 0 {
                break;
            }
            if passes_filters(&record, &options) {
                writer.write_record(&header, &record)?;
            }
        }
        let mut out = writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
        return Ok(());
    }

    // SAM text.
    let mut out = open_writer(options.output.as_deref())?;
    {
        let mut writer = SamWriter::new(&mut out);
        if options.with_header {
            writer.write_header(&header)?;
        }
        let mut record = Record::default();
        loop {
            let bytes = reader.read_record(&header, &mut record)?;
            if bytes == 0 {
                break;
            }
            if passes_filters(&record, &options) {
                writer.write_record(&header, &record)?;
            }
        }
        writer.finish()?;
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

fn open_writer(path: Option<&Path>) -> Result<Box<dyn Write>> {
    match path {
        None => Ok(Box::new(BufWriter::new(std::io::stdout()))),
        Some(path) => {
            let file = File::create(path)
                .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
            Ok(Box::new(BufWriter::new(file)))
        }
    }
}

// --------------------------------------------------------------------------- //
// depth
// --------------------------------------------------------------------------- //

type DepthMaps = Vec<BTreeMap<usize, u32>>;

#[derive(Clone, Copy)]
struct DepthRegion {
    tid: usize,
    start: usize,
    end: usize,
}

fn position_in_regions(position: usize, tid: usize, regions: Option<&[DepthRegion]>) -> bool {
    regions
        .map(|regions| {
            regions.iter().any(|region| {
                region.tid == tid && position >= region.start && position <= region.end
            })
        })
        .unwrap_or(true)
}

fn load_bed(path: &Path, references: &[(String, usize)]) -> Result<Vec<DepthRegion>> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
    let mut regions = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("track ")
            || line.starts_with("browser ")
        {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            return Err(Error::input(format!(
                "{}:{}: BED line needs at least 3 columns",
                path.display(),
                line_number + 1
            )));
        }
        let tid = references
            .iter()
            .position(|(name, _)| name == fields[0])
            .ok_or_else(|| {
                Error::input(format!(
                    "{}:{}: unknown reference in BED: {}",
                    path.display(),
                    line_number + 1,
                    fields[0]
                ))
            })?;
        let start: usize = fields[1].parse().map_err(|_| {
            Error::input(format!(
                "{}:{}: invalid BED start",
                path.display(),
                line_number + 1
            ))
        })?;
        let end: usize = fields[2].parse().map_err(|_| {
            Error::input(format!(
                "{}:{}: invalid BED end",
                path.display(),
                line_number + 1
            ))
        })?;
        if start >= end {
            return Err(Error::input(format!(
                "{}:{}: BED start must be less than end",
                path.display(),
                line_number + 1
            )));
        }
        let end = end.min(references[tid].1);
        regions.push(DepthRegion {
            tid,
            start: start + 1,
            end,
        });
    }
    Ok(regions)
}

fn position_in_region(position: usize, region: Option<&DepthRegion>) -> bool {
    region
        .map(|region| position >= region.start && position <= region.end)
        .unwrap_or(true)
}

fn parse_depth_region(text: &str, references: &[(String, usize)]) -> Result<DepthRegion> {
    let (name, coordinates) = match text.split_once(':') {
        Some((name, coordinates)) => (name, Some(coordinates)),
        None => (text, None),
    };
    let tid = references
        .iter()
        .position(|(reference, _)| reference == name)
        .ok_or_else(|| Error::usage(format!("depth: unknown reference in -r: {name}")))?;
    let length = references[tid].1;
    let (start, end) = match coordinates {
        None => (1, length),
        Some(coordinates) => {
            let (start, end) = coordinates
                .split_once('-')
                .ok_or_else(|| Error::usage(format!("depth: -r needs chr:from-to, got {text}")))?;
            let start: usize = start
                .replace(',', "")
                .parse()
                .map_err(|_| Error::usage(format!("depth: invalid -r start: {text}")))?;
            let end: usize = end
                .replace(',', "")
                .parse()
                .map_err(|_| Error::usage(format!("depth: invalid -r end: {text}")))?;
            if start == 0 || start > end {
                return Err(Error::usage(format!("depth: invalid -r interval: {text}")));
            }
            (start, end.min(length))
        }
    };
    Ok(DepthRegion { tid, start, end })
}

/// `depth`: per-reference, per-position aligned-read depth.
///
/// The implementation follows samtools' default flag filter
/// (`UNMAP,SECONDARY,QCFAIL,DUP`) and counts CIGAR `M`/`=`/`X` bases after
/// applying `-q` and `-Q`. It uses a `BTreeMap` per reference, so peak memory is
/// proportional to the number of covered positions, not the reference length.
/// `-a` fills gaps only on references used by at least one input; `-aa` also
/// includes unused references. `-r` is the exception: it emits the selected
/// interval even when no read overlaps it.
fn collect_depth(
    input: &Path,
    references: &[(String, usize)],
    regions: Option<&[DepthRegion]>,
    options: &DepthOptions,
) -> Result<(DepthMaps, Vec<bool>)> {
    let mut reader = AlignmentReader::open(input)?;
    let header = reader.read_header()?;
    let actual: Vec<(String, usize)> = header
        .reference_sequences()
        .iter()
        .map(|(name, sequence)| {
            (
                String::from_utf8_lossy(name.as_ref()).into_owned(),
                sequence.length().get(),
            )
        })
        .collect();
    if actual != references {
        return Err(Error::input(
            "depth inputs must have identical reference names and lengths".to_string(),
        ));
    }
    let mut depths: Vec<BTreeMap<usize, u32>> =
        (0..references.len()).map(|_| BTreeMap::new()).collect();
    let mut used = vec![false; references.len()];
    let mut overlaps: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut record = Record::default();
    loop {
        if reader.read_record(&header, &mut record)? == 0 {
            break;
        }
        let flags = u32::from(record.flags().bits());
        let excluded = (0x704 & !options.remove_flags) | options.exclude_flags;
        if flags & excluded != 0 {
            continue;
        }
        if options.include_flags != 0 && flags & options.include_flags == 0 {
            continue;
        }
        if flags & options.require_flags != options.require_flags {
            continue;
        }
        if record
            .mapping_quality()
            .map(|quality| quality.get())
            .unwrap_or(0)
            < options.min_mapq
        {
            continue;
        }
        if options.min_read_len > 0 && record.sequence().len() < options.min_read_len {
            continue;
        }
        let Some(tid) = record.reference_sequence_id() else {
            continue;
        };
        let Some(start) = record.alignment_start() else {
            continue;
        };
        if tid >= depths.len() {
            continue;
        }
        if let Some(regions) = regions {
            if !regions.iter().any(|region| region.tid == tid) {
                continue;
            }
        }
        used[tid] = true;

        let mut overlap_clip = 0;
        if options.remove_overlaps
            && record.flags().bits() & 0x1 != 0
            && record.flags().bits() & 0x8 == 0
        {
            let name: &[u8] = record.name().map(|name| name.as_ref()).unwrap_or(&[]);
            if let Some(end) = overlaps.remove(name) {
                overlap_clip = end;
            } else if let Some(end) = record.alignment_end().map(usize::from) {
                let mate_can_overlap = match record.mate_alignment_start() {
                    None => true,
                    Some(mate_start) => {
                        record.mate_reference_sequence_id() == Some(tid)
                            && usize::from(mate_start) <= end + 1
                    }
                };
                if mate_can_overlap {
                    overlaps.insert(name.to_vec(), end);
                }
            }
        }

        let mut reference_position = usize::from(start);
        let mut read_position = 0usize;
        let qualities = record.quality_scores().as_ref();
        for op in record.cigar().iter() {
            let op = op.map_err(|error| Error::runtime(format!("invalid CIGAR: {error}")))?;
            let length = op.len();
            match op.kind() {
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => {
                    for offset in 0..length {
                        let position = reference_position + offset;
                        if overlap_clip > 0 && position <= overlap_clip {
                            continue;
                        }
                        if !position_in_regions(position, tid, regions) {
                            continue;
                        }
                        let entry = depths[tid].entry(position).or_insert(0);
                        if qualities.get(read_position + offset).copied().unwrap_or(0)
                            >= options.min_baseq
                        {
                            *entry += 1;
                        }
                    }
                    reference_position += length;
                    read_position += length;
                }
                Kind::Insertion | Kind::SoftClip => read_position += length,
                Kind::Deletion => {
                    for offset in 0..length {
                        let position = reference_position + offset;
                        if overlap_clip > 0 && position <= overlap_clip {
                            continue;
                        }
                        if position_in_regions(position, tid, regions) {
                            let entry = depths[tid].entry(position).or_insert(0);
                            if options.include_deletions {
                                *entry += 1;
                            }
                        }
                    }
                    reference_position += length;
                }
                Kind::Skip => reference_position += length,
                Kind::HardClip | Kind::Pad => {}
            }
        }
    }
    Ok((depths, used))
}

/// `depth`: per-reference, per-position aligned-read depth.
///
/// This subset follows samtools' default flag filter (`UNMAP,SECONDARY,QCFAIL,DUP`)
/// and counts CIGAR `M`/`=`/`X` bases after applying `-q`/`-Q`. Each input gets
/// its own column; `-a` emits every declared reference position. Memory is
/// proportional to the union of covered positions, not the reference length.
pub fn depth(options: DepthOptions) -> Result<()> {
    let inputs = resolve_inputs(&options.inputs, options.file_list.as_ref(), "depth")?;
    let first = inputs.first().expect("non-empty inputs were validated");
    let mut first_reader = AlignmentReader::open(first)?;
    let first_header = first_reader.read_header()?;
    let references: Vec<(String, usize)> = first_header
        .reference_sequences()
        .iter()
        .map(|(name, sequence)| {
            (
                String::from_utf8_lossy(name.as_ref()).into_owned(),
                sequence.length().get(),
            )
        })
        .collect();
    let regions = if let Some(bed) = &options.bed {
        if options.region.is_some() {
            return Err(Error::usage(
                "depth -b and -r are mutually exclusive".to_string(),
            ));
        }
        Some(load_bed(bed, &references)?)
    } else if let Some(region) = options.region.as_deref() {
        Some(vec![parse_depth_region(region, &references)?])
    } else {
        None
    };
    let mut all_depths = Vec::with_capacity(inputs.len());
    let mut used = vec![false; references.len()];
    for input in &inputs {
        let (depths, input_used) = collect_depth(input, &references, regions.as_deref(), &options)?;
        all_depths.push(depths);
        for (used, input_used) in used.iter_mut().zip(input_used) {
            *used |= input_used;
        }
    }

    let mut out = open_writer(options.output.as_deref())?;
    if options.header {
        write!(out, "#CHROM\tPOS")
            .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        for input in &inputs {
            write!(out, "\t{}", input.display())
                .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        }
        writeln!(out).map_err(|error| Error::io(format!("cannot write output: {error}")))?;
    }
    for (tid, (name, length)) in references.iter().enumerate() {
        let mut positions = BTreeSet::new();
        if let Some(regions) = &regions {
            if options.all_positions
                && (options.absolutely_all || options.region.is_some() || used[tid])
            {
                for region in regions.iter().filter(|region| region.tid == tid) {
                    positions.extend(region.start..=region.end);
                }
            } else if !options.all_positions {
                for depths in &all_depths {
                    for position in depths[tid].keys().copied() {
                        if position_in_regions(position, tid, Some(regions)) {
                            positions.insert(position);
                        }
                    }
                }
            }
        } else if options.all_positions {
            if options.absolutely_all || used[tid] {
                positions.extend(1..=*length);
            }
        } else {
            for depths in &all_depths {
                positions.extend(depths[tid].keys().copied());
            }
        }
        for position in positions {
            write!(out, "{name}\t{position}")
                .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
            for depths in &all_depths {
                let value = depths[tid].get(&position).copied().unwrap_or(0);
                write!(out, "\t{value}")
                    .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
            }
            writeln!(out).map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        }
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

fn format_significant(value: f64, digits: usize) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        if value.is_nan() {
            return if value.is_sign_negative() {
                "-nan".to_string()
            } else {
                "nan".to_string()
            };
        }
        return if value.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    let exponent = value.abs().log10().floor() as i32;
    if exponent < -4 || exponent >= digits as i32 {
        let decimals = digits.saturating_sub(1);
        let text = format!("{value:.decimals$e}");
        let (mantissa, exponent) = text.split_once('e').expect("scientific format");
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exponent: i32 = exponent.parse().expect("formatted exponent");
        return format!("{mantissa}e{exponent:+03}");
    }
    let decimals = (digits as i32 - exponent - 1).max(0) as usize;
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    text
}

#[derive(Clone, Copy, Default)]
struct CoveragePosition {
    depth: u32,
    baseq_sum: u64,
    baseq_count: u64,
}

struct CoverageStats {
    positions: Vec<BTreeMap<usize, CoveragePosition>>,
    reads: Vec<u64>,
    all_reads: Vec<u64>,
    seen: Vec<bool>,
    mapq_sum: Vec<u64>,
}

impl CoverageStats {
    fn new(references: usize) -> Self {
        Self {
            positions: (0..references).map(|_| BTreeMap::new()).collect(),
            reads: vec![0; references],
            all_reads: vec![0; references],
            seen: vec![false; references],
            mapq_sum: vec![0; references],
        }
    }
}

fn resolve_inputs(
    inputs: &[PathBuf],
    list: Option<&PathBuf>,
    command: &str,
) -> Result<Vec<PathBuf>> {
    let inputs = if let Some(path) = list {
        let text = std::fs::read_to_string(path)
            .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()
    } else {
        inputs.to_vec()
    };
    if inputs.is_empty() {
        return Err(Error::usage(format!(
            "{command} needs at least one input file or a file list"
        )));
    }
    Ok(inputs)
}

fn coverage_inputs(options: &CoverageOptions) -> Result<Vec<PathBuf>> {
    resolve_inputs(&options.inputs, options.bam_list.as_ref(), "coverage")
}

fn accumulate_coverage(
    input: &Path,
    references: &[(String, usize)],
    region: Option<&DepthRegion>,
    options: &CoverageOptions,
    stats: &mut CoverageStats,
) -> Result<()> {
    let mut reader = AlignmentReader::open(input)?;
    let header = reader.read_header()?;
    let actual: Vec<(String, usize)> = header
        .reference_sequences()
        .iter()
        .map(|(name, sequence)| {
            (
                String::from_utf8_lossy(name.as_ref()).into_owned(),
                sequence.length().get(),
            )
        })
        .collect();
    if actual != references {
        return Err(Error::input(
            "coverage inputs must have identical reference names and lengths".to_string(),
        ));
    }

    let mut start_counts: HashMap<(usize, usize), usize> = HashMap::new();
    let mut record = Record::default();
    loop {
        if reader.read_record(&header, &mut record)? == 0 {
            break;
        }
        let Some(tid) = record.reference_sequence_id() else {
            continue;
        };
        if tid >= references.len() {
            continue;
        }
        let region_match = match region {
            None => true,
            Some(region) if tid != region.tid => false,
            Some(region) => match record.alignment_start() {
                None => false,
                Some(start) => {
                    let end = record
                        .alignment_end()
                        .map(usize::from)
                        .unwrap_or(usize::from(start));
                    usize::from(start) <= region.end && end >= region.start
                }
            },
        };
        if !region_match {
            continue;
        }
        // samtools reports filtered reads in histogram mode. This counter is
        // before flag/MAPQ/length filtering but after the region iterator.
        stats.all_reads[tid] += 1;
        let flags = u32::from(record.flags().bits());
        let excluded = options.filter_flags.unwrap_or(0x704);
        if flags & excluded != 0 {
            continue;
        }
        if options.required_flags != 0 && flags & options.required_flags == 0 {
            continue;
        }
        if record
            .mapping_quality()
            .map(|quality| quality.get())
            .unwrap_or(0)
            < options.min_mapq
        {
            continue;
        }
        if options.min_read_len > 0 && record.sequence().len() < options.min_read_len {
            continue;
        }
        let Some(start) = record.alignment_start() else {
            continue;
        };
        if let Some(region) = region {
            if tid != region.tid {
                continue;
            }
            let end = record
                .alignment_end()
                .map(usize::from)
                .unwrap_or(usize::from(start));
            if usize::from(start) > region.end || end < region.start {
                continue;
            }
        }

        stats.reads[tid] += 1;
        stats.mapq_sum[tid] += u64::from(record.mapping_quality().map(|q| q.get()).unwrap_or(0));
        stats.seen[tid] = true;

        // htslib's pileup cap limits reads at an alignment start per input.
        // It does not cap the final merged depth, and it does not reduce the
        // read/MAPQ counters above.
        let start_key = (tid, usize::from(start));
        let start_count = start_counts.entry(start_key).or_insert(0);
        let skip_for_depth = options.max_depth > 0 && *start_count >= options.max_depth;
        *start_count += 1;
        if skip_for_depth {
            continue;
        }

        let mut reference_position = usize::from(start);
        let mut read_position = 0usize;
        let qualities = record.quality_scores().as_ref();
        for op in record.cigar().iter() {
            let op = op.map_err(|error| Error::runtime(format!("invalid CIGAR: {error}")))?;
            let length = op.len();
            match op.kind() {
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => {
                    for offset in 0..length {
                        let position = reference_position + offset;
                        if !position_in_region(position, region) {
                            continue;
                        }
                        let quality = qualities.get(read_position + offset).copied().unwrap_or(0);
                        if quality >= options.min_baseq {
                            let entry = stats.positions[tid].entry(position).or_default();
                            entry.depth += 1;
                            entry.baseq_sum += u64::from(quality);
                            entry.baseq_count += 1;
                        }
                    }
                    reference_position += length;
                    read_position += length;
                }
                Kind::Insertion | Kind::SoftClip => read_position += length,
                Kind::Deletion | Kind::Skip => reference_position += length,
                Kind::HardClip | Kind::Pad => {}
            }
        }
    }
    Ok(())
}

/// `coverage`: per-reference summary of aligned-read depth.
///
/// Multiple inputs are merged by position. `--min-depth` filters coverage,
/// mean-depth and base-quality statistics after that merge; read counts and
/// mapQ remain read-level statistics. Memory grows with covered positions.
pub fn coverage(options: CoverageOptions) -> Result<()> {
    let inputs = coverage_inputs(&options)?;
    let first = inputs.first().expect("non-empty inputs were validated");
    let mut first_reader = AlignmentReader::open(first)?;
    let first_header = first_reader.read_header()?;
    let references: Vec<(String, usize)> = first_header
        .reference_sequences()
        .iter()
        .map(|(name, sequence)| {
            (
                String::from_utf8_lossy(name.as_ref()).into_owned(),
                sequence.length().get(),
            )
        })
        .collect();
    let region = options
        .region
        .as_deref()
        .map(|text| parse_depth_region(text, &references))
        .transpose()?;
    let mut stats = CoverageStats::new(references.len());
    for input in &inputs {
        accumulate_coverage(input, &references, region.as_ref(), &options, &mut stats)?;
    }

    let min_depth = options.min_depth.max(1);
    if options.histogram || options.plot_depth || options.n_bins.is_some() || options.ascii {
        return write_coverage_histogram(&options, &references, region.as_ref(), &stats);
    }
    let mut out = open_writer(options.output.as_deref())?;
    if !options.no_header {
        writeln!(
            out,
            "#rname\tstartpos\tendpos\tnumreads\tcovbases\tcoverage\tmeandepth\tmeanbaseq\tmeanmapq"
        )
        .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
    }
    for (tid, (name, length)) in references.iter().enumerate() {
        let (span_start, span_end) = match &region {
            Some(region) => {
                if tid != region.tid {
                    continue;
                }
                (region.start, region.end)
            }
            None => (1, *length),
        };
        let span = span_end.saturating_sub(span_start).saturating_add(1);
        if span == 0 {
            continue;
        }
        let mut covbases = 0u64;
        let mut depth_sum = 0u64;
        let mut baseq_sum = 0u64;
        let mut baseq_count = 0u64;
        for (position, value) in &stats.positions[tid] {
            if *position >= span_start && *position <= span_end && value.depth >= min_depth {
                covbases += 1;
                depth_sum += u64::from(value.depth);
                baseq_sum += value.baseq_sum;
                baseq_count += value.baseq_count;
            }
        }
        let coverage = (covbases as f64) / (span as f64) * 100.0;
        let mean_depth = (depth_sum as f64) / (span as f64);
        let mean_baseq = if baseq_count > 0 {
            (baseq_sum as f64) / (baseq_count as f64)
        } else {
            0.0
        };
        let mean_mapq = if stats.reads[tid] > 0 {
            (stats.mapq_sum[tid] as f64) / (stats.reads[tid] as f64)
        } else {
            0.0
        };
        writeln!(
            out,
            "{name}\t{span_start}\t{span_end}\t{}\t{covbases}\t{}\t{}\t{}\t{}",
            stats.reads[tid],
            format_significant(coverage, 6),
            format_significant(mean_depth, 6),
            format_significant(mean_baseq, 3),
            format_significant(mean_mapq, 3)
        )
        .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// coverage histogram
// --------------------------------------------------------------------------- //

fn readable_bps(value: f64) -> String {
    const UNITS: [&str; 5] = ["", "K", "M", "G", "T"];
    let mut value = value;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.unit$}{}", UNITS[unit])
}

fn centered_text(text: &str, width: usize) -> String {
    let length = text.len();
    if length >= width {
        return text.to_string();
    }
    let padding = (width - length) / 2;
    let extra = (width - length) % 2;
    if padding >= 1 {
        format!(
            " {:>width$}{}",
            text,
            " ".repeat(padding - 1 + extra),
            width = length + padding
        )
    } else {
        text.to_string()
    }
}

fn histogram_bin_count(options: &CoverageOptions) -> usize {
    let requested = options.n_bins.unwrap_or(50);
    let full_width = options.n_bins.is_none() && options.output.is_none();
    if requested == 0 || full_width {
        let columns = std::env::var("COLUMNS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        if columns > 60 {
            columns - 40
        } else {
            40
        }
    } else {
        requested
    }
}

fn histogram_glyph(ascii: bool, value: f64, current: f64, row_size: f64) -> char {
    if row_size == 0.0 {
        return ' ';
    }
    let length = if ascii { 2 } else { 8 };
    let difference = (length as f64 * (value - current) / row_size).round() as i32 - 1;
    if difference < 0 {
        return ' ';
    }
    if ascii {
        ['.', ':'][difference.min(length - 1) as usize]
    } else {
        ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][difference.min(length - 1) as usize]
    }
}

#[allow(clippy::too_many_arguments)]
fn render_coverage_histogram(
    name: &str,
    reference_length: usize,
    beg: usize,
    end: usize,
    bins: usize,
    bin_width: usize,
    histogram: &[u64],
    plot_depth: bool,
    ascii: bool,
    reads: u64,
    all_reads: u64,
    covered_bases: u64,
    depth_sum: u64,
    baseq_sum: u64,
    baseq_count: u64,
    mapq_sum: u64,
) -> String {
    let span = (end - beg) as f64;
    let factor = if plot_depth { 1.0 } else { 100.0 };
    let values: Vec<f64> = histogram
        .iter()
        .map(|value| factor * (*value as f64) / bin_width as f64)
        .collect();
    let maximum = values.iter().copied().fold(0.0_f64, f64::max);
    let row_size = maximum / 10.0;
    let vertical = if ascii { '|' } else { '│' };
    let mut output = format!("{name} ({}bp)\n", readable_bps(reference_length as f64));
    let coverage = 100.0 * covered_bases as f64 / span;
    let mean_depth = depth_sum as f64 / span;
    let mean_baseq = if baseq_count > 0 {
        baseq_sum as f64 / baseq_count as f64
    } else {
        0.0
    };
    let mean_mapq = if reads > 0 {
        mapq_sum as f64 / reads as f64
    } else {
        -f64::NAN
    };

    for row in (0..10).rev() {
        let current = row_size * row as f64;
        if plot_depth {
            output.push_str(&format!(">{current:>8.1} "));
        } else {
            output.push_str(&format!(">{current:>7.2}% "));
        }
        output.push(vertical);
        for value in &values {
            output.push(histogram_glyph(ascii, *value, current, row_size));
        }
        output.push(vertical);
        output.push(' ');
        match row {
            9 => output.push_str(&format!("Number of reads: {reads}")),
            8 if all_reads > reads => {
                output.push_str(&format!("    ({} filtered)", all_reads - reads))
            }
            7 => output.push_str(&format!(
                "Covered bases:   {}bp",
                readable_bps(covered_bases as f64)
            )),
            6 => output.push_str(&format!(
                "Percent covered: {}%",
                format_significant(coverage, 4)
            )),
            5 => output.push_str(&format!(
                "Mean coverage:   {}x",
                format_significant(mean_depth, 3)
            )),
            4 => output.push_str(&format!(
                "Mean baseQ:      {}",
                format_significant(mean_baseq, 3)
            )),
            3 => output.push_str(&format!(
                "Mean mapQ:       {}",
                format_significant(mean_mapq, 3)
            )),
            1 => output.push_str(&format!(
                "Histo bin width: {}bp",
                readable_bps(bin_width as f64)
            )),
            0 if plot_depth => output.push_str(&format!(
                "Histo max cov:   {}",
                format_significant(maximum, 5)
            )),
            0 => output.push_str(&format!(
                "Histo max bin:   {}%",
                format_significant(maximum, 5)
            )),
            _ => {}
        }
        output.push('\n');
    }

    output.push_str("     ");
    output.push_str(&centered_text(&readable_bps((beg + 1) as f64), 10));
    let mut rest = 10;
    while rest < 10 * (bins / 10) {
        output.push_str(&centered_text(
            &readable_bps((beg + bin_width * rest) as f64),
            10,
        ));
        rest += 10;
    }
    // The upstream `%*s` receives a one-space string, so width zero still
    // emits one space before the final tick label.
    output.push_str(&" ".repeat((bins % 10).max(1)));
    output.push_str(&centered_text(&readable_bps(end as f64), 10));
    output.push('\n');
    output
}

fn write_coverage_histogram(
    options: &CoverageOptions,
    references: &[(String, usize)],
    region: Option<&DepthRegion>,
    stats: &CoverageStats,
) -> Result<()> {
    let requested_bins = histogram_bin_count(options);
    let min_depth = options.min_depth.max(1);
    let mut out = open_writer(options.output.as_deref())?;
    let mut wrote = false;

    for (tid, (name, length)) in references.iter().enumerate() {
        if let Some(region) = region {
            if tid != region.tid {
                continue;
            }
        } else if !stats.seen[tid] {
            continue;
        }
        let (beg, end) = match region {
            Some(region) => (region.start - 1, region.end),
            None => (0, *length),
        };
        let span = end.saturating_sub(beg);
        if span == 0 {
            continue;
        }
        let bins = requested_bins.min(span);
        let bin_width = span / bins;
        let mut histogram = vec![0u64; bins];
        let mut covered_bases = 0u64;
        let mut depth_sum = 0u64;
        let mut baseq_sum = 0u64;
        let mut baseq_count = 0u64;
        for (position, value) in &stats.positions[tid] {
            let position0 = position - 1;
            if position0 < beg || position0 >= end {
                continue;
            }
            if value.depth >= min_depth {
                covered_bases += 1;
                depth_sum += u64::from(value.depth);
                baseq_sum += value.baseq_sum;
                baseq_count += value.baseq_count;
            }
            let bin = (position0 - beg) / bin_width;
            if bin >= bins {
                continue;
            }
            if options.plot_depth {
                histogram[bin] += u64::from(value.depth);
            } else if value.depth >= min_depth {
                histogram[bin] += 1;
            }
        }
        if wrote {
            out.write_all(b"\n")
                .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        }
        let text = render_coverage_histogram(
            name,
            *length,
            beg,
            end,
            bins,
            bin_width,
            &histogram,
            options.plot_depth,
            options.ascii,
            stats.reads[tid],
            stats.all_reads[tid],
            covered_bases,
            depth_sum,
            baseq_sum,
            baseq_count,
            stats.mapq_sum[tid],
        );
        out.write_all(text.as_bytes())
            .map_err(|error| Error::io(format!("cannot write output: {error}")))?;
        wrote = true;
    }
    out.flush()
        .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// flagstat
// --------------------------------------------------------------------------- //

/// `flagstat`: the 16 counters samtools prints, in its order and wording.
pub fn flagstat(input: &Path) -> Result<String> {
    let mut reader = AlignmentReader::open(input)?;
    let header = reader.read_header()?;

    let names = reference_names(&header);
    let mut stat = FlagStat::default();
    let mut record = Record::default();
    loop {
        let bytes = reader.read_record(&header, &mut record)?;
        if bytes == 0 {
            break;
        }
        let flags = record.flags();
        let reference = tolerant_reference_id(&record).and_then(|id| names.get(id));
        let mate_reference = record
            .mate_reference_sequence_id()
            .and_then(|id| names.get(id));
        let mapping_quality = record.mapping_quality().map(|q| q.get());
        stat.add(
            flags,
            reference.map(String::as_str),
            mate_reference.map(String::as_str),
            mapping_quality,
        );
    }
    Ok(render_flagstat(&stat))
}

/// Render `FlagStat` in `samtools flagstat`'s exact wording.
///
/// The `+ 0` halves are the QC-failed counts. `rust-bam` has no `-f` QC filter,
/// so the failed column is always zero, which is what samtools prints with no
/// filter applied.
pub fn render_flagstat(stat: &FlagStat) -> String {
    let mut out = String::new();
    // A plain free function rather than a closure: a `FnMut` capturing `out`
    // would hold a mutable borrow across the `out.push_str` calls below.
    fn line(out: &mut String, label: &str, passed: u64, failed: u64) {
        out.push_str(&format!("{passed} + {failed} {label}\n"));
    }
    line(
        &mut out,
        "in total (QC-passed reads + QC-failed reads)",
        stat.total,
        0,
    );
    line(&mut out, "primary", stat.primary, 0);
    line(&mut out, "secondary", stat.secondary, 0);
    line(&mut out, "supplementary", stat.supplementary, 0);
    line(&mut out, "duplicates", stat.duplicates, 0);
    line(&mut out, "primary duplicates", stat.primary_duplicates, 0);

    // The percentages: samtools prints N/A when the denominator is zero.
    let pct = |numerator: u64, denominator: u64| -> String {
        if denominator == 0 {
            "N/A".to_string()
        } else {
            format!("{:.2}%", numerator as f64 * 100.0 / denominator as f64)
        }
    };
    out.push_str(&format!(
        "{0} + 0 mapped ({1} : N/A)\n",
        stat.mapped,
        pct(stat.mapped, stat.total)
    ));
    out.push_str(&format!(
        "{0} + 0 primary mapped ({1} : N/A)\n",
        stat.primary_mapped,
        pct(stat.primary_mapped, stat.primary)
    ));
    line(&mut out, "paired in sequencing", stat.paired, 0);
    line(&mut out, "read1", stat.read1, 0);
    line(&mut out, "read2", stat.read2, 0);
    out.push_str(&format!(
        "{0} + 0 properly paired ({1} : N/A)\n",
        stat.properly_paired,
        pct(stat.properly_paired, stat.paired)
    ));
    line(
        &mut out,
        "with itself and mate mapped",
        stat.with_itself_and_mate_mapped,
        0,
    );
    out.push_str(&format!(
        "{0} + 0 singletons ({1} : N/A)\n",
        stat.singletons,
        pct(stat.singletons, stat.paired)
    ));
    line(
        &mut out,
        "with mate mapped to a different chr",
        stat.mate_different_chr,
        0,
    );
    line(
        &mut out,
        "with mate mapped to a different chr (mapQ>=5)",
        stat.mate_different_chr_mapq5,
        0,
    );
    out
}

// --------------------------------------------------------------------------- //
// idxstats
// --------------------------------------------------------------------------- //

/// `idxstats`: per-reference mapped/unmapped counts plus the trailing `*` line.
pub fn idxstats(input: &Path) -> Result<String> {
    let mut reader = AlignmentReader::open(input)?;
    let header = reader.read_header()?;

    let mut stats = IdxStats::from_references(header.reference_sequences().iter().map(
        |(name, reference)| {
            (
                String::from_utf8_lossy(name.as_ref()).into_owned(),
                reference.length().get() as u64,
            )
        },
    ));

    let mut record = Record::default();
    loop {
        let bytes = reader.read_record(&header, &mut record)?;
        if bytes == 0 {
            break;
        }
        let is_unmapped = record.flags().is_unmapped();
        let reference_id = tolerant_reference_id(&record);
        stats.add(reference_id, is_unmapped);
    }
    Ok(render_idxstats(&stats))
}

/// Render `IdxStats` in `samtools idxstats`'s format: tab-separated, one line per
/// reference, then the `*` line.
pub fn render_idxstats(stats: &IdxStats) -> String {
    let mut out = String::new();
    for reference in &stats.references {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            reference.name, reference.length, reference.mapped, reference.unmapped
        ));
    }
    out.push_str(&format!(
        "*\t0\t{}\t{}\n",
        stats.no_coordinate.0, stats.no_coordinate.1
    ));
    out
}

// --------------------------------------------------------------------------- //
// quickcheck
// --------------------------------------------------------------------------- //

/// The BGZF EOF marker, as htslib compares it.
///
/// htslib checks the **final 28 bytes** against this exact pattern, `XFL = 0`
/// included. A stream whose marker differs in one byte fails — the defect W1-4
/// found in `bioinfo-fastx`, where gzip `XFL` was set to 2 from the compression
/// level and `samtools quickcheck` exited 16 while `samtools view` still worked.
pub const BGZF_EOF_MARKER: [u8; 28] = [
    0x1f, 0x8b, 0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x06, 0x00, 0x42, 0x43, 0x02, 0x00,
    0x1b, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The result of a quickcheck run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickCheck {
    /// Files that passed.
    pub passed: Vec<PathBuf>,
    /// `(file, reason)` for each failure.
    pub failed: Vec<(PathBuf, String)>,
}

impl QuickCheck {
    /// True when every file passed.
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }

    /// `samtools quickcheck`'s exit code: 0 on success, 16 on any failure.
    pub fn exit_code(&self) -> i32 {
        if self.ok() {
            0
        } else {
            16
        }
    }
}

/// `quickcheck`: cheap structural validation without decoding every record.
///
/// Mirrors what `samtools quickcheck` verifies:
///
/// * the file is non-empty;
/// * a BAM has the BAM magic;
/// * a BAM ends with the 28-byte BGZF EOF marker;
/// * the header can be read and declares at least one reference sequence
///   (samtools warns "has no targets" and fails).
pub fn quickcheck(paths: &[PathBuf]) -> Result<QuickCheck> {
    let mut result = QuickCheck {
        passed: Vec::new(),
        failed: Vec::new(),
    };

    for path in paths {
        match quickcheck_one(path) {
            Ok(()) => result.passed.push(path.clone()),
            Err(reason) => result.failed.push((path.clone(), reason)),
        }
    }
    Ok(result)
}

fn quickcheck_one(path: &Path) -> std::result::Result<(), String> {
    let metadata = std::fs::metadata(path).map_err(|error| format!("{error}"))?;
    if metadata.len() == 0 {
        return Err("is empty".to_string());
    }

    let format = detect_path(path).map_err(|error| error.message().to_string())?;
    if format == Format::Cram {
        return Err("is CRAM, which this build does not support".to_string());
    }

    let mut reader = AlignmentReader::open(path).map_err(|error| error.message().to_string())?;
    let header = reader
        .read_header()
        .map_err(|error| format!("cannot read header: {}", error.message()))?;

    if header.reference_sequences().is_empty() {
        return Err("has no targets in header".to_string());
    }

    // The EOF marker check applies to BAM only: it is a BGZF concept.
    if format == Format::Bam {
        check_bgzf_eof(path)?;
    }

    Ok(())
}

/// Verify the final 28 bytes are htslib's exact BGZF EOF marker.
fn check_bgzf_eof(path: &Path) -> std::result::Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path).map_err(|error| format!("{error}"))?;
    let length = file
        .seek(SeekFrom::End(0))
        .map_err(|error| format!("{error}"))?;
    if length < BGZF_EOF_MARKER.len() as u64 {
        return Err("is truncated: shorter than a BGZF EOF block".to_string());
    }
    file.seek(SeekFrom::End(-(BGZF_EOF_MARKER.len() as i64)))
        .map_err(|error| format!("{error}"))?;
    let mut tail = [0u8; BGZF_EOF_MARKER.len()];
    file.read_exact(&mut tail)
        .map_err(|error| format!("{error}"))?;
    if tail != BGZF_EOF_MARKER {
        return Err("was missing EOF block when one should be present".to_string());
    }
    Ok(())
}

// --------------------------------------------------------------------------- //
// index
// --------------------------------------------------------------------------- //

/// `index`: write a BAI next to the BAM.
///
/// Returns the path written and the number of records indexed.
pub fn index(input: &Path, output: Option<PathBuf>, csi: bool) -> Result<(PathBuf, u64)> {
    let format = detect_path(input)?;
    if format != Format::Bam {
        return Err(Error::usage(format!(
            "index needs a BAM (BGZF) file; {} is {}",
            input.display(),
            format.as_str()
        )));
    }
    let output = output.unwrap_or_else(|| {
        if csi {
            index_util::csi_path(input)
        } else {
            index_util::bai_path(input)
        }
    });
    let count = if csi {
        index_util::build_csi(input, &output)?
    } else {
        index_util::build_bai(input, &output)?
    };
    Ok((output, count))
}

// --------------------------------------------------------------------------- //
// sort
// --------------------------------------------------------------------------- //

/// Default memory budget for one in-memory sort chunk.
pub const DEFAULT_SORT_MEMORY: u64 = 256 * 1024 * 1024;
/// Default maximum number of sorted runs opened at once.
pub const DEFAULT_MAX_OPEN_FILES: usize = 64;

/// Options for `sort`.
pub struct SortOptions {
    /// Input path.
    pub input: PathBuf,
    /// Output path; when `None` the result goes to standard output as BAM.
    pub output: Option<PathBuf>,
    /// Write SAM text instead of BAM.
    pub sam_output: bool,
    /// Approximate bytes of records held in one sort chunk. `None` uses
    /// [`DEFAULT_SORT_MEMORY`].
    pub max_memory: Option<u64>,
    /// Maximum number of run files opened during one merge pass.
    pub max_open_files: usize,
    /// `-@`: worker threads for temporary/final BAM compression; 0/1 are serial.
    pub threads: usize,
}

/// A temporary directory that removes its runs even when a later step fails.
struct TempDir {
    path: PathBuf,
    next_run: u64,
    workers: usize,
}

impl TempDir {
    fn new(workers: usize) -> Result<TempDir> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "rust-bam-sort-{}-{}-{}",
            std::process::id(),
            nanos,
            TEMP_COUNTER.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        std::fs::create_dir_all(&path).map_err(|error| {
            Error::io(format!(
                "cannot create sort workspace {}: {error}",
                path.display()
            ))
        })?;
        Ok(TempDir {
            path,
            next_run: 0,
            workers,
        })
    }

    fn run_path(&mut self) -> PathBuf {
        let part = self.next_run;
        self.next_run += 1;
        self.path.join(format!("run-{part}.bam"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn key_for(record: &Record, index: usize) -> SortKey {
    SortKey::new(
        record.flags(),
        tolerant_reference_id(record),
        record.alignment_start().map(usize::from),
        record.name().map(|name| name.as_ref()).unwrap_or(&[]),
        index,
    )
}

fn record_memory(record: &Record) -> u64 {
    let name = record
        .name()
        .map(|name| {
            let bytes: &[u8] = name.as_ref();
            bytes.len()
        })
        .unwrap_or(0);
    let sequence = record.sequence().len();
    let qualities = record.quality_scores().len();
    (std::mem::size_of::<Record>() + name + sequence + qualities + 256) as u64
}

fn sorted_indices(chunk: &[(SortKey, Record)]) -> Vec<usize> {
    let mut order: Vec<_> = (0..chunk.len()).collect();
    // Keys include the unique input index, so unstable sorting of these small
    // indices yields the same total order without moving large RecordBufs or
    // allocating a stable-sort scratch array of records.
    order.sort_unstable_by(|&a, &b| chunk[a].0.cmp(&chunk[b].0));
    order
}

fn write_index(file: &mut impl Write, index: usize) -> Result<()> {
    let index = u64::try_from(index)
        .map_err(|_| Error::runtime("record index does not fit in u64".to_string()))?;
    file.write_all(&index.to_le_bytes())
        .map_err(|error| Error::io(format!("cannot write sort index: {error}")))
}

fn flush_index(file: &mut impl Write) -> Result<()> {
    // Buffered writes can first fail here. Keep the diagnostic used when the
    // same underlying write failed immediately in write_index.
    file.flush()
        .map_err(|error| Error::io(format!("cannot write sort index: {error}")))
}

fn spill_run(
    temp: &mut TempDir,
    header: &sam::Header,
    chunk: &mut Vec<(SortKey, Record)>,
) -> Result<PathBuf> {
    let path = temp.run_path();
    let index_path = path.with_extension("idx");
    let order = sorted_indices(chunk);

    let file = File::create(&path)
        .map_err(|error| Error::io(format!("cannot create {}: {error}", path.display())))?;
    let index_file = File::create(&index_path)
        .map_err(|error| Error::io(format!("cannot create {}: {error}", index_path.display())))?;
    // Sidecars contain one eight-byte index per record. Buffer those writes;
    // flushing below still propagates errors before a run is published.
    let mut index_file = BufWriter::with_capacity(64 * 1024, index_file);
    let mut writer = crate::sort_run_writer::RunWriter::new(file, temp.workers);
    writer.write_header(header)?;
    for position in order {
        let (key, record) = &chunk[position];
        writer.write_record(header, record)?;
        write_index(&mut index_file, key.input_index)?;
    }
    let file = writer.finish()?;
    file.sync_all()
        .map_err(|error| Error::io(format!("cannot sync {}: {error}", path.display())))?;
    flush_index(&mut index_file)?;
    // Drop record storage, but keep the chunk's backing allocation for the
    // next spill. Run boundaries and write/sync error order remain unchanged.
    chunk.clear();
    Ok(path)
}

// Capture the exact bytes consumed by the full decoder. Internal runs were
// serialized once by BamWriter, so validated frames can be forwarded without
// repeating field encoding. This is deliberately confined to sort run readers:
// public inputs (including flagstat) retain their existing reader and checks.
const MAX_CACHED_SORT_FRAME: usize = 64 * 1024;

struct CaptureReader<R> {
    inner: R,
    frame: Vec<u8>,
    capture: bool,
}

impl<R: Read> Read for CaptureReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        if self.capture {
            let prefix = (4usize.saturating_sub(self.frame.len())).min(count);
            self.frame.extend_from_slice(&buffer[..prefix]);
            if self.frame.len() >= 4 {
                let size =
                    u32::from_le_bytes(self.frame[..4].try_into().expect("four bytes")) as u64 + 4;
                if size > MAX_CACHED_SORT_FRAME as u64
                    || self.frame.len() + count - prefix > MAX_CACHED_SORT_FRAME
                {
                    self.capture = false;
                    self.frame.clear();
                }
            }
            if self.capture {
                self.frame.extend_from_slice(&buffer[prefix..count]);
            }
        }
        Ok(count)
    }
}

type CapturedBamReader = noodles::bam::io::Reader<
    CaptureReader<noodles::bgzf::io::Reader<BufReader<Box<dyn Read + Send>>>>,
>;

enum RunInput {
    Bam(CapturedBamReader),
    Other(AlignmentReader),
}

impl RunInput {
    fn read_record(
        &mut self,
        header: &sam::Header,
        record: &mut Record,
        frame: &mut Vec<u8>,
    ) -> Result<usize> {
        frame.clear();
        match self {
            Self::Bam(reader) => {
                std::mem::swap(frame, &mut reader.get_mut().frame);
                reader.get_mut().frame.clear();
                reader.get_mut().capture = true;
                // Keep the full decoder, including auxiliary-tag duplicates,
                // long-CIGAR validation, and diagnostic precedence.
                let result = reader.read_record_buf(header, record);
                std::mem::swap(frame, &mut reader.get_mut().frame);
                result.map_err(|error| Error::runtime(format!("cannot read BAM record: {error}")))
            }
            Self::Other(reader) => reader.read_record(header, record),
        }
    }
}

struct RunReader {
    label: PathBuf,
    reader: RunInput,
    header: sam::Header,
    indices: BufReader<File>,
}

fn open_run(path: &Path) -> Result<RunReader> {
    let mut reader = AlignmentReader::open(path)?;
    let header = reader.read_header()?;
    let reader = match reader {
        AlignmentReader::Bam(reader) => {
            RunInput::Bam(noodles::bam::io::Reader::from(CaptureReader {
                inner: reader.into_inner(),
                frame: Vec::new(),
                capture: true,
            }))
        }
        other => RunInput::Other(other),
    };
    let index_path = path.with_extension("idx");
    let file = File::open(&index_path)
        .map_err(|error| Error::io(format!("cannot open {}: {error}", index_path.display())))?;
    Ok(RunReader {
        label: path.to_path_buf(),
        reader,
        header,
        indices: BufReader::new(file),
    })
}

fn next_from_run(
    run: &mut RunReader,
    record: &mut Record,
    key: &mut SortKey,
    frame: &mut Vec<u8>,
) -> Result<bool> {
    let mut bytes = [0u8; 8];
    let mut filled = 0;
    while filled < bytes.len() {
        match run.indices.read(&mut bytes[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(Error::io(format!(
                    "cannot read sort index {}: {error}",
                    run.label.display()
                )))
            }
        }
    }
    if filled == 0 {
        return Ok(false);
    }
    if filled != bytes.len() {
        return Err(Error::runtime(format!(
            "truncated sort index {}",
            run.label.display()
        )));
    }
    let index = usize::try_from(u64::from_le_bytes(bytes))
        .map_err(|_| Error::runtime("sort index does not fit in usize".to_string()))?;
    if run.reader.read_record(&run.header, record, frame)? == 0 {
        return Err(Error::runtime(format!(
            "sort run {} ended before its index",
            run.label.display()
        )));
    }
    key.update(
        record.flags(),
        tolerant_reference_id(record),
        record.alignment_start().map(usize::from),
        record.name().map(|name| name.as_ref()).unwrap_or(&[]),
        index,
    );
    Ok(true)
}

struct MergeItem {
    key: SortKey,
    record: Record,
    frame: Vec<u8>,
}

struct RunMerge {
    readers: Vec<RunReader>,
    heads: Vec<Option<MergeItem>>,
    tree: crate::merge_tree::MergeTree,
}

fn compare_heads(heads: &[Option<MergeItem>], a: usize, b: usize) -> std::cmp::Ordering {
    heads[a]
        .as_ref()
        .expect("active merge head")
        .key
        .cmp(&heads[b].as_ref().expect("active merge head").key)
}

impl RunMerge {
    fn refill(&mut self, run: usize) -> Result<()> {
        let item = self.heads[run].as_mut().expect("winning merge head");
        if !next_from_run(
            &mut self.readers[run],
            &mut item.record,
            &mut item.key,
            &mut item.frame,
        )? {
            self.heads[run] = None;
        }
        self.tree.replace(run, self.heads[run].is_some(), |a, b| {
            compare_heads(&self.heads, a, b)
        });
        Ok(())
    }
}

fn open_runs(paths: &[PathBuf]) -> Result<RunMerge> {
    let mut readers = Vec::with_capacity(paths.len());
    let mut heads = Vec::with_capacity(paths.len());
    for path in paths {
        let mut reader = open_run(path)?;
        let mut record = Record::default();
        let mut key = key_for(&record, 0);
        let mut frame = Vec::new();
        let head = if next_from_run(&mut reader, &mut record, &mut key, &mut frame)? {
            Some(MergeItem { key, record, frame })
        } else {
            None
        };
        heads.push(head);
        readers.push(reader);
    }
    let tree = crate::merge_tree::MergeTree::new(
        heads.len(),
        |i| heads[i].is_some(),
        |a, b| compare_heads(&heads, a, b),
    );
    Ok(RunMerge {
        readers,
        heads,
        tree,
    })
}

fn merge_to_run(temp: &mut TempDir, paths: &[PathBuf], header: &sam::Header) -> Result<PathBuf> {
    let mut merge = open_runs(paths)?;
    let path = temp.run_path();
    let index_path = path.with_extension("idx");
    let file = File::create(&path)
        .map_err(|error| Error::io(format!("cannot create {}: {error}", path.display())))?;
    let index_file = File::create(&index_path)
        .map_err(|error| Error::io(format!("cannot create {}: {error}", index_path.display())))?;
    let mut index_file = BufWriter::with_capacity(64 * 1024, index_file);
    let mut writer = crate::sort_run_writer::RunWriter::new(file, temp.workers);
    writer.write_header(header)?;
    while let Some(run) = merge.tree.winner() {
        let item = merge.heads[run].as_mut().expect("winning merge head");
        if item.frame.is_empty() {
            writer.write_record(header, &item.record)?;
        } else {
            writer.write_encoded_record(&item.frame)?;
        }
        write_index(&mut index_file, item.key.input_index)?;
        if item.frame.is_empty() {
            // Large records use the ordinary encoder; do not retain their
            // allocations in run buffers after they have been written.
            item.record = Record::default();
        }
        merge.refill(run)?;
    }
    let file = writer.finish()?;
    file.sync_all()
        .map_err(|error| Error::io(format!("cannot sync {}: {error}", path.display())))?;
    flush_index(&mut index_file)?;
    Ok(path)
}

fn reduce_runs(
    temp: &mut TempDir,
    mut runs: Vec<PathBuf>,
    max_open: usize,
    header: &sam::Header,
) -> Result<Vec<PathBuf>> {
    while runs.len() > max_open {
        let mut next = Vec::new();
        for group in runs.chunks(max_open) {
            if group.len() == 1 {
                next.push(group[0].clone());
                continue;
            }
            let merged = merge_to_run(temp, group, header)?;
            for old in group {
                let _ = std::fs::remove_file(old);
                let _ = std::fs::remove_file(old.with_extension("idx"));
            }
            next.push(merged);
        }
        runs = next;
    }
    Ok(runs)
}

fn write_records<F>(options: &SortOptions, header: &sam::Header, mut next: F) -> Result<()>
where
    F: FnMut(&mut Record, &mut Vec<u8>) -> Result<bool>,
{
    let mut record = Record::default();
    let mut frame = Vec::new();
    if options.sam_output {
        let mut out = open_writer(options.output.as_deref())?;
        {
            let mut writer = SamWriter::new(&mut out);
            writer.write_header(header)?;
            while next(&mut record, &mut frame)? {
                writer.write_record(header, &record)?;
                if frame.is_empty() {
                    record = Record::default();
                }
            }
            writer.finish()?;
        }
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    } else if options.threads > 1 {
        let out = open_writer_send(options.output.as_deref())?;
        let workers = NonZero::new(options.threads).expect("threads > 1");
        let mt = noodles::bgzf::io::multithreaded_writer::Builder::default()
            .set_worker_count(workers)
            .build_from_writer(out);
        let mut writer = noodles::bam::io::Writer::from(mt);
        writer
            .write_header(header)
            .map_err(|error| Error::io(format!("cannot write BAM header: {error}")))?;
        while next(&mut record, &mut frame)? {
            if frame.is_empty() {
                writer
                    .write_alignment_record(header, &record)
                    .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))?;
            } else {
                writer
                    .get_mut()
                    .write_all(&frame)
                    .map_err(|error| Error::io(format!("cannot write BAM record: {error}")))?;
            }
            if frame.is_empty() {
                record = Record::default();
            }
        }
        let mut mt = writer.into_inner();
        let mut out = mt
            .finish()
            .map_err(|error| Error::io(format!("cannot finish BGZF stream: {error}")))?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    } else {
        let out = open_writer(options.output.as_deref())?;
        let mut writer = BamWriter::new(out);
        writer.write_header(header)?;
        while next(&mut record, &mut frame)? {
            if frame.is_empty() {
                writer.write_record(header, &record)?;
            } else {
                writer.write_encoded_record(&frame)?;
            }
            if frame.is_empty() {
                record = Record::default();
            }
        }
        let mut out = writer.finish()?;
        out.flush()
            .map_err(|error| Error::io(format!("cannot flush output: {error}")))?;
    }
    Ok(())
}

/// `sort`: coordinate-sort records into the documented order.
///
/// Fully described in `crate::sort_order`. The first chunk is sorted in memory;
/// when the configured budget is exceeded, sorted BAM runs plus a parallel
/// `u64` input-index sidecar are spilled to a temporary directory. Runs are
/// merged with a bounded number of open files. The sidecar preserves the
/// documented final input-index tie-break exactly.
pub fn sort(options: SortOptions) -> Result<u64> {
    let mut reader = AlignmentReader::open(&options.input)?;
    let header = reader.read_header()?;
    let final_header = header_with_coordinate_sort(header.clone());
    let budget = options.max_memory.unwrap_or(DEFAULT_SORT_MEMORY).max(1);
    let max_open = options.max_open_files.max(2);

    let mut chunk: Vec<(SortKey, Record)> = Vec::new();
    let mut chunk_bytes = 0u64;
    let mut runs: Vec<PathBuf> = Vec::new();
    let mut temp: Option<TempDir> = None;
    let mut index = 0usize;
    let mut record = Record::default();

    loop {
        let bytes = reader.read_record(&header, &mut record)?;
        if bytes == 0 {
            break;
        }
        let key = key_for(&record, index);
        chunk_bytes = chunk_bytes.saturating_add(record_memory(&record));
        chunk.push((key, std::mem::take(&mut record)));
        index += 1;
        if chunk_bytes >= budget {
            if temp.is_none() {
                temp = Some(TempDir::new(options.threads)?);
            }
            let workspace = temp.as_mut().expect("sort workspace was just created");
            runs.push(spill_run(workspace, &header, &mut chunk)?);
            chunk_bytes = 0;
        }
    }

    let total = index as u64;
    if runs.is_empty() {
        let mut order = sorted_indices(&chunk).into_iter();
        write_records(&options, &final_header, |record, _frame| {
            if let Some(position) = order.next() {
                *record = std::mem::take(&mut chunk[position].1);
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        return Ok(total);
    }

    if !chunk.is_empty() {
        let workspace = temp.as_mut().expect("runs exist, so the workspace exists");
        runs.push(spill_run(workspace, &header, &mut chunk)?);
    }
    // No further input chunks are needed; release their retained allocation
    // before allocating merge buffers.
    drop(chunk);
    let mut workspace = temp.expect("runs exist, so the workspace exists");
    let runs = reduce_runs(&mut workspace, runs, max_open, &header)?;
    merge_to_output(&options, &runs, &final_header)?;
    Ok(total)
}

fn merge_to_output(options: &SortOptions, paths: &[PathBuf], header: &sam::Header) -> Result<()> {
    let mut merge = open_runs(paths)?;
    write_records(options, header, |record, frame| {
        if let Some(run) = merge.tree.winner() {
            let item = merge.heads[run].as_mut().expect("winning merge head");
            // Keep read-ahead before output: a malformed replacement must
            // still prevent the popped record from being written.
            std::mem::swap(record, &mut item.record);
            std::mem::swap(frame, &mut item.frame);
            merge.refill(run)?;
            Ok(true)
        } else {
            Ok(false)
        }
    })
}

/// Set `@HD SO:coordinate` on a header.
///
/// `samtools sort` rewrites `@HD` to `SO:coordinate`; leaving a stale
/// `SO:unsorted` after sorting would be a false statement about the file. The
/// sort order lives in the header map's *other fields* (`SO`), not in a typed
/// accessor, so it is set through `other_fields_mut`.
fn header_with_coordinate_sort(header: sam::Header) -> sam::Header {
    use noodles::sam::header::record::value::map::header::{sort_order, tag, Version};
    use noodles::sam::header::record::value::map::Header;
    use noodles::sam::header::record::value::Map;

    // Start from the existing @HD when there is one, so VN and any custom fields
    // survive; otherwise default to VN:1.6 like samtools does.
    let mut hd = header.header().cloned().unwrap_or_else(|| {
        Map::<Header>::builder()
            .set_version(Version::new(1, 6))
            .build()
            .expect("VN:1.6 is always valid")
    });
    // `SO` is a standard tag with a public constant, so no raw key is needed.
    hd.other_fields_mut().insert(
        tag::SORT_ORDER,
        String::from_utf8_lossy(sort_order::COORDINATE)
            .into_owned()
            .into(),
    );

    let mut builder = sam::Header::builder().set_header(hd);
    for (name, reference) in header.reference_sequences() {
        builder = builder.add_reference_sequence(name.clone(), reference.clone());
    }
    for (name, read_group) in header.read_groups() {
        builder = builder.add_read_group(name.clone(), read_group.clone());
    }
    for (id, program) in header.programs().as_ref() {
        builder = builder.add_program(id.clone(), program.clone());
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with_extra_index() -> (TempDir, PathBuf, sam::Header) {
        let mut temp = TempDir::new(1).unwrap();
        let header = sam::Header::default();
        let record = Record::builder().set_name(b"kept".to_vec()).build();
        let mut chunk = vec![(key_for(&record, 0), record)];
        let path = spill_run(&mut temp, &header, &mut chunk).unwrap();
        File::options()
            .append(true)
            .open(path.with_extension("idx"))
            .unwrap()
            .write_all(&1u64.to_le_bytes())
            .unwrap();
        (temp, path, header)
    }

    #[test]
    fn captured_frames_are_bounded_and_large_records_use_the_encoder() {
        for payload_size in [
            0usize,
            1,
            31,
            MAX_CACHED_SORT_FRAME - 4,
            MAX_CACHED_SORT_FRAME - 3,
            1024 * 1024,
        ] {
            let mut bytes = (payload_size as u32).to_le_bytes().to_vec();
            bytes.resize(payload_size + 4, 0x41);
            let mut reader = CaptureReader {
                inner: std::io::Cursor::new(bytes.clone()),
                frame: Vec::new(),
                capture: true,
            };
            // A single read can include both prefix and payload. Check the
            // announced size before extending the capture with a large body.
            let mut decoded = vec![0; bytes.len()];
            reader.read_exact(&mut decoded).unwrap();
            assert_eq!(decoded, bytes);
            if bytes.len() <= MAX_CACHED_SORT_FRAME {
                assert_eq!(reader.frame, bytes);
            } else {
                assert!(reader.frame.is_empty());
                assert!(!reader.capture);
            }
            assert!(reader.frame.capacity() <= 2 * MAX_CACHED_SORT_FRAME);
        }
    }

    #[test]
    fn captured_frames_still_validate_every_auxiliary_field() {
        for invalid in [
            b"XXi\x01\x00\x00\x00XXi\x02\x00\x00\x00".as_slice(),
            b"XXZunterminated",
            b"XX?",
        ] {
            let (_temp, path, header) = run_with_extra_index();
            let mut merge = open_runs(std::slice::from_ref(&path)).unwrap();
            let mut frame = merge.heads[0].take().unwrap().frame;
            frame.extend_from_slice(invalid);
            let size = u32::try_from(frame.len() - 4).unwrap();
            frame[..4].copy_from_slice(&size.to_le_bytes());
            // Deliberately corrupt an internal run while retaining valid BGZF
            // framing/CRC, so skipping the full decoder would miss this error.
            let mut writer = BamWriter::new(File::create(&path).unwrap());
            writer.write_header(&header).unwrap();
            writer.write_encoded_record(&frame).unwrap();
            writer.finish().unwrap();
            std::fs::write(path.with_extension("idx"), 0u64.to_le_bytes()).unwrap();
            let mut ordinary = AlignmentReader::open(&path).unwrap();
            let ordinary_header = ordinary.read_header().unwrap();
            let expected = ordinary
                .read_record(&ordinary_header, &mut Record::default())
                .unwrap_err();
            let actual = open_runs(&[path])
                .err()
                .expect("malformed auxiliary data must fail");
            assert_eq!(actual.message(), expected.message());
            assert_eq!(actual.exit_code(), expected.exit_code());
        }
    }

    #[test]
    fn final_merge_retains_read_ahead_before_writing_popped_record() {
        let (_temp, path, header) = run_with_extra_index();
        let output = path.with_extension("sam");
        let options = SortOptions {
            input: path.clone(),
            output: Some(output.clone()),
            sam_output: true,
            max_memory: None,
            max_open_files: 2,
            threads: 1,
        };
        let error = merge_to_output(&options, &[path], &header).unwrap_err();
        assert!(error.message().contains("ended before its index"));
        assert!(!std::fs::read(output)
            .unwrap()
            .windows(4)
            .any(|w| w == b"kept"));
    }

    #[test]
    fn intermediate_merge_writes_index_before_refilling_record() {
        for workers in [0, 1, 4] {
            let (mut temp, path, header) = run_with_extra_index();
            temp.workers = workers;
            let error = merge_to_run(&mut temp, &[path], &header).unwrap_err();
            assert!(error.message().contains("ended before its index"));
            let written = std::fs::read(temp.path.join("run-1.idx")).unwrap();
            assert_eq!(written, 0u64.to_le_bytes());
        }
    }

    #[test]
    fn reused_run_reader_preserves_partial_index_and_index_eof_policy() {
        for count in 0..8 {
            let (_temp, path, _) = run_with_extra_index();
            std::fs::write(path.with_extension("idx"), vec![0; count]).unwrap();
            let mut run = open_run(&path).unwrap();
            let mut record = Record::default();
            let mut key = key_for(&record, 0);
            let result = next_from_run(&mut run, &mut record, &mut key, &mut Vec::new());
            if count == 0 {
                assert!(!result.unwrap());
            } else {
                assert!(result
                    .unwrap_err()
                    .message()
                    .starts_with("truncated sort index"));
            }
        }
    }

    #[test]
    fn buffered_sort_index_propagates_deferred_write_failure() {
        struct FullDisk;
        impl Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut writer = BufWriter::with_capacity(64, FullDisk);
        write_index(&mut writer, 7).unwrap();
        assert_eq!(
            flush_index(&mut writer).unwrap_err().to_string(),
            "cannot write sort index: disk full"
        );
    }

    #[test]
    fn the_bgzf_eof_marker_is_the_28_byte_htslib_pattern() {
        assert_eq!(BGZF_EOF_MARKER.len(), 28);
        // gzip magic, FEXTRA, XFL = 0 (not 2), and the BC subfield.
        assert_eq!(&BGZF_EOF_MARKER[..4], &[0x1f, 0x8b, 0x08, 0x04]);
        assert_eq!(BGZF_EOF_MARKER[8], 0x00, "XFL must be 0");
        assert_eq!(&BGZF_EOF_MARKER[12..14], b"BC");
    }

    #[test]
    fn flagstat_renders_all_sixteen_lines_in_samtools_order() {
        let stat = FlagStat {
            total: 3,
            primary: 3,
            primary_mapped: 2,
            mapped: 2,
            ..FlagStat::default()
        };
        let text = render_flagstat(&stat);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 16, "samtools flagstat prints 16 lines");
        assert_eq!(
            lines[0],
            "3 + 0 in total (QC-passed reads + QC-failed reads)"
        );
        assert_eq!(lines[1], "3 + 0 primary");
        assert_eq!(lines[6], "2 + 0 mapped (66.67% : N/A)");
        assert_eq!(lines[7], "2 + 0 primary mapped (66.67% : N/A)");
        assert_eq!(
            lines[15],
            "0 + 0 with mate mapped to a different chr (mapQ>=5)"
        );
    }

    #[test]
    fn flagstat_prints_na_when_the_denominator_is_zero() {
        let stat = FlagStat::default();
        let text = render_flagstat(&stat);
        assert!(text.contains("mapped (N/A : N/A)"), "{text}");
        assert!(text.contains("properly paired (N/A : N/A)"), "{text}");
    }

    #[test]
    fn flagstat_percentages_use_two_decimals() {
        let stat = FlagStat {
            total: 3,
            mapped: 2,
            ..FlagStat::default()
        };
        let text = render_flagstat(&stat);
        assert!(text.contains("66.67%"), "{text}");

        let stat = FlagStat {
            total: 3,
            mapped: 3,
            ..FlagStat::default()
        };
        let text = render_flagstat(&stat);
        assert!(text.contains("100.00%"), "{text}");
    }

    #[test]
    fn idxstats_renders_a_trailing_star_line() {
        let stats = IdxStats::from_references(vec![("chr1", 1000), ("chr2", 500)].into_iter());
        let text = render_idxstats(&stats);
        assert_eq!(text, "chr1\t1000\t0\t0\nchr2\t500\t0\t0\n*\t0\t0\t0\n");
    }

    #[test]
    fn idxstats_numbers_appear_in_mapped_then_unmapped_order() {
        let mut stats = IdxStats::from_references(vec![("chr1", 1000)].into_iter());
        stats.add(Some(0), false);
        stats.add(Some(0), false);
        stats.add(Some(0), true);
        let text = render_idxstats(&stats);
        assert_eq!(text.lines().next().unwrap(), "chr1\t1000\t2\t1");
    }

    #[test]
    fn quickcheck_exit_code_is_16_on_failure_and_0_on_success() {
        let ok = QuickCheck {
            passed: vec![PathBuf::from("a.bam")],
            failed: Vec::new(),
        };
        assert_eq!(ok.exit_code(), 0);
        assert!(ok.ok());

        let bad = QuickCheck {
            passed: Vec::new(),
            failed: vec![(PathBuf::from("b.bam"), "is empty".to_string())],
        };
        assert_eq!(bad.exit_code(), 16);
        assert!(!bad.ok());
    }
}
