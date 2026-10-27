//! SeqKit `grep`: ID/name/sequence matching with motif and output controls.

use crate::commands::Region;
use crate::io::{CommandError, InputFormat, Output, OwnedRecord, RecordReader};
use crate::transform;
use bioinfo_fastx::writer::Layout;
use regex::{Regex, RegexBuilder};
use std::io::Write;

/// The parsed `grep` options.
pub struct GrepOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-p`: patterns supplied on the command line.
    pub patterns: Vec<String>,
    /// `-f`: one pattern per line.
    pub pattern_file: Option<String>,
    /// `-n`: match the full name rather than the ID.
    pub by_name: bool,
    /// `-s`: search sequence substrings, on both strands by default.
    pub by_seq: bool,
    /// `-v`: invert the match.
    pub invert: bool,
    /// `-C`: print the count only.
    pub count: bool,
    /// `-i`: ignore case.
    pub ignore_case: bool,
    /// `-D`: emit a record once per matching pattern, including duplicates.
    pub allow_duplicated: bool,
    /// `-c`: allow a motif to cross the circular-sequence origin.
    pub circular: bool,
    /// `--delete-matched`: retire a pattern after its first match.
    pub delete_matched: bool,
    /// `-m`: maximum Hamming mismatches for sequence motifs.
    pub max_mismatch: usize,
    /// `-P`: do not search the reverse-complement strand.
    pub only_positive: bool,
    /// `-R`: restrict sequence matching to this region.
    pub region: Option<String>,
    /// `-r`: interpret patterns as regular expressions.
    pub use_regexp: bool,
    /// `-d`: interpret IUPAC degenerate bases in motifs.
    pub degenerate: bool,
    /// `-X`: additional input paths, one per line.
    pub infile_list: Option<String>,
    /// Skip the input existence check.
    pub skip_file_check: bool,
    /// `--id-regexp`: extract the ID with the first capture group.
    pub id_regexp: Option<String>,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

pub(crate) struct Pattern {
    raw: Vec<u8>,
    regex: Option<Regex>,
}

impl Pattern {
    pub(crate) fn matches(&self, target: &[u8], options: &GrepOptions) -> bool {
        if let Some(regex) = &self.regex {
            let extended = if options.circular {
                let mut extended = target.to_vec();
                extended.extend_from_slice(target);
                extended
            } else {
                target.to_vec()
            };
            if regex.is_match(&String::from_utf8_lossy(&extended)) {
                return true;
            }
            if options.by_seq && !options.only_positive {
                let reverse = transform::reverse_complement(&extended);
                if regex.is_match(&String::from_utf8_lossy(&reverse)) {
                    return true;
                }
            }
            return false;
        }
        if !options.by_seq {
            let target = if options.ignore_case {
                target.to_ascii_lowercase()
            } else {
                target.to_vec()
            };
            let pattern = if options.ignore_case {
                self.raw.to_ascii_lowercase()
            } else {
                self.raw.clone()
            };
            return target == pattern;
        }
        if self.raw.is_empty() {
            return target.is_empty();
        }
        if options.circular {
            let mut extended = target.to_vec();
            let prefix = self.raw.len().saturating_sub(1).min(target.len());
            extended.extend_from_slice(&target[..prefix]);
            return self.matches_strand(&extended, options)
                || (!options.only_positive
                    && self.matches_strand(&transform::reverse_complement(&extended), options));
        }
        self.matches_strand(target, options)
            || (!options.only_positive
                && self.matches_strand(&transform::reverse_complement(target), options))
    }

    fn matches_strand(&self, target: &[u8], options: &GrepOptions) -> bool {
        if self.raw.len() > target.len() {
            return false;
        }
        target.windows(self.raw.len()).any(|window| {
            let mismatches = self
                .raw
                .iter()
                .zip(window)
                .filter(|(pattern, base)| {
                    if options.degenerate {
                        !degenerate_matches(**pattern, **base)
                    } else if options.ignore_case {
                        !pattern.eq_ignore_ascii_case(base)
                    } else {
                        pattern != base
                    }
                })
                .count();
            mismatches <= options.max_mismatch
        })
    }
}

fn degenerate_matches(pattern: u8, base: u8) -> bool {
    let pattern = pattern.to_ascii_uppercase();
    let base = base.to_ascii_uppercase();
    match pattern {
        b'A' => base == b'A',
        b'C' => base == b'C',
        b'G' => base == b'G',
        b'T' | b'U' => base == b'T' || base == b'U',
        b'R' => matches!(base, b'A' | b'G'),
        b'Y' => matches!(base, b'C' | b'T' | b'U'),
        b'S' => matches!(base, b'G' | b'C'),
        b'W' => matches!(base, b'A' | b'T' | b'U'),
        b'K' => matches!(base, b'G' | b'T' | b'U'),
        b'M' => matches!(base, b'A' | b'C'),
        b'B' => matches!(base, b'C' | b'G' | b'T' | b'U'),
        b'D' => matches!(base, b'A' | b'G' | b'T' | b'U'),
        b'H' => matches!(base, b'A' | b'C' | b'T' | b'U'),
        b'V' => matches!(base, b'A' | b'C' | b'G'),
        b'N' | b'X' => true,
        _ => pattern == base,
    }
}

pub(crate) fn load_patterns(options: &GrepOptions) -> Result<Vec<Pattern>, CommandError> {
    if options.use_regexp && options.degenerate {
        return Err(CommandError::usage(
            "--use-regexp and --degenerate cannot be used together",
        ));
    }
    let mut raw_patterns = Vec::new();
    for raw in &options.patterns {
        raw_patterns.extend(raw.split(',').map(|value| value.as_bytes().to_vec()));
    }
    if let Some(path) = &options.pattern_file {
        let text = std::fs::read_to_string(path)
            .map_err(|error| CommandError::io(format!("{path}: {error}")))?;
        raw_patterns.extend(text.lines().map(|line| line.as_bytes().to_vec()));
    }
    if raw_patterns.is_empty() {
        return Err(CommandError::usage(
            "grep needs at least one -p/--pattern or -f/--pattern-file",
        ));
    }
    raw_patterns
        .into_iter()
        .map(|raw| {
            let regex = if options.use_regexp {
                let text = String::from_utf8_lossy(&raw);
                Some(
                    RegexBuilder::new(&text)
                        .case_insensitive(options.ignore_case)
                        .build()
                        .map_err(|error| {
                            CommandError::usage(format!("invalid pattern: {error}")).with_code(255)
                        })?,
                )
            } else {
                None
            };
            Ok(Pattern { raw, regex })
        })
        .collect()
}

fn expand_inputs(options: &GrepOptions) -> Result<Vec<String>, CommandError> {
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
    Ok(if inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        inputs
    })
}

fn record_id(record: &OwnedRecord, options: &GrepOptions) -> Result<Vec<u8>, CommandError> {
    let Some(expression) = &options.id_regexp else {
        return Ok(record.id().to_vec());
    };
    let regex = RegexBuilder::new(expression).build().map_err(|error| {
        CommandError::usage(format!("invalid --id-regexp: {error}")).with_code(255)
    })?;
    if regex.captures_len() < 2 {
        return Err(CommandError::usage(
            "fastx: regular expression must contain a capturing group for the ID",
        )
        .with_code(255));
    }
    let name = String::from_utf8_lossy(&record.name);
    let captures = regex.captures(&name);
    Ok(captures
        .and_then(|captures| captures.get(1).or_else(|| captures.get(0)))
        .map(|match_| match_.as_str().as_bytes().to_vec())
        .unwrap_or_default())
}

pub(crate) fn target_for_record(
    record: &OwnedRecord,
    options: &GrepOptions,
    region: Option<&Region>,
) -> Result<Vec<u8>, CommandError> {
    if !options.by_seq {
        return if options.by_name {
            Ok(record.name.clone())
        } else {
            record_id(record, options)
        };
    }
    if let Some(region) = region {
        let (start, end) = region.bounds(record.sequence.len());
        Ok(record.sequence[start..end].to_vec())
    } else {
        Ok(record.sequence.clone())
    }
}

/// Run `grep`.
pub fn run(options: GrepOptions) -> Result<(), CommandError> {
    let patterns = load_patterns(&options)?;
    let inputs = expand_inputs(&options)?;
    let region = options.region.as_deref().map(Region::parse).transpose()?;
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    let mut hits = 0u64;
    let mut active = patterns;

    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let record = record?;
            let target = target_for_record(&record, &options, region.as_ref())?;
            let mut matched = Vec::new();
            for (index, pattern) in active.iter().enumerate() {
                if pattern.matches(&target, &options) {
                    matched.push(index);
                    if !options.allow_duplicated && !options.delete_matched {
                        break;
                    }
                }
            }
            let matched_count = matched.len();
            if options.delete_matched && matched_count > 0 {
                active = active
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| !matched.contains(index))
                    .map(|(_, pattern)| pattern)
                    .collect();
            }
            let selected = matched_count > 0;
            if selected == options.invert {
                continue;
            }
            let output_count = if options.allow_duplicated {
                matched_count
            } else {
                1
            };
            if options.count {
                hits += output_count as u64;
            } else {
                for _ in 0..output_count {
                    record.write_to(&mut out, layout, options.line_width)?;
                }
            }
        }
    }
    if options.count {
        writeln!(out, "{hits}")
            .map_err(|error| CommandError::io(format!("cannot write output: {error}")))?;
    }
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iupac_and_hamming_matching_are_distinct() {
        let pattern = Pattern {
            raw: b"ARY".to_vec(),
            regex: None,
        };
        let options = GrepOptions {
            inputs: vec![],
            patterns: vec![],
            pattern_file: None,
            by_name: false,
            by_seq: true,
            invert: false,
            count: false,
            ignore_case: false,
            allow_duplicated: false,
            circular: false,
            delete_matched: false,
            max_mismatch: 0,
            only_positive: true,
            region: None,
            use_regexp: false,
            degenerate: true,
            infile_list: None,
            skip_file_check: false,
            id_regexp: None,
            line_width: 60,
            out_file: None,
            compress_level: None,
        };
        assert!(pattern.matches(b"AAT", &options));
        assert!(!pattern.matches(b"AAG", &options));
        let mut mismatch = options;
        mismatch.degenerate = false;
        mismatch.max_mismatch = 1;
        let plain = Pattern {
            raw: b"AAA".to_vec(),
            regex: None,
        };
        assert!(mismatch_matches(&plain, b"AAT", &mismatch));
    }

    fn mismatch_matches(pattern: &Pattern, target: &[u8], options: &GrepOptions) -> bool {
        pattern.matches_strand(target, options)
    }
}
