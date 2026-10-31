//! SeqKit record ranges, processed independently for each input.
//!
//! Positive ranges (including START:-1) retain one record. Negative ranges
//! retain at most |START| records, not the entire input. The short-input tail
//! rule follows SeqKit 2.14's circular record buffer rather than clamping END.

use crate::commands::RangeOptions;
use crate::io::{CommandError, InputFormat, OwnedRecord, RecordReader};
use crate::seqkit_io::{check_paths, SeqkitOutput};
use std::collections::VecDeque;
use std::io::Write;

struct IndexRange {
    start: i64,
    end: i64,
}

impl IndexRange {
    fn parse(text: &str) -> Result<Self, CommandError> {
        let error = |message: String| CommandError::usage(message).with_code(255);
        if text.is_empty() {
            return Err(error("flag -r (--range) needed".into()));
        }
        // The upstream regexp is deliberately unanchored; Atoi then validates
        // the first two colon-separated fields (for example +1:2 and 1:2:3).
        if !regex::Regex::new(r"-?[0-9]+:-?[0-9]+")
            .expect("constant range regexp")
            .is_match(text)
        {
            return Err(error(format!(
                "invalid range: {text}. type \"seqkit range -h\" for more examples"
            )));
        }
        let mut fields = text.split(':');
        let parse = |value: &str| {
            value.parse::<i64>().map_err(|failure| {
                let reason = match failure.kind() {
                    std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                        "value out of range"
                    }
                    _ => "invalid syntax",
                };
                error(format!("strconv.Atoi: parsing {value:?}: {reason}"))
            })
        };
        let start = parse(fields.next().unwrap_or_default())?;
        let end = parse(fields.next().unwrap_or_default())?;
        if start == 0 || end == 0 {
            return Err(error("either start and end should not be 0".into()));
        }
        if start < 0 && end > 0 {
            return Err(error("when start < 0, end should not > 0".into()));
        }
        if start < 0 && end < 0 && start > end {
            return Err(error(
                "when start < 0 and end < 0, start should be < end".into(),
            ));
        }
        if start > 0 && end < 0 && end != -1 {
            return Err(error(format!(
                "not supported range: {start}:{end}, the end needs to be -1 when start > 0 and end < 0"
            )));
        }
        Ok(Self { start, end })
    }
}

pub(crate) fn run(options: RangeOptions) -> Result<(), CommandError> {
    run_inner(options).map_err(|error| error.with_code(255))
}

fn run_inner(options: RangeOptions) -> Result<(), CommandError> {
    let region = IndexRange::parse(&options.region)?;
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs
    };
    check_paths(&inputs, options.out_file.as_deref())?;
    let mut out = SeqkitOutput::create(options.out_file.as_deref(), options.compress_level)?;
    for input in &inputs {
        let mut reader = RecordReader::seqkit(input)?;
        let fastq = reader.format() == InputFormat::Fastq;
        let width = if fastq && !options.line_width_explicit {
            0
        } else {
            options.line_width
        };
        if region.start < 0 {
            let keep = region.start.unsigned_abs();
            if keep > i64::MAX as u64 {
                return Err(CommandError::usage(
                    "RecordLoopBuffer: capacity should be > 0",
                ));
            }
            // Grow with actual input, rather than allocating |START| slots
            // before reading even a single record.
            let mut tail: VecDeque<OwnedRecord> = VecDeque::new();
            while let Some(record) = reader.next_record() {
                let record = record?;
                if tail.len() as u64 == keep {
                    tail.pop_front();
                }
                tail.push_back(record);
            }
            if !tail.is_empty() {
                // SeqKit walks backward around its ring from the newest record.
                // END before the beginning wraps; it does not clamp to zero.
                let back = (region.end.unsigned_abs() - 1) % tail.len() as u64;
                let count = tail.len() - back as usize;
                for record in tail.into_iter().take(count) {
                    write_record(&mut out, &record, fastq, width)?;
                }
            }
        } else {
            let mut number = 0u64;
            while let Some(record) = reader.next_record() {
                let record = record?;
                number += 1;
                if number < region.start as u64 {
                    continue;
                }
                if region.end > 0 && number > region.end as u64 {
                    break;
                }
                write_record(&mut out, &record, fastq, width)?;
            }
        }
    }
    out.finish()
}

fn write_record(
    out: &mut SeqkitOutput,
    record: &OwnedRecord,
    fastq: bool,
    width: usize,
) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    bytes.push(if fastq { b'@' } else { b'>' });
    bytes.extend_from_slice(&record.name);
    bytes.push(b'\n');
    wrap(&mut bytes, &record.sequence, width);
    bytes.push(b'\n');
    if fastq {
        bytes.extend_from_slice(b"+\n");
        wrap(&mut bytes, &record.qualities, width);
        bytes.push(b'\n');
    }
    out.write_all(&bytes)
}

fn wrap(out: &mut Vec<u8>, sequence: &[u8], width: usize) {
    if width == 0 {
        out.extend_from_slice(sequence);
    } else {
        for (index, chunk) in sequence.chunks(width).enumerate() {
            if index != 0 {
                out.push(b'\n');
            }
            out.extend_from_slice(chunk);
        }
    }
}
