//! SeqKit sort helpers: natural ordering and bounded two-pass FASTA sorting.

use crate::commands::SortOptions;
use crate::io::{CommandError, Output, OwnedRecord};
use crate::transform;
use bioinfo_fastx::writer::Layout;
use std::cmp::Ordering;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Compare digit runs numerically and other bytes lexicographically.
pub fn natural_cmp(left: &[u8], right: &[u8], ignore_case: bool) -> Ordering {
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        let left_digit = left[left_index].is_ascii_digit();
        let right_digit = right[right_index].is_ascii_digit();
        if left_digit && right_digit {
            let left_end = digit_end(left, left_index);
            let right_end = digit_end(right, right_index);
            let left_number = trim_zeroes(&left[left_index..left_end]);
            let right_number = trim_zeroes(&right[right_index..right_end]);
            let ordering = left_number
                .len()
                .cmp(&right_number.len())
                .then_with(|| left_number.cmp(right_number));
            if ordering != Ordering::Equal {
                return ordering;
            }
            // Equal numeric values fall back to the original byte order, so
            // r02 sorts before r2, as in SeqKit.
            left_index = left_end;
            right_index = right_end;
        } else {
            let left_byte = fold(left[left_index], ignore_case);
            let right_byte = fold(right[right_index], ignore_case);
            let ordering = left_byte.cmp(&right_byte);
            if ordering != Ordering::Equal {
                return ordering;
            }
            left_index += 1;
            right_index += 1;
        }
    }
    (left.len() - left_index)
        .cmp(&(right.len() - right_index))
        .then_with(|| left.cmp(right))
}

fn digit_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .map(|offset| start + offset)
        .unwrap_or(bytes.len())
}

fn trim_zeroes(bytes: &[u8]) -> &[u8] {
    let first = bytes
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(bytes.len().saturating_sub(1));
    &bytes[first..]
}

fn fold(byte: u8, ignore_case: bool) -> u8 {
    if ignore_case {
        byte.to_ascii_lowercase()
    } else {
        byte
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IndexEntry {
    pub(crate) name: Vec<u8>,
    pub(crate) id: Vec<u8>,
    length: usize,
    non_gap: usize,
    prefix: Vec<u8>,
    offset: u64,
    line_bases: usize,
    line_width: usize,
    ordinal: usize,
    input_index: usize,
}

pub(crate) fn fai_path(input: &Path) -> PathBuf {
    PathBuf::from(format!("{}.seqkit.fai", input.display()))
}

fn trim_line(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\n")
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .unwrap_or(bytes)
}

pub(crate) fn read_record_at(file: &Path, entry: &IndexEntry) -> Result<OwnedRecord, CommandError> {
    let mut reader = BufReader::new(
        File::open(file)
            .map_err(|error| CommandError::io(format!("{}: {error}", file.display())))?,
    );
    reader
        .seek(SeekFrom::Start(entry.offset))
        .map_err(|error| CommandError::io(format!("{}: {error}", file.display())))?;
    let mut header = Vec::new();
    reader
        .read_until(b'\n', &mut header)
        .map_err(|error| CommandError::io(format!("{}: {error}", file.display())))?;
    let header = trim_line(&header).to_vec();
    if !header.starts_with(b">") {
        return Err(CommandError::input(format!(
            "{}: invalid FASTA header at byte {}",
            file.display(),
            entry.offset
        )));
    }
    let mut sequence = Vec::new();
    loop {
        let mut line = Vec::new();
        let count = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| CommandError::io(format!("{}: {error}", file.display())))?;
        if count == 0 || line.starts_with(b">") {
            break;
        }
        sequence.extend_from_slice(trim_line(&line));
    }
    Ok(OwnedRecord {
        name: header[1..].to_vec(),
        raw_header: header,
        sequence,
        plus: Vec::new(),
        qualities: Vec::new(),
        has_quality_line: false,
        index: entry.ordinal as u64,
    })
}

pub(crate) fn scan_fasta(
    input: &Path,
    prefix_length: usize,
    gap_letters: &[u8],
) -> Result<Vec<IndexEntry>, CommandError> {
    let prefix_length = if prefix_length == 0 {
        usize::MAX
    } else {
        prefix_length
    };
    let file = File::open(input)
        .map_err(|error| CommandError::io(format!("{}: {error}", input.display())))?;
    let mut reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut offset = 0u64;
    let mut current: Option<IndexEntry> = None;
    loop {
        let mut line = Vec::new();
        let count = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| CommandError::io(format!("{}: {error}", input.display())))?;
        if count == 0 {
            break;
        }
        let line_start = offset;
        offset += count as u64;
        let content = trim_line(&line);
        if content.starts_with(b">") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            let name = content[1..].to_vec();
            current = Some(IndexEntry {
                id: transform::sequence_id(&name).to_vec(),
                name,
                length: 0,
                non_gap: 0,
                prefix: Vec::new(),
                offset: line_start,
                line_bases: 0,
                line_width: 0,
                ordinal: entries.len(),
                input_index: 0,
            });
        } else {
            let Some(entry) = current.as_mut() else {
                if content.is_empty() {
                    continue;
                }
                return Err(CommandError::input(format!(
                    "{}: sequence data before the first FASTA header",
                    input.display()
                )));
            };
            if content.is_empty() {
                continue;
            }
            if entry.line_bases == 0 {
                entry.line_bases = content.len();
                entry.line_width = line.len();
            }
            entry.length += content.len();
            entry.non_gap += content
                .iter()
                .filter(|base| !gap_letters.contains(base))
                .count();
            if entry.prefix.len() < prefix_length {
                let remaining = prefix_length - entry.prefix.len();
                entry
                    .prefix
                    .extend_from_slice(&content[..remaining.min(content.len())]);
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    Ok(entries)
}

pub(crate) fn write_fai(input: &Path, entries: &[IndexEntry]) -> Result<(), CommandError> {
    write_fai_at(&fai_path(input), entries)
}

pub(crate) fn write_fai_at(path: &Path, entries: &[IndexEntry]) -> Result<(), CommandError> {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            String::from_utf8_lossy(&entry.name),
            entry.length,
            entry.offset + entry.raw_header_len() as u64,
            entry.line_bases,
            entry.line_width
        ));
    }
    std::fs::write(path, text)
        .map_err(|error| CommandError::io(format!("{}: {error}", path.display())))
}

impl IndexEntry {
    fn raw_header_len(&self) -> usize {
        self.name.len() + 2
    }
}

pub(crate) fn load_fai(
    input: &Path,
    prefix_length: usize,
    gap_letters: &[u8],
) -> Result<Vec<IndexEntry>, CommandError> {
    load_fai_at(&fai_path(input), input, prefix_length, gap_letters)
}

pub(crate) fn load_fai_at(
    path: &Path,
    input: &Path,
    prefix_length: usize,
    gap_letters: &[u8],
) -> Result<Vec<IndexEntry>, CommandError> {
    let prefix_length = if prefix_length == 0 {
        usize::MAX
    } else {
        prefix_length
    };
    let text = std::fs::read_to_string(path)
        .map_err(|error| CommandError::io(format!("{}: {error}", path.display())))?;
    let mut entries = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 5 {
            return Err(CommandError::input(format!(
                "{}:{}: invalid FASTA index",
                path.display(),
                line_number + 1
            )));
        }
        let length = fields[1].parse::<usize>().map_err(|_| {
            CommandError::input(format!(
                "{}:{}: invalid length",
                path.display(),
                line_number + 1
            ))
        })?;
        let offset = fields[2].parse::<u64>().map_err(|_| {
            CommandError::input(format!(
                "{}:{}: invalid offset",
                path.display(),
                line_number + 1
            ))
        })?;
        let line_bases = fields[3].parse::<usize>().map_err(|_| {
            CommandError::input(format!(
                "{}:{}: invalid line bases",
                path.display(),
                line_number + 1
            ))
        })?;
        let line_width = fields[4].parse::<usize>().map_err(|_| {
            CommandError::input(format!(
                "{}:{}: invalid line width",
                path.display(),
                line_number + 1
            ))
        })?;
        let name = fields[0].as_bytes().to_vec();
        let offset = offset.saturating_sub(name.len() as u64 + 2);
        let mut entry = IndexEntry {
            id: transform::sequence_id(&name).to_vec(),
            name,
            length,
            non_gap: 0,
            prefix: Vec::new(),
            offset,
            line_bases,
            line_width,
            ordinal: entries.len(),
            input_index: 0,
        };
        let record = read_record_at(input, &entry)?;
        entry.non_gap = record
            .sequence
            .iter()
            .filter(|base| !gap_letters.contains(base))
            .count();
        entry.prefix = record.sequence[..record.sequence.len().min(prefix_length)].to_vec();
        entries.push(entry);
    }
    Ok(entries)
}

pub(crate) fn load_or_build_fai(
    input: &Path,
    index_path: &Path,
    update: bool,
    prefix_length: usize,
    gap_letters: &[u8],
) -> Result<Vec<IndexEntry>, CommandError> {
    if index_path.exists() && !update {
        load_fai_at(index_path, input, prefix_length, gap_letters)
    } else {
        let entries = scan_fasta(input, prefix_length, gap_letters)?;
        write_fai_at(index_path, &entries)?;
        Ok(entries)
    }
}

/// Sort plain FASTA records using only metadata and byte offsets in memory.
pub fn two_pass(options: &SortOptions) -> Result<(), CommandError> {
    if options.by_bases {
        return Err(
            CommandError::usage("-b/--by-bases is incompatible with -2/--two-pass").with_code(255),
        );
    }
    if options.inputs.len() != 1 || options.inputs.iter().any(|input| input == "-") {
        return Err(CommandError::usage("no more than one file should be given").with_code(255));
    }
    let mut entries = Vec::new();
    for (input_index, input) in options.inputs.iter().enumerate() {
        let path = Path::new(input);
        if is_compressed(path) {
            return Err(CommandError::usage(format!(
                "sort --two-pass currently supports plain FASTA only: {input}"
            )));
        }
        let index = fai_path(path);
        let mut input_entries = if index.exists() && !options.update_faidx {
            load_fai(path, options.seq_prefix_length, &options.gap_letters)?
        } else {
            let entries = scan_fasta(path, options.seq_prefix_length, &options.gap_letters)?;
            write_fai(path, &entries)?;
            entries
        };
        let base = entries.len();
        for entry in &mut input_entries {
            entry.ordinal += base;
            entry.input_index = input_index;
        }
        entries.extend(input_entries);
    }
    let mut comparison_error = None;
    entries.sort_by(|left, right| {
        let ordering = if options.by_length {
            left.length.cmp(&right.length)
        } else if options.by_bases {
            left.non_gap.cmp(&right.non_gap)
        } else if options.by_seq {
            let left_seq = fold_bytes(&left.prefix, options.ignore_case);
            let right_seq = fold_bytes(&right.prefix, options.ignore_case);
            let ordering = left_seq.cmp(&right_seq);
            if ordering == Ordering::Equal && options.seq_prefix_length > 0 {
                let left_path = Path::new(&options.inputs[left.input_index]);
                let right_path = Path::new(&options.inputs[right.input_index]);
                match (
                    read_record_at(left_path, left),
                    read_record_at(right_path, right),
                ) {
                    (Ok(left_record), Ok(right_record)) => {
                        fold_bytes(&left_record.sequence, options.ignore_case)
                            .cmp(&fold_bytes(&right_record.sequence, options.ignore_case))
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        comparison_error = Some(error);
                        Ordering::Equal
                    }
                }
            } else {
                ordering
            }
        } else if options.by_name {
            if options.natural_order {
                natural_cmp(&left.name, &right.name, options.ignore_case)
            } else {
                fold_bytes(&left.name, options.ignore_case)
                    .cmp(&fold_bytes(&right.name, options.ignore_case))
            }
        } else if options.natural_order {
            natural_cmp(&left.id, &right.id, options.ignore_case)
        } else {
            fold_bytes(&left.id, options.ignore_case)
                .cmp(&fold_bytes(&right.id, options.ignore_case))
        };
        if options.reverse {
            ordering.reverse()
        } else {
            ordering
        }
    });

    if let Some(error) = comparison_error {
        return Err(error);
    }

    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    for entry in entries {
        let input = &options.inputs[entry.input_index()];
        let record = read_record_at(Path::new(input), &entry)?;
        record.write_to(&mut out, Layout::Fasta, options.line_width)?;
    }
    out.finish()
}

impl IndexEntry {
    fn input_index(&self) -> usize {
        self.input_index
    }
}

fn is_compressed(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("gz" | "bz2" | "xz" | "zst")
    )
}

fn fold_bytes(bytes: &[u8], ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        bytes.to_ascii_lowercase()
    } else {
        bytes.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numeric_runs_without_losing_leading_zero_ties() {
        assert_eq!(natural_cmp(b"r2", b"r10", false), Ordering::Less);
        assert_eq!(natural_cmp(b"r02", b"r2", false), Ordering::Less);
        assert_eq!(natural_cmp(b"r1", b"r01", false), Ordering::Greater);
    }
}
