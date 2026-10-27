//! `extract`: move UMI bases from a FASTQ read into the read name.
//!
//! Implemented subset: `--extract-method=string` with a barcode pattern where
//! `N` marks a UMI base, `--extract-method=regex` with named `umi_*` and
//! `discard_*` groups, single or paired input, 5' or 3' barcodes, gzip by file
//! extension, quality filtering and masking. Cell-barcode correction is
//! rejected before any output is created.

use crate::cli::ExtractArgs;
use crate::commands::{Logger, PROGRAM};
use crate::error::{Error, Result};
use crate::io::fastq::{FastqReader, FastqRecord, FastqWriter};
use crate::io::spill::TempOutput;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Output {
    writer: FastqWriter,
    temp: Option<(TempOutput, PathBuf)>,
}

impl Output {
    fn create(path: Option<&Path>) -> Result<Output> {
        let gzip = path
            .and_then(|path| path.extension())
            .and_then(|value| value.to_str())
            == Some("gz");
        match path {
            None => Ok(Output {
                writer: FastqWriter::create(None, false)?,
                temp: None,
            }),
            Some(path) if path.as_os_str() == "-" => Ok(Output {
                writer: FastqWriter::create(None, false)?,
                temp: None,
            }),
            Some(path) => {
                let directory = path
                    .parent()
                    .filter(|value| !value.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let temp = TempOutput::create(directory, "umitools-extract")?;
                Ok(Output {
                    writer: FastqWriter::create(Some(temp.path()), gzip)?,
                    temp: Some((temp, path.to_path_buf())),
                })
            }
        }
    }

    fn write(&mut self, record: &FastqRecord) -> Result<()> {
        self.writer.write_record(record)
    }

    fn finish(self) -> Result<()> {
        self.writer.finish()?;
        if let Some((temp, destination)) = self.temp {
            temp.commit(&destination)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Stats {
    reads_in: u64,
    reads_out: u64,
    discarded_quality: u64,
    discarded_mismatch: u64,
    too_short: u64,
}

#[derive(Clone, Copy)]
struct QualityOptions {
    offset: u8,
    threshold: Option<u8>,
    mask: Option<u8>,
}

/// Extract the UMI from one read and rewrite its name. Returns the UMI, or
/// `None` when the read is discarded.
fn extract_umi(
    record: &mut FastqRecord,
    pattern: &str,
    three_prime: bool,
    separator: &str,
    quality_options: QualityOptions,
    stats: &mut Stats,
) -> Option<String> {
    let length = pattern.len();
    if record.seq.len() < length {
        stats.too_short += 1;
        return None;
    }
    let start = if three_prime {
        record.seq.len() - length
    } else {
        0
    };
    let mut umi = Vec::with_capacity(length);
    for (offset, expected) in pattern.bytes().enumerate() {
        let base = record.seq[start + offset];
        let quality = record.qual[start + offset].saturating_sub(quality_options.offset);
        if expected == b'N' || expected == b'n' {
            if let Some(minimum) = quality_options.threshold {
                if quality < minimum {
                    stats.discarded_quality += 1;
                    return None;
                }
            }
            let masked = quality_options
                .mask
                .is_some_and(|minimum| quality < minimum);
            umi.push(if masked { b'N' } else { base });
        } else if !base.eq_ignore_ascii_case(&expected) {
            stats.discarded_mismatch += 1;
            return None;
        }
    }
    record.seq.drain(start..start + length);
    record.qual.drain(start..start + length);
    let umi = String::from_utf8_lossy(&umi).into_owned();
    record.name = format!("{}{}{}", record.name, separator, umi);
    Some(umi)
}

/// Regex extraction: named groups starting with `umi_` form the UMI and their
/// spans, together with `discard_` spans, are removed from the sequence.
fn extract_regex(
    record: &mut FastqRecord,
    pattern: &Regex,
    separator: &str,
    stats: &mut Stats,
) -> Option<String> {
    let sequence = String::from_utf8_lossy(&record.seq).into_owned();
    let Some(captures) = pattern.captures(&sequence) else {
        stats.discarded_mismatch += 1;
        return None;
    };
    let mut umi = String::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for name in pattern.capture_names().flatten() {
        if let Some(found) = captures.name(name) {
            if name.starts_with("umi_") {
                umi.push_str(found.as_str());
                spans.push((found.start(), found.end()));
            } else if name.starts_with("discard_") {
                spans.push((found.start(), found.end()));
            }
        }
    }
    if umi.is_empty() {
        stats.discarded_mismatch += 1;
        return None;
    }
    spans.sort_unstable_by_key(|span| std::cmp::Reverse(span.0));
    for (start, end) in spans {
        record.seq.drain(start..end);
        record.qual.drain(start..end);
    }
    record.name = format!("{}{}{}", record.name, separator, umi);
    Some(umi)
}

/// Trim a read2 region without contributing its bases to the UMI.
fn trim_pattern(record: &mut FastqRecord, pattern: &str, three_prime: bool) -> bool {
    let length = pattern.len();
    if record.seq.len() < length {
        return false;
    }
    let start = if three_prime {
        record.seq.len() - length
    } else {
        0
    };
    for (offset, expected) in pattern.bytes().enumerate() {
        if expected == b'N' || expected == b'n' {
            continue;
        }
        if !record.seq[start + offset].eq_ignore_ascii_case(&expected) {
            return false;
        }
    }
    record.seq.drain(start..start + length);
    record.qual.drain(start..start + length);
    true
}

pub fn run(args: &ExtractArgs) -> Result<()> {
    let started = Instant::now();
    let mut log = Logger::new(&args.log, args.quiet)?;
    let quality_offset = match args.quality_encoding.as_str() {
        "phred33" => 33,
        "phred64" => 64,
        "solexa" => 59,
        other => {
            return Err(Error::usage_with_hint(
                format!("unknown --quality-encoding '{other}'"),
                "choose one of: phred33, phred64, solexa",
            ));
        }
    };
    let quality_options = QualityOptions {
        offset: quality_offset,
        threshold: args.quality_filter_threshold,
        mask: args.quality_filter_mask,
    };
    let method = args.extract_method.as_str();
    let regex_pattern = match method {
        "string" => None,
        "regex" => Some({
            let pattern = args.bc_pattern.as_deref().ok_or_else(|| {
                Error::usage_with_hint(
                    "regex extraction needs -p with named groups",
                    "example: -p '(?P<umi_1>.{4})'",
                )
            })?;
            let compiled = Regex::new(pattern)
                .map_err(|e| Error::usage(format!("--bc-pattern is not a valid regex: {e}")))?;
            if !compiled
                .capture_names()
                .flatten()
                .any(|name| name.starts_with("umi_"))
            {
                return Err(Error::usage_with_hint(
                    "regex extraction needs at least one (?P<umi_...>) group",
                    "example: -p '(?P<umi_1>.{4})(?P<discard_1>ACGT)'",
                ));
            }
            compiled
        }),
        other => {
            return Err(Error::usage_with_hint(
                format!("unknown --extract-method '{other}'"),
                "choose one of: string, regex",
            ))
        }
    };
    let pattern = args.bc_pattern.as_deref().ok_or_else(|| {
        Error::usage_with_hint("extract needs -p/--bc-pattern", "example: -p NNNN")
    })?;
    if pattern.is_empty() {
        return Err(Error::usage("--bc-pattern must not be empty"));
    }
    let paired = args.read2_in.is_some() || args.read2_out.is_some();
    if paired && (args.read2_in.is_none() || args.read2_out.is_none()) {
        return Err(Error::usage_with_hint(
            "paired extraction needs both --read2-in and --read2-out",
            "or omit both for single-end input",
        ));
    }
    let mut reader1 = FastqReader::open(args.input.as_deref())?;
    let mut reader2 = match &args.read2_in {
        Some(path) => Some(FastqReader::open(Some(path))?),
        None => None,
    };
    let mut out1 = Output::create(args.output.as_deref())?;
    let mut out2 = match &args.read2_out {
        Some(path) => Some(Output::create(Some(path))?),
        None => None,
    };
    let mut stats = Stats::default();

    while let Some(mut first) = reader1.next_record()? {
        stats.reads_in += 1;
        if let Some(reader) = reader2.as_mut() {
            let Some(mut second) = reader.next_record()? else {
                return Err(Error::Format(
                    "read2 input ended before read1; read pairs do not match".to_string(),
                ));
            };
            if first.name != second.name {
                return Err(Error::Format(format!(
                    "read pairs do not match: {} != {}",
                    first.name, second.name
                )));
            }
            let umi = match &regex_pattern {
                Some(regex) => extract_regex(&mut first, regex, &args.umi_separator, &mut stats),
                None => extract_umi(
                    &mut first,
                    pattern,
                    args.three_prime,
                    &args.umi_separator,
                    quality_options,
                    &mut stats,
                ),
            };
            let Some(umi) = umi else {
                continue;
            };
            if let Some(pattern2) = &args.bc_pattern2 {
                if !trim_pattern(&mut second, pattern2, args.three_prime) {
                    stats.discarded_mismatch += 1;
                    continue;
                }
            }
            second.name = format!("{}{}{}", second.name, args.umi_separator, umi);
            out1.write(&first)?;
            out2.as_mut()
                .expect("paired output validated above")
                .write(&second)?;
            stats.reads_out += 1;
        } else {
            let umi = match &regex_pattern {
                Some(regex) => extract_regex(&mut first, regex, &args.umi_separator, &mut stats),
                None => extract_umi(
                    &mut first,
                    pattern,
                    args.three_prime,
                    &args.umi_separator,
                    quality_options,
                    &mut stats,
                ),
            };
            if umi.is_none() {
                continue;
            }
            out1.write(&first)?;
            stats.reads_out += 1;
        }
    }
    if let Some(reader) = reader2.as_mut() {
        if reader.next_record()?.is_some() {
            return Err(Error::Format(
                "read1 input ended before read2; read pairs do not match".to_string(),
            ));
        }
    }
    out1.finish()?;
    if let Some(out2) = out2 {
        out2.finish()?;
    }
    log.info(&format!(
        "{PROGRAM} extract: reads in={} reads out={} discarded_quality={} \
         discarded_mismatch={} too_short={} wall={:.1}s",
        stats.reads_in,
        stats.reads_out,
        stats.discarded_quality,
        stats.discarded_mismatch,
        stats.too_short,
        started.elapsed().as_secs_f64()
    ));
    Ok(())
}
