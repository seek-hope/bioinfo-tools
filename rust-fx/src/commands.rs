//! The SeqKit/fq subcommands.
//!
//! Each command is a thin planner over the shared reader/writer in
//! [`crate::io`]. The behaviour they implement is SeqKit 2.14.0's documented
//! behaviour (and fq 0.12.0's for `lint`). Direct per-command differential
//! evidence and remaining gaps are recorded in `compat/matrix.tsv`.

use crate::io::{CommandError, InputFormat, LintReader, Output, OwnedRecord, RecordReader};
use crate::lint::{Level, LintRecord, Linter};
use crate::sample::{Sampler, Strategy};
use crate::split::{self, SplitBy};
use crate::stats::{GcSkewCounts, Merge, QualStats, SeqStats, DEFAULT_GAP_LETTERS};
use crate::table::{self, Layout as TableLayout, StatsRow};
use crate::transform::{self, Transforms};
use bioinfo_fastx::writer::Layout;
use regex::RegexBuilder;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

// --------------------------------------------------------------------------- //
// stats
// --------------------------------------------------------------------------- //

/// Everything `stats` accumulates for one input.
#[derive(Default)]
struct InputStats {
    records: u64,
    lengths: BTreeMap<u64, u64>,
    sum_len: u64,
    min_len: Option<u64>,
    max_len: u64,
    sum_gap: u64,
    sum_n: u64,
    gc: u64,
    at: u64,
    qual: QualStats,
    format: Option<InputFormat>,
}

impl InputStats {
    fn absorb(&mut self, record: &OwnedRecord, gap_letters: &[u8], quality_base: u8) {
        let sequence = SeqStats::from_sequence(&record.sequence, gap_letters);
        self.records += 1;
        *self.lengths.entry(sequence.length).or_insert(0) += 1;
        self.sum_len += sequence.length;
        self.min_len = Some(match self.min_len {
            Some(current) => current.min(sequence.length),
            None => sequence.length,
        });
        self.max_len = self.max_len.max(sequence.length);
        self.sum_gap += sequence.gap_count;
        self.sum_n += sequence.n_count;
        self.gc += sequence.gc;
        self.at += sequence.at;
        if record.is_fastq() {
            self.qual
                .merge(&QualStats::from_qualities(&record.qualities, quality_base));
        }
    }

    fn row(&self, label: String, all: bool) -> StatsRow {
        let num_seqs = self.records;
        let avg_len = if num_seqs == 0 {
            0.0
        } else {
            self.sum_len as f64 / num_seqs as f64
        };
        let (n50, n50_num) = table::n50_from_histogram(&self.lengths);
        let (q1, q2, q3) = table::quartiles_from_histogram(&self.lengths);
        StatsRow {
            file: label,
            format: self.format.unwrap_or(InputFormat::Fastq).label(),
            seq_type: crate::transform::guess_seq_type(&[]),
            num_seqs,
            sum_len: self.sum_len,
            min_len: self.min_len.unwrap_or(0),
            avg_len,
            max_len: self.max_len,
            q1: all.then_some(q1),
            q2: all.then_some(q2),
            q3: all.then_some(q3),
            sum_gap: all.then_some(self.sum_gap),
            n50: all.then_some(n50),
            n50_num: all.then_some(n50_num),
            q20: all.then(|| self.qual.q20_percent().unwrap_or(0.0)),
            q30: all.then(|| self.qual.q30_percent().unwrap_or(0.0)),
            avg_qual: all.then(|| self.qual.average_quality().unwrap_or(0.0)),
            // `stats` GC(%) is (G+C)/length, so N and gaps dilute it. This is a
            // different contract from `fx2tab -g`, which is (G+C)/(G+C+A+T).
            // Verified: GCGCAAAANN -> stats 40.00, fx2tab -g 50.00.
            gc: all.then(|| {
                if self.sum_len == 0 {
                    0.0
                } else {
                    self.gc as f64 * 100.0 / self.sum_len as f64
                }
            }),
            sum_n: all.then_some(self.sum_n),
        }
    }
}

/// The parsed `stats` options.
pub struct StatsOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-T`: tab-separated output with no digit grouping.
    pub tabular: bool,
    /// `-a`: add quartiles, N50, gap/N counts and quality columns.
    pub all: bool,
    /// `-b`: print only the file's base name.
    pub basename: bool,
    /// `-G`: characters counted as gaps.
    pub gap_letters: Vec<u8>,
    /// `-i`: label used in place of `-` for standard input.
    pub stdin_label: String,
    /// `-e`: warn and continue on an unreadable input.
    pub skip_err: bool,
    /// ASCII offset for `-E/--fq-encoding`.
    pub quality_base: u8,
    /// `-o`: output file (`-` or absent means stdout).
    pub out_file: Option<String>,
}

/// Run `stats`.
pub fn stats(options: StatsOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut rows = Vec::new();
    for input in &inputs {
        match collect_stats(input, &options) {
            Ok((row, _seq_type)) => rows.push(row),
            Err(error) => {
                if options.skip_err {
                    eprintln!("[WARN] {input}: {}", error.message);
                    continue;
                }
                return Err(error);
            }
        }
    }
    let rendered = table::render(
        &rows,
        options.all,
        if options.tabular {
            TableLayout::Tabular
        } else {
            TableLayout::Human
        },
    );
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    out.write_all(rendered.as_bytes())?;
    out.finish()
}

/// Accumulate one input and build its row.
fn collect_stats(
    input: &str,
    options: &StatsOptions,
) -> Result<(StatsRow, &'static str), CommandError> {
    let mut reader = RecordReader::open(input)?;
    let format = reader.format();
    let mut stats = InputStats {
        format: Some(format),
        ..InputStats::default()
    };
    // The first record's sequence decides the reported type, matching
    // "guessed from the leading subsequences of the first sequences".
    let mut seq_type = "Unlimit";
    let mut first = true;
    let mut record = OwnedRecord::default();
    while reader.read_record(&mut record)? {
        if first {
            seq_type = transform::guess_seq_type(&record.sequence);
            first = false;
        }
        stats.absorb(&record, &options.gap_letters, options.quality_base);
    }
    let label = if input == "-" {
        options.stdin_label.clone()
    } else if options.basename {
        Path::new(input)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| input.to_string())
    } else {
        input.to_string()
    };
    let mut row = stats.row(label, options.all);
    row.seq_type = seq_type;
    Ok((row, seq_type))
}

// --------------------------------------------------------------------------- //
// seq
// --------------------------------------------------------------------------- //

/// The parsed `seq` options.
pub struct SeqOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-n`: print names only.
    pub name: bool,
    /// `-i`: print the ID rather than the full header.
    pub only_id: bool,
    /// `-s`: print sequences only.
    pub seq_only: bool,
    /// `-q`: print qualities only.
    pub qual_only: bool,
    /// Which sequence transforms to apply.
    pub transforms: Transforms,
    /// `-G`: characters removed by `-g`.
    pub gap_letters: Vec<u8>,
    /// `-m`: minimum length, inclusive.
    pub min_len: Option<u64>,
    /// `-M`: maximum length, inclusive.
    pub max_len: Option<u64>,
    /// `-Q`: minimum average quality, inclusive.
    pub min_qual: Option<f64>,
    /// `-R`: maximum average quality, exclusive.
    pub max_qual: Option<f64>,
    /// `-b`: ASCII offset for quality decoding.
    pub quality_base: u8,
    /// `-w`: FASTA line width (0 disables wrapping).
    pub line_width: usize,
    /// `-o`: output file (`-` or absent means stdout).
    pub out_file: Option<String>,
}

/// Run `seq`.
pub fn seq(options: SeqOptions) -> Result<(), CommandError> {
    // Contradictory output selections are a usage error rather than a silent
    // precedence rule, which is what boundary rule 6 requires.
    let selected = [options.name, options.seq_only, options.qual_only]
        .iter()
        .filter(|flag| **flag)
        .count();
    if selected > 1 {
        return Err(CommandError::usage(
            "only one of -n/--name, -s/--seq and -q/--qual may be given",
        ));
    }
    if options.transforms.upper && options.transforms.lower {
        return Err(CommandError::usage(
            "-u/--upper-case and -l/--lower-case are mutually exclusive",
        ));
    }
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let input_format = reader.format();
        while let Some(record) = reader.next_record() {
            let record = record?;
            // Length filters use the raw length, gaps included, as documented.
            if let Some(min) = options.min_len {
                if (record.sequence.len() as u64) < min {
                    continue;
                }
            }
            if let Some(max) = options.max_len {
                if (record.sequence.len() as u64) > max {
                    continue;
                }
            }
            if options.min_qual.is_some() || options.max_qual.is_some() {
                let average = QualStats::from_qualities(&record.qualities, options.quality_base)
                    .average_quality()
                    .unwrap_or(0.0);
                if let Some(min) = options.min_qual {
                    if average < min {
                        continue;
                    }
                }
                if let Some(max) = options.max_qual {
                    if average >= max {
                        continue;
                    }
                }
            }
            write_seq_record(&mut out, &record, input_format, &options)?;
        }
    }
    out.finish()
}

fn write_seq_record(
    out: &mut Output,
    record: &OwnedRecord,
    input_format: InputFormat,
    options: &SeqOptions,
) -> Result<(), CommandError> {
    // The transformed sequence, if any transform was requested.
    let transformed = if options.transforms.is_identity() {
        None
    } else {
        Some(
            options
                .transforms
                .apply(&record.sequence, &options.gap_letters),
        )
    };
    let sequence = transformed.as_deref().unwrap_or(&record.sequence);

    if options.name {
        if options.only_id {
            out.write_all(record.id())?;
        } else {
            out.write_all(&record.name)?;
        }
        out.write_all(b"\n")?;
        return Ok(());
    }
    if options.seq_only {
        out.write_all(sequence)?;
        out.write_all(b"\n")?;
        return Ok(());
    }
    if options.qual_only {
        out.write_all(&record.qualities)?;
        out.write_all(b"\n")?;
        return Ok(());
    }

    // Full record output. FASTA input stays FASTA; FASTQ input stays FASTQ, with
    // the *same* quality string when the sequence length is unchanged.
    let layout = match input_format {
        InputFormat::Fastq => Layout::Fastq,
        InputFormat::Fasta => Layout::Fasta,
    };
    if transformed.is_some() && sequence.len() != record.sequence.len() && layout == Layout::Fastq {
        return Err(CommandError::input(
            "a transform changed the sequence length of a FASTQ record, so its qualities no \
             longer match; use -s/--seq for length-changing transforms",
        ));
    }
    let emitted = OwnedRecord {
        name: record.name.clone(),
        raw_header: record.raw_header.clone(),
        sequence: sequence.to_vec(),
        plus: record.plus.clone(),
        qualities: record.qualities.clone(),
        has_quality_line: record.has_quality_line,
        index: record.index,
    };
    emitted.write_to(out, layout, options.line_width)?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// head
// --------------------------------------------------------------------------- //

/// The parsed `head` options.
pub struct HeadOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-n`: number of records to print (ignored when `length` is set).
    pub number: u64,
    /// `-l`: stop after the total sequence length reaches this value.
    pub length: Option<u64>,
    /// `-X`: additional input paths, one per line.
    pub infile_list: Option<String>,
    /// Skip the input existence check for list-driven invocations.
    pub skip_file_check: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `head`.
pub fn head(options: HeadOptions) -> Result<(), CommandError> {
    let mut inputs = options.inputs.clone();
    if let Some(path) = &options.infile_list {
        let text = std::fs::read_to_string(path)
            .map_err(|error| CommandError::io(format!("{path}: {error}")))?;
        inputs.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string),
        );
    }
    if !options.skip_file_check {
        for input in &inputs {
            if input != "-" {
                std::fs::File::open(input)
                    .map_err(|error| CommandError::io(format!("{input}: {error}")))?;
            }
        }
    }
    let inputs = if inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        inputs
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    let mut written = 0u64;
    let mut total_length = 0u64;
    // SeqKit treats -l 0 as "no length limit" (and still overrides -n).
    let length_limit = options.length.filter(|limit| *limit > 0);
    let all_by_length = options.length == Some(0);
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            if let Some(limit) = length_limit {
                if total_length >= limit {
                    break;
                }
                total_length += record.sequence.len() as u64;
                record.write_to(&mut out, layout, options.line_width)?;
                if total_length >= limit {
                    break;
                }
            } else if all_by_length {
                record.write_to(&mut out, layout, options.line_width)?;
            } else {
                if written >= options.number {
                    break;
                }
                record.write_to(&mut out, layout, options.line_width)?;
                written += 1;
            }
        }
        if length_limit
            .map(|limit| total_length >= limit)
            .unwrap_or(false)
            || (!all_by_length && options.length.is_none() && written >= options.number)
        {
            break;
        }
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// subseq
// --------------------------------------------------------------------------- //

/// The parsed `subseq` options.
pub struct SubseqOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-r`: 1-based region, negative indices allowed.
    pub region: Option<String>,
    /// `--bed`: 0-based half-open intervals.
    pub bed: Option<String>,
    /// `--gtf`: 1-based GTF features.
    pub gtf: Option<String>,
    /// `--feature`: optional GTF feature filter.
    pub features: Vec<String>,
    /// `--gtf-tag`: GTF attribute used as the output comment.
    pub gtf_tag: String,
    /// `--chr`: optional BED/GTF chromosome filter.
    pub chr: Vec<String>,
    /// `-u`: upstream flank length.
    pub up_stream: usize,
    /// `-d`: downstream flank length.
    pub down_stream: usize,
    /// `-f`: only return flanking sequence.
    pub only_flank: bool,
    /// `-R`: append the region coordinates to the record name.
    pub region_coord: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

pub(crate) struct Region {
    start: i64,
    end: i64,
}

impl Region {
    pub(crate) fn parse(text: &str) -> Result<Region, CommandError> {
        let (start, end) = text.split_once(':').ok_or_else(|| {
            CommandError::usage("--region must be START:END, for example 1:12 or -12:-1")
        })?;
        let start = start
            .parse::<i64>()
            .map_err(|_| CommandError::usage(format!("invalid region start '{start}'")))?;
        let end = end
            .parse::<i64>()
            .map_err(|_| CommandError::usage(format!("invalid region end '{end}'")))?;
        if start == 0 || end == 0 {
            return Err(CommandError::usage(
                "region indices are 1-based and must not be 0",
            ));
        }
        Ok(Region { start, end })
    }

    pub(crate) fn bounds(&self, length: usize) -> (usize, usize) {
        let resolve = |index: i64| -> i64 {
            if index > 0 {
                index - 1
            } else {
                length as i64 + index
            }
        };
        let start = resolve(self.start).clamp(0, length as i64);
        let end = resolve(self.end).clamp(0, length as i64);
        if end < start {
            (start as usize, start as usize)
        } else {
            (start as usize, (end + 1).min(length as i64) as usize)
        }
    }
}

/// Run `subseq -r`.
pub fn subseq(options: SubseqOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    if let Some(gtf) = &options.gtf {
        return subseq_gtf(&options, gtf, &inputs);
    }
    if let Some(bed) = &options.bed {
        return subseq_bed(&options, bed, &inputs);
    }
    let region_text = options
        .region
        .as_deref()
        .ok_or_else(|| CommandError::usage("subseq needs -r/--region or --bed"))?;
    let region = Region::parse(region_text)?;
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let mut record = record?;
            let (start, end) = region.bounds(record.sequence.len());
            record.sequence = record.sequence[start..end].to_vec();
            if record.has_quality_line {
                record.qualities = record.qualities[start..end].to_vec();
            }
            if options.region_coord {
                let suffix = format!(":{}-{} ", region.start, region.end);
                record.name.extend_from_slice(suffix.as_bytes());
                if !record.raw_header.is_empty() {
                    record.raw_header.extend_from_slice(suffix.as_bytes());
                }
            }
            record.write_to(&mut out, layout, options.line_width)?;
        }
    }
    out.finish()
}

struct BedInterval {
    chrom: String,
    start: usize,
    end: usize,
    strand: String,
}

fn read_bed(path: &str) -> Result<Vec<BedInterval>, CommandError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CommandError::io(format!("{path}: {error}")))?;
    let mut intervals = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("track ") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            return Err(CommandError::input(format!(
                "{path}:{}: BED needs at least 3 columns",
                line_no + 1
            )));
        }
        let start = fields[1].parse::<usize>().map_err(|_| {
            CommandError::input(format!("{path}:{}: invalid BED start", line_no + 1))
        })?;
        let end = fields[2]
            .parse::<usize>()
            .map_err(|_| CommandError::input(format!("{path}:{}: invalid BED end", line_no + 1)))?;
        if start >= end {
            return Err(CommandError::input(format!(
                "{path}:{}: BED start must be less than end",
                line_no + 1
            )));
        }
        intervals.push(BedInterval {
            chrom: fields[0].to_string(),
            start,
            end,
            strand: fields.get(5).copied().unwrap_or(".").to_string(),
        });
    }
    Ok(intervals)
}

fn subseq_bed(options: &SubseqOptions, bed: &str, inputs: &[String]) -> Result<(), CommandError> {
    let intervals = read_bed(bed)?;
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        let mut records = Vec::new();
        while let Some(record) = reader.next_record() {
            records.push(record?);
        }
        for interval in &intervals {
            if !options.chr.is_empty()
                && !options
                    .chr
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&interval.chrom))
            {
                continue;
            }
            let Some(source) = records
                .iter()
                .find(|record| record.id() == interval.chrom.as_bytes())
            else {
                continue;
            };
            let length = source.sequence.len();
            if interval.start >= length {
                continue;
            }
            let end = interval.end.min(length);
            let mut record = source.clone();
            let mut start = interval.start;
            let mut stop = end;
            let mut suffix = String::new();
            if options.only_flank {
                if options.up_stream > 0 && options.down_stream > 0 {
                    return Err(CommandError::usage(
                        "-f/--only-flank allows only one of -u/--up-stream and -d/--down-stream",
                    ));
                }
                if options.up_stream > 0 {
                    start = start.saturating_sub(options.up_stream);
                    stop = interval.start.min(length);
                    suffix.push_str(&format!("_usf:{}", options.up_stream));
                } else if options.down_stream > 0 {
                    start = end.min(length);
                    stop = (end + options.down_stream).min(length);
                    suffix.push_str(&format!("_dsf:{}", options.down_stream));
                } else {
                    continue;
                }
            } else {
                if options.up_stream > 0 {
                    start = start.saturating_sub(options.up_stream);
                    suffix.push_str(&format!("_us:{}", options.up_stream));
                }
                if options.down_stream > 0 {
                    stop = (stop + options.down_stream).min(length);
                    suffix.push_str(&format!("_ds:{}", options.down_stream));
                }
            }
            record.sequence = record.sequence[start..stop].to_vec();
            if record.has_quality_line {
                record.qualities = record.qualities[start..stop].to_vec();
            }
            let header = format!(
                "{}_{}-{}:{}{}",
                interval.chrom,
                interval.start + 1,
                interval.end,
                interval.strand,
                suffix
            );
            record.name = format!("{header} ").into_bytes();
            let marker = record.raw_header.first().copied().unwrap_or(b'>');
            record.raw_header = format!("{}{} ", char::from(marker), header).into_bytes();
            record.write_to(&mut out, layout, options.line_width)?;
        }
    }
    out.finish()
}

struct GtfInterval {
    chrom: String,
    start: usize,
    end: usize,
    strand: String,
    tag: String,
}

fn read_gtf(
    path: &str,
    wanted_features: &[String],
    tag_name: &str,
) -> Result<Vec<GtfInterval>, CommandError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CommandError::io(format!("{path}: {error}")))?;
    let mut intervals = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 9 {
            return Err(CommandError::input(format!(
                "{path}:{}: GTF needs 9 tab-separated columns",
                line_no + 1
            )));
        }
        if !wanted_features.is_empty()
            && !wanted_features
                .iter()
                .any(|feature| feature.eq_ignore_ascii_case(fields[2]))
        {
            continue;
        }
        let start = fields[3].parse::<usize>().map_err(|_| {
            CommandError::input(format!("{path}:{}: invalid GTF start", line_no + 1))
        })?;
        let end = fields[4]
            .parse::<usize>()
            .map_err(|_| CommandError::input(format!("{path}:{}: invalid GTF end", line_no + 1)))?;
        if start == 0 || end < start {
            return Err(CommandError::input(format!(
                "{path}:{}: invalid 1-based GTF interval",
                line_no + 1
            )));
        }
        let tag = gtf_attribute(fields[8], tag_name).unwrap_or_default();
        intervals.push(GtfInterval {
            chrom: fields[0].to_string(),
            start: start - 1,
            end,
            strand: fields[6].to_string(),
            tag,
        });
    }
    Ok(intervals)
}

fn gtf_attribute(attributes: &str, name: &str) -> Option<String> {
    for part in attributes.split(';') {
        let part = part.trim();
        let (key, value) = part.split_once(' ')?;
        if key == name {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn subseq_gtf(options: &SubseqOptions, gtf: &str, inputs: &[String]) -> Result<(), CommandError> {
    let intervals = read_gtf(gtf, &options.features, &options.gtf_tag)?;
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        let mut records = Vec::new();
        while let Some(record) = reader.next_record() {
            records.push(record?);
        }
        for interval in &intervals {
            if !options.chr.is_empty()
                && !options
                    .chr
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&interval.chrom))
            {
                continue;
            }
            let Some(source) = records
                .iter()
                .find(|record| record.id() == interval.chrom.as_bytes())
            else {
                continue;
            };
            if interval.start >= source.sequence.len() {
                continue;
            }
            let end = interval.end.min(source.sequence.len());
            let length = source.sequence.len();
            let negative = interval.strand == "-";
            let mut start = interval.start;
            let mut stop = end;
            let mut suffix = String::new();
            if options.only_flank {
                if options.up_stream > 0 && options.down_stream > 0 {
                    return Err(CommandError::usage(
                        "-f/--only-flank allows only one of -u/--up-stream and -d/--down-stream",
                    ));
                }
                if options.up_stream > 0 {
                    if negative {
                        start = end.min(length);
                        stop = (end + options.up_stream).min(length);
                    } else {
                        start = start.saturating_sub(options.up_stream);
                        stop = interval.start.min(length);
                    }
                    suffix.push_str(&format!("_usf:{}", options.up_stream));
                } else if options.down_stream > 0 {
                    if negative {
                        start = start.saturating_sub(options.down_stream);
                        stop = interval.start.min(length);
                    } else {
                        start = end.min(length);
                        stop = (end + options.down_stream).min(length);
                    }
                    suffix.push_str(&format!("_dsf:{}", options.down_stream));
                } else {
                    continue;
                }
            } else {
                let (left, right) = if negative {
                    (options.down_stream, options.up_stream)
                } else {
                    (options.up_stream, options.down_stream)
                };
                if left > 0 {
                    start = start.saturating_sub(left);
                }
                if right > 0 {
                    stop = (stop + right).min(length);
                }
                if options.up_stream > 0 {
                    suffix.push_str(&format!("_us:{}", options.up_stream));
                }
                if options.down_stream > 0 {
                    suffix.push_str(&format!("_ds:{}", options.down_stream));
                }
            }
            let mut record = source.clone();
            record.sequence = record.sequence[start..stop].to_vec();
            if record.has_quality_line {
                record.qualities = record.qualities[start..stop].to_vec();
            }
            if interval.strand == "-" {
                record.sequence = transform::reverse_complement(&record.sequence);
                if record.has_quality_line {
                    record.qualities.reverse();
                }
            }
            let base = format!(
                "{}_{}-{}:{}{}",
                interval.chrom,
                interval.start + 1,
                interval.end,
                interval.strand,
                suffix
            );
            let comment = if interval.tag.is_empty() {
                if suffix.is_empty() {
                    base
                } else {
                    format!("{base} ")
                }
            } else {
                format!("{base} {}", interval.tag)
            };
            record.name = comment.clone().into_bytes();
            let marker = record.raw_header.first().copied().unwrap_or(b'>');
            record.raw_header = format!("{}{}", char::from(marker), comment).into_bytes();
            record.write_to(&mut out, layout, options.line_width)?;
        }
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// range
// --------------------------------------------------------------------------- //

/// The parsed `range` options.
pub struct RangeOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-r`: 1-based record range, negative indices allowed.
    pub region: String,
    /// Whether `-w` was explicitly supplied (also wraps FASTQ in SeqKit).
    pub line_width_explicit: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `range -r` independently for each input file.
pub fn range(options: RangeOptions) -> Result<(), CommandError> {
    crate::range::run(options)
}

// --------------------------------------------------------------------------- //
// sliding
// --------------------------------------------------------------------------- //

/// The parsed `sliding` options.
pub struct SlidingOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-W`: window size.
    pub window: usize,
    /// `-s`: step size.
    pub step: usize,
    /// `-S`: suffix added to the ID.
    pub suffix: String,
    /// `-g`: include a final shorter window.
    pub greedy: bool,
    /// `-c/-C`: allow windows to cross the circular origin.
    pub circular: bool,
    /// `-X`: additional input paths, one per line.
    pub infile_list: Option<String>,
    /// Skip the input existence check.
    pub skip_file_check: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `sliding`.
pub fn sliding(options: SlidingOptions) -> Result<(), CommandError> {
    let mut inputs = options.inputs.clone();
    if let Some(path) = &options.infile_list {
        let text = std::fs::read_to_string(path)
            .map_err(|error| CommandError::io(format!("{path}: {error}")))?;
        inputs.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string),
        );
    }
    if !options.skip_file_check {
        for input in &inputs {
            if input != "-" {
                std::fs::File::open(input)
                    .map_err(|error| CommandError::io(format!("{input}: {error}")))?;
            }
        }
    }
    let inputs = if inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        inputs
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            let length = record.sequence.len();
            let extended_length = if options.circular {
                length + options.window.saturating_sub(1)
            } else {
                length
            };
            let mut start = 0usize;
            loop {
                let end = if options.circular || start + options.window <= length {
                    start + options.window
                } else if options.greedy && start < length {
                    length
                } else {
                    break;
                };
                if end > extended_length || start >= length {
                    break;
                }
                let mut window = record.clone();
                window.sequence = if options.circular {
                    let mut sequence = Vec::with_capacity(extended_length);
                    while sequence.len() < extended_length {
                        sequence.extend_from_slice(&record.sequence);
                    }
                    sequence[start..end].to_vec()
                } else {
                    record.sequence[start..end].to_vec()
                };
                if window.has_quality_line {
                    window.qualities = if options.circular {
                        let mut qualities = Vec::with_capacity(extended_length);
                        while qualities.len() < extended_length {
                            qualities.extend_from_slice(&record.qualities);
                        }
                        qualities[start..end].to_vec()
                    } else {
                        record.qualities[start..end].to_vec()
                    };
                }
                let stem = String::from_utf8_lossy(record.name.trim_ascii_end());
                let display_end = if options.circular {
                    (end - 1) % length + 1
                } else {
                    end
                };
                let name = format!("{}{}:{}-{}", stem, options.suffix, start + 1, display_end);
                window.name = name.clone().into_bytes();
                let marker = record.raw_header.first().copied().unwrap_or(b'>');
                window.raw_header = format!("{}{}", char::from(marker), name).into_bytes();
                window.write_to(&mut out, layout, options.line_width)?;
                if !options.circular && end == length && !options.greedy {
                    break;
                }
                start += options.step;
                if start >= length {
                    break;
                }
            }
        }
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// duplicate
// --------------------------------------------------------------------------- //

/// The parsed `duplicate` options.
pub struct DuplicateOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-n`: duplication number.
    pub times: usize,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `duplicate`.
pub fn duplicate(options: DuplicateOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            for _ in 0..options.times {
                record.write_to(&mut out, layout, options.line_width)?;
            }
        }
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// rmdup
// --------------------------------------------------------------------------- //

/// The parsed `rmdup` options.
pub struct RmdupOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-n`: compare full names.
    pub by_name: bool,
    /// `-s`: compare sequences.
    pub by_seq: bool,
    /// `-i`: ignore case for sequence comparison.
    pub ignore_case: bool,
    /// `-P`: only compare the positive strand.
    pub only_positive: bool,
    /// `-D`: write duplicate counts and ID lists here.
    pub dup_num_file: Option<String>,
    /// `-d`: write records after the first occurrence here.
    pub dup_seqs_file: Option<String>,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `rmdup`.
pub fn rmdup(options: RmdupOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let report_duplicates = options.dup_num_file.is_some() || options.dup_seqs_file.is_some();
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    let mut seen: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut groups: Vec<(Vec<Vec<u8>>, Vec<OwnedRecord>)> = Vec::new();
    let mut layout = Layout::Fasta;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            let mut key = if options.by_seq {
                let mut sequence = record.sequence.clone();
                if options.ignore_case {
                    sequence.make_ascii_uppercase();
                }
                if !options.only_positive {
                    let reverse = transform::reverse_complement(&sequence);
                    if reverse < sequence {
                        sequence = reverse;
                    }
                }
                sequence
            } else if options.by_name {
                record.name.clone()
            } else {
                record.id().to_vec()
            };
            if options.by_seq && options.ignore_case {
                key.make_ascii_uppercase();
            }
            if let Some(index) = seen.get(&key).copied() {
                if report_duplicates {
                    groups[index].0.push(record.name.clone());
                    if options.dup_seqs_file.is_some() {
                        groups[index].1.push(record);
                    }
                }
            } else {
                let index = groups.len();
                seen.insert(key, index);
                if report_duplicates {
                    groups.push((vec![record.name.clone()], Vec::new()));
                }
                record.write_to(&mut out, layout, options.line_width)?;
            }
        }
    }
    out.finish()?;
    if let Some(path) = &options.dup_num_file {
        let mut report = Output::create_with_level(Some(path), options.compress_level)?;
        for (names, _) in &groups {
            if names.len() > 1 {
                writeln!(
                    report,
                    "{}\t{}",
                    names.len(),
                    names
                        .iter()
                        .map(|name| String::from_utf8_lossy(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .map_err(|error| {
                    CommandError::io(format!("cannot write duplicate counts: {error}"))
                })?;
            }
        }
        report.finish()?;
    }
    if let Some(path) = &options.dup_seqs_file {
        let mut report = Output::create_with_level(Some(path), options.compress_level)?;
        for (_, records) in &groups {
            for record in records {
                record.write_to(&mut report, layout, options.line_width)?;
            }
        }
        report.finish()?;
    }
    Ok(())
}

// --------------------------------------------------------------------------- //
// faidx
// --------------------------------------------------------------------------- //

/// The parsed `faidx` options.
pub struct FaidxOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// FASTA file.
    pub fasta: String,
    /// Positional regions.
    pub regions: Vec<String>,
    /// `-f`: full header.
    pub full_head: bool,
    /// `-i`: case-insensitive IDs.
    pub ignore_case: bool,
    /// `-l`: region list file.
    pub region_file: Option<String>,
    /// `-r`: regex IDs.
    pub regexp: bool,
    /// `-U`: rebuild an existing FASTA index.
    pub update_faidx: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

fn parse_faidx_region(text: &str) -> Option<(&str, i64, i64)> {
    let (id, coords) = text.rsplit_once(':')?;
    let (start, end) = coords.split_once('-')?;
    let start = start.parse::<i64>().ok()?;
    let end = end.parse::<i64>().ok()?;
    if start == 0 || end == 0 || id.is_empty() {
        return None;
    }
    Some((id, start, end))
}

/// Run `faidx` region extraction.
pub fn faidx(options: FaidxOptions) -> Result<(), CommandError> {
    let input = Path::new(&options.fasta);
    let index_path = if options.full_head {
        PathBuf::from(format!("{}.seqkit.fai", input.display()))
    } else {
        PathBuf::from(format!("{}.fai", input.display()))
    };
    let entries =
        crate::sort::load_or_build_fai(input, &index_path, options.update_faidx, 1, b"- \t.")?;
    let mut regions = options.regions.clone();
    if let Some(path) = &options.region_file {
        let text = std::fs::read_to_string(path)
            .map_err(|error| CommandError::io(format!("{path}: {error}")))?;
        regions.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_string),
        );
    }
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for region in &regions {
        if options.regexp {
            let regex = RegexBuilder::new(region)
                .case_insensitive(options.ignore_case)
                .build()
                .map_err(|error| {
                    CommandError::usage(format!("invalid ID regexp: {error}")).with_code(255)
                })?;
            for entry in &entries {
                if !regex.is_match(&String::from_utf8_lossy(&entry.id)) {
                    continue;
                }
                let mut record = crate::sort::read_record_at(input, entry)?;
                let header = if options.full_head {
                    entry.name.clone()
                } else {
                    entry.id.clone()
                };
                record.name = header.clone();
                record.raw_header = format!(">{}", String::from_utf8_lossy(&header)).into_bytes();
                record.write_to(&mut out, Layout::Fasta, options.line_width)?;
            }
            continue;
        }
        let Some((id, raw_start, raw_end)) = parse_faidx_region(region) else {
            continue;
        };
        let Some(entry) = entries.iter().find(|entry| {
            if options.ignore_case {
                entry.id.eq_ignore_ascii_case(id.as_bytes())
            } else {
                entry.id == id.as_bytes()
            }
        }) else {
            continue;
        };
        let mut record = crate::sort::read_record_at(input, entry)?;
        let length = record.sequence.len();
        let resolve = |index: i64| -> i64 {
            if index > 0 {
                index - 1
            } else {
                length as i64 + index
            }
        };
        let start = resolve(raw_start).clamp(0, length as i64);
        let end = resolve(raw_end).clamp(0, length as i64);
        if end < start {
            continue;
        }
        let end = (end + 1).min(length as i64) as usize;
        record.sequence = record.sequence[start as usize..end].to_vec();
        if record.has_quality_line {
            record.qualities = record.qualities[start as usize..end].to_vec();
        }
        let coords = region
            .rsplit_once(':')
            .map(|(_, value)| value)
            .unwrap_or(region);
        let mut header = if options.full_head {
            entry.name.clone()
        } else {
            entry.id.clone()
        };
        header.extend_from_slice(format!(":{coords}").as_bytes());
        record.name = header.clone();
        record.raw_header = format!(">{}", String::from_utf8_lossy(&header)).into_bytes();
        record.write_to(&mut out, Layout::Fasta, options.line_width)?;
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// rename
// --------------------------------------------------------------------------- //

/// The parsed `rename` options.
pub struct RenameOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-n`: check duplicates by full name.
    pub by_name: bool,
    /// `-1`: rename the first record too.
    pub rename_first: bool,
    /// `-s`: separator.
    pub separator: String,
    /// `-N`: first duplicate counter.
    pub start_num: u64,
    /// `-m`: write one output file per input file.
    pub multiple_outfiles: bool,
    /// `-O`: directory for multiple output files.
    pub out_dir: Option<String>,
    /// `-f`: suppress the non-empty output-directory warning.
    pub force: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `rename`.
pub fn rename(options: RenameOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let multiple = options.multiple_outfiles;
    let out_dir = options
        .out_dir
        .clone()
        .unwrap_or_else(|| "renamed".to_string());
    if multiple {
        if inputs.iter().any(|input| input == "-") {
            return Err(CommandError::usage(
                "rename -m requires file inputs, not standard input",
            ));
        }
        std::fs::create_dir_all(&out_dir)
            .map_err(|error| CommandError::io(format!("{out_dir}: {error}")))?;
        let nonempty = std::fs::read_dir(&out_dir)
            .map_err(|error| CommandError::io(format!("{out_dir}: {error}")))?
            .next()
            .is_some();
        if nonempty && !options.force {
            eprintln!("[WARN] outdir not empty: {out_dir}, you can use --force to overwrite");
        }
    }
    let mut out = if multiple {
        None
    } else {
        Some(Output::create_with_level(
            options.out_file.as_deref(),
            options.compress_level,
        )?)
    };
    let mut counts: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    for input in &inputs {
        if multiple {
            let name = Path::new(input)
                .file_name()
                .ok_or_else(|| CommandError::input(format!("{input}: no file name")))?;
            let path = Path::new(&out_dir).join(name);
            out = Some(Output::create_with_level(
                Some(path.to_string_lossy().as_ref()),
                options.compress_level,
            )?);
        }
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let mut record = record?;
            let key = if options.by_name {
                record.name.clone()
            } else {
                record.id().to_vec()
            };
            let count = counts.entry(key).or_insert(0);
            *count += 1;
            let renamed = *count > 1 || options.rename_first;
            if renamed {
                let suffix = format!(
                    "{}{}",
                    options.separator,
                    if *count == 1 {
                        1
                    } else {
                        options.start_num + *count - 2
                    }
                );
                if options.by_name {
                    record.name.extend_from_slice(suffix.as_bytes());
                } else {
                    let id_len = record.id().len();
                    record.name.splice(id_len..id_len, suffix.bytes());
                }
                let marker = record.raw_header.first().copied().unwrap_or(b'>');
                record.raw_header = format!(
                    "{}{}",
                    char::from(marker),
                    String::from_utf8_lossy(&record.name)
                )
                .into_bytes();
            }
            record.write_to(
                out.as_mut().expect("output is initialized"),
                layout,
                options.line_width,
            )?;
        }
        if multiple {
            out.take().expect("output is initialized").finish()?;
        }
    }
    if multiple {
        Ok(())
    } else {
        out.take().expect("output is initialized").finish()
    }
}

// --------------------------------------------------------------------------- //
// replace
// --------------------------------------------------------------------------- //

pub use crate::replace::ReplaceOptions;

/// Run `replace`.
pub fn replace(options: ReplaceOptions) -> Result<(), CommandError> {
    crate::replace::run(options)
}

// --------------------------------------------------------------------------- //
// tab2fx
// --------------------------------------------------------------------------- //

/// The parsed `tab2fx` options.
pub struct Tab2fxOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// Comment-line prefixes to skip; defaults to `#` and `//`.
    pub comment_prefixes: Vec<String>,
    /// `-b`: maximum tabular line size in bytes, including its newline.
    /// Negative values retain SeqKit's scanner-error behavior.
    pub buffer_size: i64,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `tab2fx`.
pub fn tab2fx(options: Tab2fxOptions) -> Result<(), CommandError> {
    crate::tab2fx::run(options)
}

// --------------------------------------------------------------------------- //
// restart
// --------------------------------------------------------------------------- //

/// The parsed `restart` options.
pub struct RestartOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-i`: 1-based new start; negative counts from the end.
    pub new_start: i64,
    /// `-s`: rotate to this subsequence instead of a numeric start.
    pub start_with: Option<String>,
    /// `-I`: ignore case while finding `--start-with`.
    pub ignore_case: bool,
    /// `-m`: maximum mismatches while finding `--start-with`.
    pub max_mismatch: usize,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

fn find_motif(
    sequence: &[u8],
    motif: &[u8],
    ignore_case: bool,
    max_mismatch: usize,
) -> Option<usize> {
    if motif.is_empty() {
        return Some(0);
    }
    if motif.len() > sequence.len() {
        return None;
    }
    sequence.windows(motif.len()).position(|window| {
        window
            .iter()
            .zip(motif)
            .filter(|(left, right)| {
                if ignore_case {
                    !left.eq_ignore_ascii_case(right)
                } else {
                    left != right
                }
            })
            .count()
            <= max_mismatch
    })
}

fn rotate_record(record: &mut OwnedRecord, start: usize, _reverse: bool) {
    let length = record.sequence.len();
    if length == 0 {
        return;
    }
    let start = start.min(length - 1);
    let mut sequence = Vec::with_capacity(length);
    sequence.extend_from_slice(&record.sequence[start..]);
    sequence.extend_from_slice(&record.sequence[..start]);
    record.sequence = sequence;
    if record.has_quality_line {
        let mut quality = Vec::with_capacity(length);
        quality.extend_from_slice(&record.qualities[start..]);
        quality.extend_from_slice(&record.qualities[..start]);
        record.qualities = quality;
    }
}

/// Run `restart -i` or `restart -s`.
pub fn restart(options: RestartOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let mut record = record?;
            if let Some(motif) = &options.start_with {
                let motif = motif.as_bytes();
                if let Some(start) = find_motif(
                    &record.sequence,
                    motif,
                    options.ignore_case,
                    options.max_mismatch,
                ) {
                    rotate_record(&mut record, start, false);
                } else {
                    let reverse = transform::reverse_complement(&record.sequence);
                    if let Some(start) =
                        find_motif(&reverse, motif, options.ignore_case, options.max_mismatch)
                    {
                        record.sequence = reverse;
                        if record.has_quality_line {
                            record.qualities.reverse();
                        }
                        rotate_record(&mut record, start, true);
                    } else {
                        eprintln!(
                            "[WARN] the given subsequence is not found in both strands of sequence: {}",
                            String::from_utf8_lossy(record.id())
                        );
                        continue;
                    }
                }
                record.write_to(&mut out, layout, options.line_width)?;
                continue;
            }
            let length = record.sequence.len();
            if length == 0 {
                record.write_to(&mut out, layout, options.line_width)?;
                continue;
            }
            let start = if options.new_start > 0 {
                (options.new_start - 1) as usize
            } else {
                (length as i64 + options.new_start) as usize
            }
            .min(length - 1);
            let mut sequence = Vec::with_capacity(length);
            sequence.extend_from_slice(&record.sequence[start..]);
            sequence.extend_from_slice(&record.sequence[..start]);
            record.sequence = sequence;
            if record.has_quality_line {
                let mut quality = Vec::with_capacity(length);
                quality.extend_from_slice(&record.qualities[start..]);
                quality.extend_from_slice(&record.qualities[..start]);
                record.qualities = quality;
            }
            record.write_to(&mut out, layout, options.line_width)?;
        }
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// fa2fq
// --------------------------------------------------------------------------- //

/// The parsed `fa2fq` options.
pub struct Fa2fqOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// FASTA file whose names select FASTQ records.
    pub fasta: String,
    /// FASTQ inputs; empty means stdin.
    pub inputs: Vec<String>,
    /// `-P`: only compare the positive strand.
    pub only_positive: bool,
    /// `-w`: FASTA line width (unused for FASTQ).
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

/// Run `fa2fq`.
pub fn fa2fq(options: Fa2fqOptions) -> Result<(), CommandError> {
    let mut queries: std::collections::BTreeMap<Vec<u8>, Vec<OwnedRecord>> =
        std::collections::BTreeMap::new();
    let mut fasta = RecordReader::open(&options.fasta)?;
    while let Some(record) = fasta.next_record() {
        let record = record?;
        queries.entry(record.name.clone()).or_default().push(record);
    }
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        while let Some(record) = reader.next_record() {
            let record = record?;
            let Some(candidates) = queries.get(&record.name) else {
                continue;
            };
            for query in candidates {
                if query.sequence.is_empty() {
                    continue;
                }
                if let Some(start) = find_subsequence(&record.sequence, &query.sequence) {
                    let end = start + query.sequence.len();
                    let mut matched = record.clone();
                    matched.sequence = record.sequence[start..end].to_vec();
                    matched.qualities = record.qualities[start..end].to_vec();
                    matched.write_to(&mut out, Layout::Fastq, options.line_width)?;
                } else if !options.only_positive {
                    let reverse = transform::reverse_complement(&query.sequence);
                    if let Some(start) = find_subsequence(&record.sequence, &reverse) {
                        let end = start + reverse.len();
                        let mut matched = record.clone();
                        matched.sequence = query.sequence.clone();
                        matched.qualities =
                            record.qualities[start..end].iter().rev().copied().collect();
                        matched.write_to(&mut out, Layout::Fastq, options.line_width)?;
                    }
                }
            }
        }
    }
    out.finish()
}

fn find_subsequence(sequence: &[u8], query: &[u8]) -> Option<usize> {
    if query.is_empty() || query.len() > sequence.len() {
        return None;
    }
    sequence
        .windows(query.len())
        .position(|window| window == query)
}

// --------------------------------------------------------------------------- //
// translate
// --------------------------------------------------------------------------- //

pub use crate::translate::TranslateOptions;

/// Run `translate`.
pub fn translate(options: TranslateOptions) -> Result<(), CommandError> {
    crate::translate::run(options)
}

// grep
// --------------------------------------------------------------------------- //

pub use crate::grep::GrepOptions;

/// Run `grep`.
pub fn grep(options: GrepOptions) -> Result<(), CommandError> {
    crate::grep::run(options)
}

// --------------------------------------------------------------------------- //
// sort
// --------------------------------------------------------------------------- //

/// The parsed `sort` options.
pub struct SortOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-l`: sort by sequence length.
    pub by_length: bool,
    /// `-n`: sort by full name.
    pub by_name: bool,
    /// `-s`: sort by sequence.
    pub by_seq: bool,
    /// `-b`: sort by non-gap bases.
    pub by_bases: bool,
    /// `-i`: ignore case.
    pub ignore_case: bool,
    /// `-r`: reverse the result.
    pub reverse: bool,
    /// `-G`: gap letters for `-b`.
    pub gap_letters: Vec<u8>,
    /// `-L`: sequence prefix length used for sequence sorting; 0 means whole sequence.
    pub seq_prefix_length: usize,
    /// `-N`: use natural ordering for IDs/full names.
    pub natural_order: bool,
    /// `-2`: use metadata and byte offsets instead of buffering all FASTA records.
    pub two_pass: bool,
    /// `-U`: rebuild an existing `.seqkit.fai`.
    pub update_faidx: bool,
    /// `-k`: retain temporary files when a two-pass path needs them.
    pub keep_temp: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

fn sort_text(bytes: &[u8], ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        bytes.to_ascii_lowercase()
    } else {
        bytes.to_vec()
    }
}

/// Run `sort`.
pub fn sort(options: SortOptions) -> Result<(), CommandError> {
    let modes = [
        options.by_length,
        options.by_name,
        options.by_seq,
        options.by_bases,
    ]
    .iter()
    .filter(|flag| **flag)
    .count();
    if modes > 1 {
        return Err(CommandError::usage(
            "only one of -l/--by-length, -n/--by-name, -s/--by-seq and -b/--by-bases may be given",
        ));
    }
    if options.two_pass {
        return crate::sort::two_pass(&options);
    }
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut records: Vec<(OwnedRecord, Layout)> = Vec::new();
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            records.push((record?, layout));
        }
    }
    records.sort_by(|(left, _), (right, _)| {
        let ordering = if options.by_length {
            left.sequence.len().cmp(&right.sequence.len())
        } else if options.by_bases {
            let count = |sequence: &[u8]| {
                sequence
                    .iter()
                    .filter(|base| !options.gap_letters.contains(base))
                    .count()
            };
            count(&left.sequence).cmp(&count(&right.sequence))
        } else if options.by_seq {
            let prefix_length = if options.seq_prefix_length == 0 {
                usize::MAX
            } else {
                options.seq_prefix_length
            };
            let left_sequence = &left.sequence[..left.sequence.len().min(prefix_length)];
            let right_sequence = &right.sequence[..right.sequence.len().min(prefix_length)];
            let ordering = sort_text(left_sequence, options.ignore_case)
                .cmp(&sort_text(right_sequence, options.ignore_case));
            if ordering == std::cmp::Ordering::Equal && prefix_length > 0 {
                sort_text(&left.sequence, options.ignore_case)
                    .cmp(&sort_text(&right.sequence, options.ignore_case))
            } else {
                ordering
            }
        } else if options.by_name {
            if options.natural_order {
                crate::sort::natural_cmp(&left.name, &right.name, options.ignore_case)
            } else {
                sort_text(&left.name, options.ignore_case)
                    .cmp(&sort_text(&right.name, options.ignore_case))
            }
        } else if options.natural_order {
            crate::sort::natural_cmp(left.id(), right.id(), options.ignore_case)
        } else {
            sort_text(left.id(), options.ignore_case)
                .cmp(&sort_text(right.id(), options.ignore_case))
        };
        if options.reverse {
            ordering.reverse()
        } else {
            ordering
        }
    });
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for (record, layout) in records {
        record.write_to(&mut out, layout, options.line_width)?;
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// fq2fa
// --------------------------------------------------------------------------- //

/// The parsed `fq2fa` options.
pub struct Fq2faOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-w`: FASTA line width (0 disables wrapping).
    pub line_width: usize,
    /// `-o`: output file (`-` or absent means stdout).
    pub out_file: Option<String>,
}

/// Run `fq2fa`.
///
/// idempotent for FASTA input: a FASTA read in and written out is unchanged
/// apart from `-w` wrapping, which is what makes `fq2fa` safe to pipe.
pub fn fq2fa(options: Fq2faOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let mut record = OwnedRecord::default();
        while reader.read_record(&mut record)? {
            record.write_to(&mut out, Layout::Fasta, options.line_width)?;
        }
    }
    out.finish()
}

// --------------------------------------------------------------------------- //
// fx2tab
// --------------------------------------------------------------------------- //

/// The parsed `fx2tab` options.
pub struct Fx2tabOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-H`: print a header line naming the metrics.
    pub header_line: bool,
    /// `-l`: append the sequence length.
    pub length: bool,
    /// `-g`: append `(G+C)/(G+C+A+T)` as a percentage.
    pub gc: bool,
    /// `-G`: append `(G-C)/(G+C)`.
    pub gc_skew: bool,
    /// `-n`: print names only, dropping the sequence columns.
    pub name: bool,
    /// `-i`: print the ID rather than the full header.
    pub only_id: bool,
    /// `-Q`: omit the quality column.
    pub no_qual: bool,
    /// `-q`: append the mean-error average quality.
    pub avg_qual: bool,
    /// `-s`: append the MD5 of the sequence.
    pub seq_hash: bool,
    /// `-a`: append the distinct letters present.
    pub alphabet: bool,
    /// `-I`: keep case when hashing and counting.
    pub case_sensitive: bool,
    /// `-B`: base-content percentages, one column per value.
    pub base_content: Vec<String>,
    /// `-C`: base counts, one column per value.
    pub base_count: Vec<String>,
    /// `--basename`: print only the file's base name.
    pub basename: bool,
    /// `-f`: prepend the input file name.
    pub file_name: bool,
    /// `-b`: ASCII offset for quality decoding.
    pub quality_base: u8,
    /// `--stdin-label`: label used in place of `-`.
    pub stdin_label: String,
    /// `-o`: output file (`-` or absent means stdout).
    pub out_file: Option<String>,
}

/// Run `fx2tab`.
pub fn fx2tab(options: Fx2tabOptions) -> Result<(), CommandError> {
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    if options.header_line {
        let mut columns: Vec<String> = Vec::new();
        if options.file_name || options.basename {
            columns.push("file".to_string());
        }
        // `-H` prints the first column as `#name`, so a consumer can tell the
        // header line from a record line whose name is literally "name".
        columns.push("#name".to_string());
        if !options.name {
            columns.push("seq".to_string());
            // The documented rule: three fixed columns for either format, unless
            // -n/--name or -Q/--no-qual is given. FASTA therefore prints an empty
            // quality column rather than omitting it.
            if !options.no_qual {
                columns.push("qual".to_string());
            }
        }
        if options.length {
            columns.push("length".to_string());
        }
        if options.gc {
            columns.push("GC".to_string());
        }
        if options.gc_skew {
            columns.push("GC-Skew".to_string());
        }
        // SeqKit prints every `-C` (count) column before every `-B` (content)
        // column, regardless of the order the flags appear on the command line,
        // and labels each column with the bare spec string -- `GC`, `AT` -- with
        // no `(%)` or `(count)` suffix:
        //
        //   $ seqkit fx2tab -H -B AT -C GC in.fa
        //   #name  seq  qual  GC  AT
        //
        //   $ seqkit fx2tab -H -C GC -C AT in.fa
        //   #name  seq  qual  GC  AT
        for spec in &options.base_count {
            columns.push(spec.clone());
        }
        for spec in &options.base_content {
            columns.push(spec.clone());
        }
        if options.alphabet {
            columns.push("alphabet".to_string());
        }
        if options.avg_qual {
            columns.push("avg.qual".to_string());
        }
        if options.seq_hash {
            columns.push("seq.hash".to_string());
        }
        writeln!(out, "{}", columns.join("\t"))?;
    }

    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let input_format = reader.format();
        let file_label = if input == "-" {
            options.stdin_label.clone()
        } else if options.basename {
            Path::new(input)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| input.to_string())
        } else {
            input.to_string()
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            let mut columns: Vec<String> = Vec::new();
            if options.file_name || options.basename {
                columns.push(file_label.clone());
            }
            let name = if options.only_id {
                String::from_utf8_lossy(record.id()).into_owned()
            } else {
                String::from_utf8_lossy(&record.name).into_owned()
            };
            columns.push(name);
            if !options.name {
                columns.push(String::from_utf8_lossy(&record.sequence).into_owned());
                if !options.no_qual {
                    columns.push(String::from_utf8_lossy(&record.qualities).into_owned());
                }
            }
            if options.length {
                columns.push(record.sequence.len().to_string());
            }
            let seq_stats = SeqStats::from_sequence(&record.sequence, DEFAULT_GAP_LETTERS);
            if options.gc {
                // `fx2tab -g` uses the unambiguous denominator and prints
                // `NaN` when there are no G/C/A/T bases; `stats` uses the
                // length denominator and prints 0.00. See stats::gc_percent.
                columns.push(match seq_stats.gc_percent_unambiguous() {
                    Some(value) => table::percent(value),
                    None => "NaN".to_string(),
                });
            }
            if options.gc_skew {
                let skew = GcSkewCounts::from_sequence(&record.sequence).skew();
                columns.push(table::percent(skew.unwrap_or(0.0)));
            }
            // Counts first, then contents: see the header comment above.
            for spec in &options.base_count {
                columns.push(base_count(&record.sequence, spec).to_string());
            }
            for spec in &options.base_content {
                columns.push(table::percent(base_content(&record.sequence, spec)));
            }
            if options.alphabet {
                columns.push(alphabet(&record.sequence));
            }
            if options.avg_qual {
                if input_format == InputFormat::Fastq {
                    let average =
                        QualStats::from_qualities(&record.qualities, options.quality_base)
                            .average_quality()
                            .unwrap_or(0.0);
                    // `-q` prints the mean with exactly two decimals, like
                    // `stats -a`'s AvgQual: 40.00, 20.00, 0.00.
                    columns.push(table::percent(average));
                } else {
                    columns.push(String::new());
                }
            }
            if options.seq_hash {
                // SeqKit's `-s` MD5s the *lower-cased* sequence, not the upper.
                //
                // Verified against SeqKit 2.14.0: for `ACGTACGT` it prints
                // db819856e873893888e8ac8f72cae1f8, which is md5("acgtacgt");
                // md5("ACGTACGT") is cc0af3a4fedb18378b4b57b98068e69f. Upper and
                // lower inputs produce the same hash, and mixed case hashes its
                // lower-cased form. `--case-sensitive` is not a SeqKit flag on
                // this subcommand, so there is no upper-case mode to honour.
                let lower = crate::transform::to_lower(&record.sequence);
                columns.push(crate::md5::hex_digest(&lower));
            }
            writeln!(out, "{}", columns.join("\t"))?;
        }
    }
    out.finish()
}

/// Fraction of `sequence` made up of the letters in `spec`, as a percentage.
///
/// The documented denominator is the sequence length, not the number of matching
/// letters' complement.
fn base_content(sequence: &[u8], spec: &str) -> f64 {
    if sequence.is_empty() {
        return 0.0;
    }
    let count = base_count(sequence, spec);
    count as f64 * 100.0 / sequence.len() as f64
}

/// Number of bytes in `sequence` matching any letter in `spec`, case-insensitive.
fn base_count(sequence: &[u8], spec: &str) -> u64 {
    let wanted: Vec<u8> = spec.bytes().map(|b| b.to_ascii_uppercase()).collect();
    sequence
        .iter()
        .filter(|byte| wanted.contains(&byte.to_ascii_uppercase()))
        .count() as u64
}

/// The distinct letters present, sorted.
fn alphabet(sequence: &[u8]) -> String {
    let mut seen: Vec<u8> = sequence.iter().map(|b| b.to_ascii_uppercase()).collect();
    seen.sort_unstable();
    seen.dedup();
    String::from_utf8_lossy(&seen).into_owned()
}

// --------------------------------------------------------------------------- //
// sample
// --------------------------------------------------------------------------- //

/// The parsed `sample` options.
pub struct SampleOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-p`: keep each record with this probability.
    pub proportion: Option<f64>,
    /// `-n`: keep about this many records.
    pub number: Option<u64>,
    /// `-s`: seed; the same seed reproduces the selection.
    pub rand_seed: u64,
    /// `-r`: use a time-based seed instead.
    pub non_deterministic: bool,
    /// `-w`: FASTA line width (0 disables wrapping).
    pub line_width: usize,
    /// `-o`: output file (`-` or absent means stdout).
    pub out_file: Option<String>,
}

/// Run `sample`.
///
/// Sampling is one pass and streaming: the record count is not known in advance,
/// so `-n` is implemented exactly as SeqKit documents it ("may return fewer than
/// requested") by first counting, then sampling by the derived probability. Two
/// passes over a stream is not possible, so a stream is buffered when `-n` is
/// used and the input is stdin.
pub fn sample(options: SampleOptions) -> Result<(), CommandError> {
    if options.proportion.is_some() == options.number.is_some() {
        return Err(CommandError::usage(
            "exactly one of -p/--proportion and -n/--number must be given",
        ));
    }
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let seed = if options.non_deterministic {
        // `-r/--non-deterministic` asks for a varying seed by design.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(options.rand_seed)
    } else {
        options.rand_seed
    };

    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        // `-p` needs no total. `-n` does, so that input is counted once first;
        // SeqKit documents the same cost for `-n` without `--two-pass`.
        let total = match options.number {
            Some(_) => count_records(input)?,
            None => 0,
        };
        let strategy = match (options.proportion, options.number) {
            (Some(proportion), None) => Strategy::Proportion(proportion),
            (None, Some(number)) => Strategy::Number(number),
            _ => {
                return Err(CommandError::usage(
                    "exactly one of -p/--proportion and -n/--number must be given",
                ))
            }
        };
        let mut sampler = Sampler::for_strategy(seed, strategy, total);

        let mut reader = RecordReader::open(input)?;
        let input_format = reader.format();
        let layout = match input_format {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            if sampler.keep_next() {
                record.write_to(&mut out, layout, options.line_width)?;
            }
        }
    }
    out.finish()
}

/// Count the records in one input.
fn count_records(input: &str) -> Result<u64, CommandError> {
    let mut reader = RecordReader::open(input)?;
    let mut count = 0u64;
    while let Some(record) = reader.next_record() {
        record?;
        count += 1;
    }
    Ok(count)
}

// --------------------------------------------------------------------------- //
// split
// --------------------------------------------------------------------------- //

/// The parsed `split` options.
pub struct SplitOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// The input file; `split` needs a real file to name parts after.
    pub input: String,
    /// `-s/--by-size` or `-p/--by-part`.
    pub by: SplitBy,
    /// `-O`: output directory (default `<input>.split`).
    pub out_dir: Option<String>,
    /// `-P`: file-name prefix for the parts.
    pub out_prefix: Option<String>,
    /// `-W`: zero-padded width of the part number.
    pub part_width: usize,
    /// `-e`: override the part file extension.
    pub extension: Option<String>,
    /// `-d`: print what would be written, create nothing.
    pub dry_run: bool,
    /// `-f`: accepted for compatibility; parts are always overwritten.
    pub force: bool,
}

/// How a split is planned without buffering the input records.
#[derive(Clone, Copy, Debug)]
pub struct PartPlan {
    total: u64,
    by: SplitBy,
    parts: u64,
}

impl PartPlan {
    /// Plan a split without buffering the input records.
    pub fn new(total: u64, by: SplitBy) -> Result<PartPlan, CommandError> {
        let parts = match by {
            SplitBy::Size(size) => {
                if size == 0 {
                    return Err(CommandError::usage("--by-size must be at least 1"));
                }
                split::parts_for_size(total, size)
            }
            SplitBy::Parts(parts) => {
                if parts == 0 {
                    return Err(CommandError::usage("--by-part must be at least 1"));
                }
                parts.min(total)
            }
        };
        Ok(PartPlan { total, by, parts })
    }

    /// Number of parts this plan will produce.
    pub fn parts(&self) -> u64 {
        self.parts
    }

    /// Half-open record range for 0-based part number `part`.
    pub fn range(&self, part: u64) -> (u64, u64) {
        match self.by {
            SplitBy::Size(size) => {
                let start = part * size;
                (start, (start + size).min(self.total))
            }
            SplitBy::Parts(parts) => {
                let parts = parts.min(self.total);
                let base = self.total / parts;
                let remainder = self.total % parts;
                if part < remainder {
                    let start = part * (base + 1);
                    (start, start + base + 1)
                } else {
                    let start = remainder * (base + 1) + (part - remainder) * base;
                    (start, start + base)
                }
            }
        }
    }

    /// Part number containing a 0-based record index.
    pub fn index(&self, record: u64) -> u64 {
        match self.by {
            SplitBy::Size(size) => record / size,
            SplitBy::Parts(parts) => {
                let parts = parts.min(self.total);
                let base = self.total / parts;
                let remainder = self.total % parts;
                let extra_end = remainder * (base + 1);
                if record < extra_end {
                    record / (base + 1)
                } else {
                    remainder + (record - extra_end) / base
                }
            }
        }
    }
}

/// Run `split`.
pub fn split(options: SplitOptions) -> Result<(), CommandError> {
    if options.input == "-" {
        return Err(CommandError::usage(
            "split needs a real input file: parts are named after it",
        ));
    }

    // Two passes: one to learn the total (needed for --by-part and the last
    // by-size part), then a streaming pass that never holds more than one record.
    let total = count_records(&options.input)?;
    let plan = PartPlan::new(total, options.by)?;
    let input = Path::new(&options.input);
    let out_dir: PathBuf = match &options.out_dir {
        Some(dir) => PathBuf::from(dir),
        None => split::default_out_dir(input),
    };
    let stem = match &options.out_prefix {
        Some(prefix) => prefix.clone(),
        None => split::part_stem(input),
    };
    let extension = split::part_extension(input);
    let layout = {
        let reader = RecordReader::open(&options.input)?;
        match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        }
    };

    if plan.parts() == 0 {
        return Ok(());
    }

    if options.dry_run {
        // Diagnostics go to stderr, as SeqKit's do: its stdout stays clean so
        // a part file or a `--json` report is the only thing on stdout.
        for part in 0..plan.parts() {
            let (start, end) = plan.range(part);
            let path = split::part_path(
                &out_dir,
                &stem,
                part + 1,
                options.part_width,
                &extension,
                options.extension.as_deref(),
            );
            eprintln!(
                "[INFO] write {} sequences to file: {}",
                end - start,
                path.display()
            );
        }
        return Ok(());
    }

    std::fs::create_dir_all(&out_dir).map_err(|error| {
        CommandError::io(format!("cannot create {}: {error}", out_dir.display()))
    })?;

    let mut reader = RecordReader::open(&options.input)?;
    let mut current: Option<(u64, Output, u64, PathBuf)> = None;
    let mut index = 0u64;
    while let Some(record) = reader.next_record() {
        let record = record?;
        let part = plan.index(index);
        if current
            .as_ref()
            .is_none_or(|(open_part, _, _, _)| *open_part != part)
        {
            if let Some((_, out, written, path)) = current.take() {
                out.finish()?;
                eprintln!(
                    "[INFO] write {written} sequences to file: {}",
                    path.display()
                );
            }
            let path = split::part_path(
                &out_dir,
                &stem,
                part + 1,
                options.part_width,
                &extension,
                options.extension.as_deref(),
            );
            let out =
                Output::create_with_level(Some(&path.to_string_lossy()), options.compress_level)?;
            current = Some((part, out, 0, path));
        }
        let (_, out, written, _) = current.as_mut().expect("part was just opened");
        record.write_to(out, layout, 0)?;
        *written += 1;
        index += 1;
    }
    if let Some((_, out, written, path)) = current.take() {
        out.finish()?;
        eprintln!(
            "[INFO] write {written} sequences to file: {}",
            path.display()
        );
    }
    Ok(())
}

// --------------------------------------------------------------------------- //
// lint
// --------------------------------------------------------------------------- //

/// The parsed `lint` options.
pub struct LintOptions {
    /// Read 1 source.
    pub r1: String,
    /// Read 2 source, when validating a pair.
    pub r2: Option<String>,
    /// `--lint-mode log`: report every error instead of the first.
    pub log_mode: bool,
    /// `--single-read-validation-level` ceiling.
    pub single_level: Level,
    /// `--paired-read-validation-level` ceiling.
    pub paired_level: Level,
    /// `--disable-validator` codes.
    pub disable: Vec<String>,
}

/// An INFO log line with no file position, in fq's format.
///
/// The `target` field is not always `fq::commands::lint`: fq emits the validator
/// inventory from `fq::validators`, and the harness compares the target too, so
/// each line has to name the module fq names.
fn fq_log_message(message: &str) -> String {
    fq_log_from("fq::commands::lint", message)
}

/// An INFO log line from a named target.
fn fq_log_from(target: &str, message: &str) -> String {
    format!("{}  INFO {target}: {message}", log_timestamp())
}

/// Render a validator list the way fq prints it: `"[S003] NameValidator", ...`.
///
/// fq's order is its registry order, which is by level then insertion, not code
/// order: high-level validators first for the enabled list at the default level.
/// Reproducing that order matters because the line is compared byte for byte
/// after timestamp normalisation.
fn render_validators(validators: &[crate::lint::Validator]) -> String {
    use crate::lint::Validator;
    // fq's registry order, verified from its own output:
    //   S003, S004, S002, S001, S005, S006
    const ORDER: [&str; 6] = ["S003", "S004", "S002", "S001", "S005", "S006"];
    let mut sorted: Vec<&Validator> = validators.iter().collect();
    sorted.sort_by_key(|v| {
        ORDER
            .iter()
            .position(|code| *code == v.code())
            .unwrap_or(usize::MAX)
    });
    sorted
        .iter()
        .map(|v| format!("\"[{}] {}Validator\"", v.code(), v.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format one log line the way fq 0.12.0 does.
///
/// fq uses a Rust `tracing` subscriber whose text format is
///
///   <RFC3339 timestamp> <LEVEL> <target>: <message>
///
/// and prints it to **stdout**. The timestamp is wall-clock, so a byte-level
/// comparison of two runs can never match; the harness normalises it. Everything
/// after the timestamp -- the level, the target and the message -- does match,
/// including the `<file>:<line>:<col>: ` prefix on an error.
fn fq_log_line(level: &str, file: &str, error: &crate::lint::LintError) -> String {
    // With colour disabled, `tracing` pads the level to five characters, so INFO
    // becomes " INFO" and ERROR stays "ERROR":
    //
    //   NO_COLOR=1 fq lint ok.fq
    //   2026-10-20T...Z  INFO fq::commands::lint: fq-lint start
    //
    // The harness runs the oracle with the ambient environment, so the padding is
    // reproduced here rather than normalised away.
    let padded = format!("{level:>5}");
    format!(
        "{} {padded} fq::commands::lint: {}",
        log_timestamp(),
        error.render_at(file)
    )
}

/// A UTC timestamp in the shape fq's logger emits, e.g.
/// `2026-10-20T09:13:55.850350Z`.
///
/// Seconds resolution would be enough for a human, but the harness normalises
/// this field away, so the exact value is not contractual -- only its presence
/// and shape are.
fn log_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let micros = now.subsec_micros();
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}Z")
}

/// Convert days since 1970-01-01 to a (year, month, day) civil date.
///
/// Howard Hinnant's `civil_from_days`. Kept here rather than pulling in `chrono`
/// so the crate gains no dependency.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Run `lint`.
///
/// Exit codes: 0 when every enabled validator passes, 3 when any fails (an input
/// precondition violation), 2 for a usage error such as an unknown validator code.
pub fn lint(options: LintOptions) -> Result<(), CommandError> {
    // Validate the codes once, through a throwaway linter, so an unknown code is a
    // usage error before any input is opened.
    let unknown = {
        let mut probe = Linter::new();
        probe.disable(&options.disable)
    };
    if !unknown.is_empty() {
        return Err(CommandError::usage(format!(
            "unknown validator code(s): {}; known codes are S001-S007 and P001",
            unknown.join(", ")
        )));
    }

    let mut errors = 0u64;
    let mut first_error: Option<crate::lint::LintError> = None;
    let mut read1_names: Vec<Vec<u8>> = Vec::new();
    let mut records_seen = 0u64;

    // fq brackets a lint run with INFO lines on stdout:
    //
    //   fq-lint start
    //   validating single end read          (or: validating paired end reads)
    //   disabled validators: [...]
    //   enabled single read validators: ["[S003] NameValidator", ...]
    //   enabled paired read validators: [...]
    //   starting validation
    //   read N records
    //   fq-lint end
    //
    // They are emitted here rather than via a logging dependency so the crate
    // stays stdlib-only. The timestamp is normalised by the harness.
    println!("{}", fq_log_message("fq-lint start"));
    if options.r2.is_some() {
        println!("{}", fq_log_message("validating paired end reads"));
    } else {
        println!("{}", fq_log_message("validating single end read"));
    }
    println!(
        "{}",
        fq_log_from(
            "fq::validators",
            &format!(
                "disabled validators: [{}]",
                options
                    .disable
                    .iter()
                    .map(|code| format!("\"{code}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        )
    );

    // Read 1 and read 2 are separate name spaces: a pair legitimately uses the
    // same name in both files, so S007 must not see one file's names while
    // validating the other. Each mate therefore gets its own linter.
    let mut linter1 = Linter::new()
        .with_single_level(options.single_level)
        .with_paired_level(options.paired_level);
    linter1.disable(&options.disable);
    let mut linter2 = Linter::new()
        .with_single_level(options.single_level)
        .with_paired_level(options.paired_level);
    linter2.disable(&options.disable);

    // fq lists each enabled validator as "[S003] NameValidator", in the order its
    // own registry iterates, which is not code order.
    let enabled_single = render_validators(&linter1.active_single());
    // fq enables the paired validators only when a second input is present; with
    // one input the list it prints is empty.
    let enabled_paired = if options.r2.is_some() {
        render_validators(&linter2.active_paired())
    } else {
        String::new()
    };
    println!(
        "{}",
        fq_log_from(
            "fq::validators",
            &format!("enabled single read validators: [{enabled_single}]"),
        )
    );
    println!(
        "{}",
        fq_log_from(
            "fq::validators",
            &format!("enabled paired read validators: [{enabled_paired}]"),
        )
    );
    // fq lists its "special" validators on their own line -- but only for a paired
    // run, and only after the paired list. A single-end run prints no such line.
    let paired = options.r2.is_some();
    if paired {
        println!(
            "{}",
            fq_log_message("enabled special validators: [\"[S007] DuplicateNameValidator\"]")
        );
    }
    // A paired run validates twice and says so; a single-end run has one pass.
    println!(
        "{}",
        fq_log_message(if paired {
            "starting validation (pass 1)"
        } else {
            "starting validation"
        })
    );

    // `lint` must see a malformed record rather than reject it in the parser: a
    // length mismatch or a bad header IS the thing it validates. The lenient
    // reader therefore delivers records the strict parser would stop on, and the
    // validators decide.
    {
        let mut reader = LintReader::open(&options.r1)?;
        let mut record = OwnedRecord::default();
        while reader.read_record(&mut record)? {
            if !record.is_fastq() {
                return Err(CommandError::input(format!(
                    "{}: fq lint validates FASTQ, but this input is FASTA",
                    options.r1
                )));
            }
            let lint_record = LintRecord {
                header: &record.name,
                raw_header: &record.raw_header,
                sequence: &record.sequence,
                plus: &record.plus,
                qualities: record
                    .has_quality_line
                    .then_some(record.qualities.as_slice()),
            };
            let found = linter1.check_record(&lint_record);
            records_seen += 1;
            if options.r2.is_some() {
                // Raw header, with its '@', because fq quotes it verbatim in a P001
                // message: expected '@other', got '@p2'.
                read1_names.push(record.raw_header.clone());
            }
            for error in found {
                errors += 1;
                if options.log_mode {
                    println!("{}", fq_log_line("ERROR", &options.r1, &error));
                }
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            if !options.log_mode && errors > 0 {
                break;
            }
        }
    }

    if let Some(r2) = &options.r2 {
        let mut reader = LintReader::open(r2)?;
        let mut index = 0usize;
        let mut record = OwnedRecord::default();
        while reader.read_record(&mut record)? {
            if !record.is_fastq() {
                return Err(CommandError::input(format!(
                    "{r2}: fq lint validates FASTQ, but this input is FASTA"
                )));
            }
            let lint_record = LintRecord {
                header: &record.name,
                raw_header: &record.raw_header,
                sequence: &record.sequence,
                plus: &record.plus,
                qualities: record
                    .has_quality_line
                    .then_some(record.qualities.as_slice()),
            };
            let mut found = linter2.check_record(&lint_record);
            if let Some(first) = read1_names.get(index) {
                found.extend(linter2.check_pair(index as u64 + 1, first, &record.raw_header));
            }
            index += 1;
            for error in found {
                errors += 1;
                if options.log_mode {
                    // A pair-name mismatch is reported against read 1's file, which
                    // is where fq reads the naming convention from.
                    let file = if error.report_against_read1 {
                        options.r1.as_str()
                    } else {
                        r2.as_str()
                    };
                    println!("{}", fq_log_line("ERROR", file, &error));
                }
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            if !options.log_mode && errors > 0 {
                break;
            }
        }
        if read1_names.len() != index && !options.log_mode && first_error.is_none() {
            return Err(CommandError::input(format!(
                "{} and {} have different numbers of records ({} vs {})",
                options.r1,
                r2,
                read1_names.len(),
                index
            )));
        }
    }

    // A paired run reports both mates before its second pass, then the read-2
    // count on its own. Sequence from fq 0.12.0:
    //
    //   starting validation (pass 1)
    //   read 1 * 2 records
    //   starting validation (pass 2)
    //   read 1 records
    //   fq-lint end
    if paired {
        println!(
            "{}",
            fq_log_message(&format!("read {records_seen} * {} records", records_seen))
        );
        println!("{}", fq_log_message("starting validation (pass 2)"));
        println!(
            "{}",
            fq_log_message(&format!("read {records_seen} records"))
        );
    } else {
        println!(
            "{}",
            fq_log_message(&format!("read {records_seen} records"))
        );
    }

    // fq logs "fq-lint end" before it exits, including on a validation failure, so
    // the closing line must be emitted before any error return below.
    println!("{}", fq_log_message("fq-lint end"));

    if let Some(error) = first_error {
        // A validation failure is exit 1, matching `fq lint`.
        //
        // fq reports a bad record as a runtime failure, not an input-precondition
        // violation: `fq lint bad.fq; echo $?` prints 1, both in `panic` and `log`
        // mode. Returning ExitClass::Input here produced 3 and every lint case in
        // the differential failed on the exit code alone.
        //
        // In `panic` mode fq still logs the single error it hit, so it is printed
        // here; in `log` mode every error was already printed above.
        if !options.log_mode {
            println!("{}", fq_log_line("ERROR", &options.r1, &error));
        }
        return Err(CommandError::runtime(error.render()));
    }
    if errors > 0 {
        return Err(CommandError::runtime(format!(
            "{errors} validation error(s); see the messages above"
        )));
    }
    Ok(())
}

// --------------------------------------------------------------------------- //
// common
// --------------------------------------------------------------------------- //

/// The parsed `common` options.
pub struct CommonOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; at least two.
    pub inputs: Vec<String>,
    /// `-n`: match by full name.
    pub by_name: bool,
    /// `-s`: match by sequence.
    pub by_seq: bool,
    /// `-e`: match embedded subsequences of exact sequences in the files.
    pub embedded: bool,
    /// `-i`: ignore case.
    pub ignore_case: bool,
    /// `-P`: only the positive strand for -s.
    pub only_positive: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

fn common_key(record: &OwnedRecord, options: &CommonOptions) -> Vec<u8> {
    let mut key = if options.by_seq {
        let mut sequence = record.sequence.clone();
        if options.ignore_case {
            sequence.make_ascii_uppercase();
        }
        if !options.only_positive {
            let reverse = transform::reverse_complement(&sequence);
            if reverse < sequence {
                sequence = reverse;
            }
        }
        sequence
    } else if options.by_name {
        record.name.clone()
    } else {
        record.id().to_vec()
    };
    if options.ignore_case && !options.by_seq {
        key.make_ascii_lowercase();
    }
    key
}

/// Run `common`.
pub fn common(options: CommonOptions) -> Result<(), CommandError> {
    if options.inputs.len() < 2 {
        return Err(CommandError::usage("common needs at least two input files"));
    }
    if options.by_name && options.by_seq {
        return Err(CommandError::usage(
            "only one/none of the flags -s (--by-seq) and -n (--by-name) is allowed",
        ));
    }
    if options.embedded && !options.by_seq {
        return Err(CommandError::usage(
            "flag -s (--by-seq) needed when using -e (--check-embedded-seqs)",
        ));
    }
    if options.embedded {
        return common_embedded(&options);
    }
    let mut first: Vec<(Vec<u8>, OwnedRecord)> = Vec::new();
    let mut shared: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
    for (index, input) in options.inputs.iter().enumerate() {
        let mut reader = RecordReader::open(input)?;
        let mut set = std::collections::BTreeSet::new();
        while let Some(record) = reader.next_record() {
            let record = record?;
            let key = common_key(&record, &options);
            set.insert(key.clone());
            if index == 0 {
                first.push((key, record));
            }
        }
        shared = if index == 0 {
            set
        } else {
            shared.intersection(&set).cloned().collect()
        };
    }
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for (key, record) in first {
        if shared.contains(&key) {
            record.write_to(&mut out, Layout::Fasta, options.line_width)?;
        }
    }
    out.finish()
}

fn sequence_contains(sequence: &[u8], needle: &[u8], options: &CommonOptions) -> bool {
    if needle.is_empty() || needle.len() > sequence.len() {
        return false;
    }
    if sequence
        .windows(needle.len())
        .any(|window| window == needle)
    {
        return true;
    }
    if !options.only_positive {
        let reverse = transform::reverse_complement(needle);
        reverse != needle
            && sequence
                .windows(reverse.len())
                .any(|window| window == reverse)
    } else {
        false
    }
}

fn embedded_candidate(sequence: &[u8], options: &CommonOptions) -> Vec<u8> {
    if options.only_positive {
        return sequence.to_vec();
    }
    let reverse = transform::reverse_complement(sequence);
    if reverse.as_slice() < sequence {
        reverse
    } else {
        sequence.to_vec()
    }
}

fn common_embedded(options: &CommonOptions) -> Result<(), CommandError> {
    let mut first: Vec<OwnedRecord> = Vec::new();
    let mut files: Vec<Vec<Vec<u8>>> = Vec::new();
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    let mut seen: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
    for (index, input) in options.inputs.iter().enumerate() {
        let mut reader = RecordReader::open(input)?;
        let mut sequences = Vec::new();
        while let Some(record) = reader.next_record() {
            let record = record?;
            let mut sequence = record.sequence.clone();
            if options.ignore_case {
                sequence.make_ascii_uppercase();
            }
            let candidate = embedded_candidate(&sequence, options);
            if !candidate.is_empty() && seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
            sequences.push(sequence);
            if index == 0 {
                first.push(record);
            }
        }
        files.push(sequences);
    }

    let common: Vec<Vec<u8>> = candidates
        .into_iter()
        .filter(|candidate| {
            files.iter().all(|sequences| {
                sequences
                    .iter()
                    .any(|sequence| sequence_contains(sequence, candidate, options))
            })
        })
        .collect();

    // A shorter common sequence contained in a longer common sequence is an
    // embedded subsequence; upstream emits only the longer one.
    let maximal: Vec<Vec<u8>> = common
        .iter()
        .filter(|candidate| {
            !common.iter().any(|other| {
                other.len() > candidate.len() && sequence_contains(other, candidate, options)
            })
        })
        .cloned()
        .collect();

    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for record in &first {
        let mut sequence = record.sequence.clone();
        if options.ignore_case {
            sequence.make_ascii_uppercase();
        }

        let mut full = false;
        for candidate in &maximal {
            let reverse_full = !options.only_positive
                && record.sequence.len() == candidate.len()
                && transform::reverse_complement(&sequence) == *candidate;
            if sequence == *candidate || reverse_full {
                full = true;
                break;
            }
        }
        if full {
            record.write_to(&mut out, Layout::Fasta, options.line_width)?;
            continue;
        }

        let mut intervals: Vec<(usize, usize)> = Vec::new();
        for candidate in &maximal {
            // Upstream emits the first embedded occurrence of each candidate;
            // multiple candidates in one record are ordered by coordinate.
            let mut first_interval: Option<(usize, usize)> = None;
            let mut needles = vec![candidate.clone()];
            if !options.only_positive {
                let reverse = transform::reverse_complement(candidate);
                if reverse != *candidate {
                    needles.push(reverse);
                }
            }
            for needle in &needles {
                if needle.is_empty() || needle.len() > sequence.len() {
                    continue;
                }
                if let Some(start) = sequence
                    .windows(needle.len())
                    .position(|window| window == needle.as_slice())
                {
                    let interval = (start, start + needle.len());
                    first_interval = match first_interval {
                        Some(current) if current.0 <= interval.0 => Some(current),
                        _ => Some(interval),
                    };
                }
            }
            if let Some(interval) = first_interval {
                intervals.push(interval);
            }
        }
        intervals.sort_unstable();
        intervals.dedup();

        for (start, end) in intervals {
            let id = record.id();
            let suffix = &record.name[id.len()..];
            let mut name = Vec::with_capacity(id.len() + suffix.len() + 16);
            name.extend_from_slice(id);
            name.extend_from_slice(format!(":{}-{}", start + 1, end).as_bytes());
            name.extend_from_slice(suffix);
            let mut raw_header = Vec::with_capacity(name.len() + 1);
            raw_header.push(b'>');
            raw_header.extend_from_slice(&name);
            let embedded = OwnedRecord {
                name,
                raw_header,
                sequence: record.sequence[start..end].to_vec(),
                plus: Vec::new(),
                qualities: Vec::new(),
                has_quality_line: false,
                index: record.index,
            };
            embedded.write_to(&mut out, Layout::Fasta, options.line_width)?;
        }
    }
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rust-fx-cmd-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    fn write(name: &str, content: &[u8]) -> PathBuf {
        let path = temp(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    // ---- stats -----------------------------------------------------------

    #[test]
    fn stats_reports_the_documented_columns() {
        let input = write(
            "stats.fq",
            b"@r1\nACGT\n+\nIIII\n@r2\nACGTACGT\n+\nIIIIIIII\n",
        );
        let out = temp("stats.out");
        let _ = std::fs::remove_file(&out);
        stats(StatsOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            tabular: true,
            all: false,
            basename: false,
            gap_letters: DEFAULT_GAP_LETTERS.to_vec(),
            stdin_label: "-".to_string(),
            skip_err: false,
            quality_base: 33,
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        let text = read(&out);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "file\tformat\ttype\tnum_seqs\tsum_len\tmin_len\tavg_len\tmax_len"
        );
        assert_eq!(lines.len(), 2);
        let fields: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields[1], "FASTQ");
        assert_eq!(fields[2], "DNA");
        assert_eq!(fields[3], "2");
        assert_eq!(fields[4], "12");
        assert_eq!(fields[5], "4");
        assert_eq!(fields[6], "6.0");
        assert_eq!(fields[7], "8");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn stats_all_adds_the_extra_columns() {
        let input = write("stats_all.fq", b"@r1\nGCAT\n+\nIIII\n");
        let out = temp("stats_all.out");
        let _ = std::fs::remove_file(&out);
        stats(StatsOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            tabular: true,
            all: true,
            basename: false,
            gap_letters: DEFAULT_GAP_LETTERS.to_vec(),
            stdin_label: "-".to_string(),
            skip_err: false,
            quality_base: 33,
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        let text = read(&out);
        let header = text.lines().next().unwrap();
        assert_eq!(header.split('\t').count(), 19, "{header}");
        assert!(header.contains("GC(%)"));
        assert!(header.contains("sum_n"));
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn stats_detects_rna_and_protein() {
        let rna = write("rna.fa", b">r1\nACGUACGU\n");
        let out = temp("rna.out");
        let _ = std::fs::remove_file(&out);
        stats(StatsOptions {
            inputs: vec![rna.to_string_lossy().into_owned()],
            tabular: true,
            all: false,
            basename: false,
            gap_letters: DEFAULT_GAP_LETTERS.to_vec(),
            stdin_label: "-".to_string(),
            skip_err: false,
            quality_base: 33,
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        assert!(read(&out).contains("RNA"));
        let _ = std::fs::remove_file(&rna);
        let _ = std::fs::remove_file(&out);
    }

    // ---- seq -------------------------------------------------------------

    fn seq_options(input: &Path, out: &Path) -> SeqOptions {
        SeqOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            name: false,
            only_id: false,
            seq_only: false,
            qual_only: false,
            transforms: Transforms::default(),
            gap_letters: DEFAULT_GAP_LETTERS.to_vec(),
            min_len: None,
            max_len: None,
            min_qual: None,
            max_qual: None,
            quality_base: 33,
            line_width: 60,
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        }
    }

    #[test]
    fn seq_passes_records_through_unchanged() {
        let input = write("seq.fq", b"@r1 a\nACGT\n+\nIIII\n");
        let out = temp("seq.out");
        let _ = std::fs::remove_file(&out);
        seq(seq_options(&input, &out)).unwrap();
        assert_eq!(read(&out), "@r1 a\nACGT\n+\nIIII\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn seq_name_and_only_id() {
        let input = write("seq_name.fq", b"@r1 a\nACGT\n+\nIIII\n");
        for (only_id, expected) in [(false, "r1 a\n"), (true, "r1\n")] {
            let out = temp(&format!("seq_name_{only_id}.out"));
            let _ = std::fs::remove_file(&out);
            let mut options = seq_options(&input, &out);
            options.name = true;
            options.only_id = only_id;
            seq(options).unwrap();
            assert_eq!(read(&out), expected);
            let _ = std::fs::remove_file(&out);
        }
        let _ = std::fs::remove_file(&input);
    }

    #[test]
    fn seq_only_prints_sequences() {
        let input = write("seq_s.fq", b"@r1\nACGT\n+\nIIII\n");
        let out = temp("seq_s.out");
        let _ = std::fs::remove_file(&out);
        let mut options = seq_options(&input, &out);
        options.seq_only = true;
        seq(options).unwrap();
        assert_eq!(read(&out), "ACGT\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn seq_reverse_complement() {
        let input = write("seq_rc.fq", b"@r1\nATGC\n+\nIIII\n");
        let out = temp("seq_rc.out");
        let _ = std::fs::remove_file(&out);
        let mut options = seq_options(&input, &out);
        options.transforms.reverse = true;
        options.transforms.complement = true;
        seq(options).unwrap();
        assert_eq!(read(&out), "@r1\nGCAT\n+\nIIII\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn seq_length_filter_is_inclusive() {
        let input = write(
            "seq_len.fq",
            b"@short\nACGT\n+\nIIII\n@long\nACGTACGT\n+\nIIIIIIII\n",
        );
        let out = temp("seq_len.out");
        let _ = std::fs::remove_file(&out);
        let mut options = seq_options(&input, &out);
        options.min_len = Some(8);
        seq(options).unwrap();
        assert_eq!(read(&out), "@long\nACGTACGT\n+\nIIIIIIII\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn seq_removes_gaps_and_uppercases() {
        let input = write("seq_gap.fq", b"@r1\nac-gt\n+\nIIIII\n");
        let out = temp("seq_gap.out");
        let _ = std::fs::remove_file(&out);
        let mut options = seq_options(&input, &out);
        options.transforms.remove_gaps = true;
        options.transforms.upper = true;
        // Removing a gap shortens a FASTQ record, so only the sequence is printed;
        // this is the documented behaviour of a length-changing transform.
        options.seq_only = true;
        seq(options).unwrap();
        assert_eq!(read(&out), "ACGT\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn seq_rejects_contradictory_output_selections() {
        let input = write("seq_bad.fq", b"@r1\nACGT\n+\nIIII\n");
        let out = temp("seq_bad.out");
        let mut options = seq_options(&input, &out);
        options.name = true;
        options.seq_only = true;
        let error = seq(options).expect_err("must reject");
        assert_eq!(error.exit_code(), 2);
        let _ = std::fs::remove_file(&input);
    }

    #[test]
    fn seq_rejects_both_case_flags() {
        let input = write("seq_case.fq", b"@r1\nACGT\n+\nIIII\n");
        let out = temp("seq_case.out");
        let mut options = seq_options(&input, &out);
        options.transforms.upper = true;
        options.transforms.lower = true;
        let error = seq(options).expect_err("must reject");
        assert_eq!(error.exit_code(), 2);
        let _ = std::fs::remove_file(&input);
    }

    // ---- fq2fa -----------------------------------------------------------

    #[test]
    fn fq2fa_converts_and_is_idempotent_on_fasta() {
        let input = write("conv.fq", b"@r1 a\nACGT\n+\nIIII\n");
        let out = temp("conv.fa");
        let _ = std::fs::remove_file(&out);
        fq2fa(Fq2faOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            line_width: 60,
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        assert_eq!(read(&out), ">r1 a\nACGT\n");

        // Feeding the FASTA back through must be a no-op.
        let out2 = temp("conv2.fa");
        let _ = std::fs::remove_file(&out2);
        fq2fa(Fq2faOptions {
            inputs: vec![out.to_string_lossy().into_owned()],
            line_width: 60,
            out_file: Some(out2.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        assert_eq!(read(&out2), read(&out));
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&out2);
    }

    // ---- fx2tab ----------------------------------------------------------

    fn fx2tab_options(input: &Path, out: &Path) -> Fx2tabOptions {
        Fx2tabOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            header_line: false,
            length: false,
            gc: false,
            gc_skew: false,
            name: false,
            only_id: false,
            no_qual: false,
            avg_qual: false,
            seq_hash: false,
            alphabet: false,
            case_sensitive: false,
            base_content: Vec::new(),
            base_count: Vec::new(),
            basename: false,
            file_name: false,
            quality_base: 33,
            stdin_label: "-".to_string(),
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        }
    }

    // The documented rule: three fixed columns for either format.
    #[test]
    fn fx2tab_emits_three_columns_by_default() {
        let fastq = write("tab.fq", b"@r1 a\nACGT\n+\nIIII\n");
        let out = temp("tab.out");
        let _ = std::fs::remove_file(&out);
        fx2tab(fx2tab_options(&fastq, &out)).unwrap();
        assert_eq!(read(&out), "r1 a\tACGT\tIIII\n");

        // A FASTA input still prints the (empty) quality column.
        let fasta = write("tab.fa", b">r1 a\nACGT\n");
        let out2 = temp("tab2.out");
        let _ = std::fs::remove_file(&out2);
        fx2tab(fx2tab_options(&fasta, &out2)).unwrap();
        assert_eq!(read(&out2), "r1 a\tACGT\t\n");
        let _ = std::fs::remove_file(&fastq);
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&fasta);
        let _ = std::fs::remove_file(&out2);
    }

    #[test]
    fn fx2tab_length_and_gc() {
        let input = write("tab_lg.fa", b">r1\nGCAT\n");
        let out = temp("tab_lg.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.length = true;
        options.gc = true;
        fx2tab(options).unwrap();
        // GC now prints two decimals, as SeqKit does.
        assert_eq!(read(&out), "r1\tGCAT\t\t4\t50.00\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn fx2tab_name_only_drops_the_sequence_columns() {
        let input = write("tab_n.fa", b">r1\nGCAT\n");
        let out = temp("tab_n.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.name = true;
        options.length = true;
        fx2tab(options).unwrap();
        assert_eq!(read(&out), "r1\t4\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn fx2tab_no_qual_drops_the_quality_column() {
        let input = write("tab_q.fq", b"@r1\nACGT\n+\nIIII\n");
        let out = temp("tab_q.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.no_qual = true;
        fx2tab(options).unwrap();
        assert_eq!(read(&out), "r1\tACGT\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn fx2tab_header_line_lists_the_selected_metrics() {
        let input = write("tab_h.fa", b">r1\nGCAT\n");
        let out = temp("tab_h.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.header_line = true;
        options.length = true;
        options.gc = true;
        fx2tab(options).unwrap();
        let text = read(&out);
        let header = text.lines().next().unwrap();
        // `-H` names the first column `#name`.
        assert_eq!(header, "#name\tseq\tqual\tlength\tGC");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn fx2tab_average_quality_is_blank_for_fasta() {
        let input = write("tab_aq.fa", b">r1\nACGT\n");
        let out = temp("tab_aq.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.avg_qual = true;
        options.header_line = true;
        fx2tab(options).unwrap();
        let text = read(&out);
        assert!(text.lines().next().unwrap().ends_with("avg.qual"));
        assert!(
            text.lines().nth(1).unwrap().ends_with('\t'),
            "blank final column"
        );
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn fx2tab_sequence_hash_is_md5_of_the_sequence() {
        let input = write("tab_s.fa", b">r1\nACGT\n");
        let out = temp("tab_s.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.seq_hash = true;
        fx2tab(options).unwrap();
        // MD5 of the LOWER-CASED sequence, which is what SeqKit hashes:
        //   $ seqkit fx2tab -s <(printf ">r1\nACGT\n")
        //   r1  ACGT    db516c3913e179338b162b2476d1c23f
        assert!(
            read(&out).contains("db516c3913e179338b162b2476d1c23f"),
            "seq hash must MD5 the lower-cased sequence: {}",
            read(&out)
        );
        assert!(!read(&out).contains("f1f8f4bf413b16ad135722aa4591043e"));
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn fx2tab_base_content_and_count() {
        let input = write("tab_b.fa", b">r1\nACGTACGT\n");
        let out = temp("tab_b.out");
        let _ = std::fs::remove_file(&out);
        let mut options = fx2tab_options(&input, &out);
        options.base_content = vec!["AT".to_string()];
        options.base_count = vec!["GC".to_string()];
        fx2tab(options).unwrap();
        // Counts come before contents, and content carries two decimals:
        //   -C GC gives 4, -B AT gives 50.00
        assert_eq!(read(&out), "r1\tACGTACGT\t\t4\t50.00\n");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    // ---- sample ----------------------------------------------------------

    fn sample_file(name: &str, records: usize) -> PathBuf {
        let mut text = String::new();
        for i in 0..records {
            text.push_str(&format!("@r{i}\nACGT\n+\nIIII\n"));
        }
        write(name, text.as_bytes())
    }

    #[test]
    fn sample_is_reproducible_for_a_fixed_seed() {
        let input = sample_file("sample.fq", 200);
        let run = |out: &Path| {
            let _ = std::fs::remove_file(out);
            sample(SampleOptions {
                inputs: vec![input.to_string_lossy().into_owned()],
                proportion: Some(0.25),
                number: None,
                rand_seed: 11,
                non_deterministic: false,
                line_width: 60,
                out_file: Some(out.to_string_lossy().into_owned()),
                compress_level: None,
            })
            .unwrap();
            read(out)
        };
        let first = run(&temp("sample1.out"));
        let second = run(&temp("sample2.out"));
        assert_eq!(
            first, second,
            "the same seed must reproduce the same output"
        );
        assert!(
            !first.is_empty(),
            "a quarter of 200 records should not be empty"
        );
        // A different seed must produce a different selection.
        let out3 = temp("sample3.out");
        let _ = std::fs::remove_file(&out3);
        sample(SampleOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            proportion: Some(0.25),
            number: None,
            rand_seed: 12,
            non_deterministic: false,
            line_width: 60,
            out_file: Some(out3.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        assert_ne!(
            read(&out3),
            first,
            "a different seed must change the selection"
        );
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(temp("sample1.out"));
        let _ = std::fs::remove_file(temp("sample2.out"));
        let _ = std::fs::remove_file(&out3);
    }

    #[test]
    fn sample_by_number_is_approximately_the_requested_count() {
        let input = sample_file("sample_n.fq", 1000);
        let out = temp("sample_n.out");
        let _ = std::fs::remove_file(&out);
        sample(SampleOptions {
            inputs: vec![input.to_string_lossy().into_owned()],
            proportion: None,
            number: Some(100),
            rand_seed: 7,
            non_deterministic: false,
            line_width: 60,
            out_file: Some(out.to_string_lossy().into_owned()),
            compress_level: None,
        })
        .unwrap();
        let count = read(&out).lines().count() / 4;
        assert!(
            (70..130).contains(&count),
            "expected about 100, got {count}"
        );
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn sample_rejects_both_or_neither_selection() {
        let input = sample_file("sample_bad.fq", 10);
        for (proportion, number) in [(Some(0.5), Some(5)), (None, None)] {
            let error = sample(SampleOptions {
                inputs: vec![input.to_string_lossy().into_owned()],
                proportion,
                number,
                rand_seed: 1,
                non_deterministic: false,
                line_width: 60,
                out_file: None,
                compress_level: None,
            })
            .expect_err("must reject");
            assert_eq!(error.exit_code(), 2);
        }
        let _ = std::fs::remove_file(&input);
    }

    // ---- split -----------------------------------------------------------

    #[test]
    fn split_by_size_writes_numbered_parts_in_order() {
        let input = sample_file("split.fq", 10);
        let dir = temp("split_dir");
        let _ = std::fs::remove_dir_all(&dir);
        split(SplitOptions {
            input: input.to_string_lossy().into_owned(),
            by: SplitBy::Size(4),
            out_dir: Some(dir.to_string_lossy().into_owned()),
            out_prefix: None,
            part_width: 3,
            extension: None,
            dry_run: false,
            force: false,
            compress_level: None,
        })
        .unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        // The part stem comes from the input's own file name, which in this test
        // is a unique temporary name.
        let stem = crate::split::part_stem(&input);
        assert_eq!(
            names,
            vec![
                format!("{stem}.part_001.fq"),
                format!("{stem}.part_002.fq"),
                format!("{stem}.part_003.fq"),
            ]
        );
        // 4 + 4 + 2, in input order.
        let part1 = std::fs::read_to_string(dir.join(format!("{stem}.part_001.fq"))).unwrap();
        assert_eq!(part1.lines().count() / 4, 4);
        assert!(part1.starts_with("@r0\n"));
        let part3 = std::fs::read_to_string(dir.join(format!("{stem}.part_003.fq"))).unwrap();
        assert_eq!(part3.lines().count() / 4, 2);
        assert!(part3.starts_with("@r8\n"), "the last part holds the tail");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_by_part_distributes_evenly() {
        let input = sample_file("splitp.fq", 10);
        let dir = temp("splitp_dir");
        let _ = std::fs::remove_dir_all(&dir);
        split(SplitOptions {
            input: input.to_string_lossy().into_owned(),
            by: SplitBy::Parts(3),
            out_dir: Some(dir.to_string_lossy().into_owned()),
            out_prefix: None,
            part_width: 3,
            extension: None,
            dry_run: false,
            force: false,
            compress_level: None,
        })
        .unwrap();
        let stem = crate::split::part_stem(&input);
        let counts: Vec<usize> = ["001", "002", "003"]
            .iter()
            .map(|n| {
                std::fs::read_to_string(dir.join(format!("{stem}.part_{n}.fq")))
                    .unwrap()
                    .lines()
                    .count()
                    / 4
            })
            .collect();
        assert_eq!(counts, vec![4, 3, 3]);
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_dry_run_creates_nothing() {
        let input = sample_file("splitd.fq", 5);
        let dir = temp("splitd_dir");
        let _ = std::fs::remove_dir_all(&dir);
        split(SplitOptions {
            input: input.to_string_lossy().into_owned(),
            by: SplitBy::Size(2),
            out_dir: Some(dir.to_string_lossy().into_owned()),
            out_prefix: None,
            part_width: 3,
            extension: None,
            dry_run: true,
            force: false,
            compress_level: None,
        })
        .unwrap();
        assert!(!dir.exists(), "a dry run must not create the directory");
        let _ = std::fs::remove_file(&input);
    }

    #[test]
    fn split_rejects_stdin() {
        let error = split(SplitOptions {
            input: "-".to_string(),
            by: SplitBy::Size(1),
            out_dir: None,
            out_prefix: None,
            part_width: 3,
            extension: None,
            dry_run: true,
            force: false,
            compress_level: None,
        })
        .expect_err("must reject stdin");
        assert_eq!(error.exit_code(), 2);
    }

    // ---- lint ------------------------------------------------------------

    fn lint_options(r1: &Path, r2: Option<&Path>) -> LintOptions {
        LintOptions {
            r1: r1.to_string_lossy().into_owned(),
            r2: r2.map(|p| p.to_string_lossy().into_owned()),
            log_mode: true,
            single_level: Level::High,
            paired_level: Level::High,
            disable: Vec::new(),
        }
    }

    #[test]
    fn lint_accepts_a_valid_pair() {
        let r1 = write("l_a.fq", b"@r1\nACGT\n+\nIIII\n@r2\nTTTT\n+\nIIII\n");
        let r2 = write("l_b.fq", b"@r1\nGGGG\n+\nIIII\n@r2\nCCCC\n+\nIIII\n");
        lint(lint_options(&r1, Some(&r2))).unwrap();
        let _ = std::fs::remove_file(&r1);
        let _ = std::fs::remove_file(&r2);
    }

    #[test]
    fn lint_rejects_a_length_mismatch_with_exit_1() {
        // fq reports a validation failure as exit 1, not the 3 reserved for a
        // violated input precondition. `fq lint bad.fq; echo $?` prints 1.
        let r1 = write("l_bad.fq", b"@r1\nACGTACGT\n+\nIII\n");
        let error = lint(lint_options(&r1, None)).expect_err("must reject");
        assert_eq!(error.exit_code(), 1);
        assert!(error.message.contains("S005"), "{}", error.message);
        let _ = std::fs::remove_file(&r1);
    }

    #[test]
    fn lint_accepts_a_duplicate_name() {
        // fq 0.12.0 never enables S007 for single reads, so duplicate names pass.
        let r1 = write("l_dup.fq", b"@r1\nACGT\n+\nIIII\n@r1\nACGT\n+\nIIII\n");
        assert!(
            lint(lint_options(&r1, None)).is_ok(),
            "duplicate names must pass, as fq does"
        );
        let _ = std::fs::remove_file(&r1);
    }

    #[test]
    fn lint_rejects_mismatched_pair_names() {
        let r1 = write("l_p1.fq", b"@r1\nACGT\n+\nIIII\n");
        let r2 = write("l_p2.fq", b"@other\nACGT\n+\nIIII\n");
        let error = lint(lint_options(&r1, Some(&r2))).expect_err("must reject");
        assert!(error.message.contains("P001"), "{}", error.message);
        let _ = std::fs::remove_file(&r1);
        let _ = std::fs::remove_file(&r2);
    }

    #[test]
    fn lint_rejects_an_unknown_validator_code() {
        let r1 = write("l_code.fq", b"@r1\nACGT\n+\nIIII\n");
        let mut options = lint_options(&r1, None);
        options.disable = vec!["S999".to_string()];
        let error = lint(options).expect_err("must reject");
        assert_eq!(error.exit_code(), 2);
        assert!(error.message.contains("S999"));
        let _ = std::fs::remove_file(&r1);
    }

    #[test]
    fn lint_can_disable_a_failing_validator() {
        let r1 = write("l_dis.fq", b"@r1\nACGTACGT\n+\nIII\n");
        let mut options = lint_options(&r1, None);
        options.disable = vec!["S005".to_string()];
        lint(options).expect("S005 is disabled, so this must pass");
        let _ = std::fs::remove_file(&r1);
    }

    #[test]
    fn lint_rejects_fasta_input() {
        let fasta = write("l.fa", b">r1\nACGT\n");
        let error = lint(lint_options(&fasta, None)).expect_err("must reject");
        assert_eq!(error.exit_code(), 3);
        let _ = std::fs::remove_file(&fasta);
    }
}
