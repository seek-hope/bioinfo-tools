//! SeqKit `replace`: filtered regular-expression edits and replacement symbols.

use crate::commands::Region;
use crate::grep::{self, GrepOptions};
use crate::io::{CommandError, InputFormat, Output, OwnedRecord, RecordReader};
use bioinfo_fastx::writer::Layout;
use regex::{Regex, RegexBuilder};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The parsed `replace` options.
pub struct ReplaceOptions {
    /// Optional SeqKit compression level (`None` means `-1`).
    pub compress_level: Option<i32>,
    /// Input files; empty means standard input.
    pub inputs: Vec<String>,
    /// `-p`: regular expression.
    pub pattern: String,
    /// `-r`: replacement, supporting capture variables and replacement symbols.
    pub replacement: String,
    /// `-s`: replace sequence instead of name.
    pub by_seq: bool,
    /// `-i`: case-insensitive pattern.
    pub ignore_case: bool,
    /// `--nr-width`: minimum width of `{nr}`.
    pub nr_width: usize,
    /// `-k`: tab-delimited key-value file used by `{kv}`.
    pub kv_file: Option<String>,
    /// `-K`: use the key when no value is found.
    pub keep_key: bool,
    /// `-U`: leave the record name unchanged when no value is found.
    pub keep_untouch: bool,
    /// `-I`: capture group containing the key (1-based).
    pub key_capt_idx: usize,
    /// `-m`: replacement when the key has no value.
    pub key_miss_repl: Option<String>,
    /// `--f-by-name`: filter by full name instead of ID.
    pub f_by_name: bool,
    /// `--f-by-seq`: filter by sequence.
    pub f_by_seq: bool,
    /// `--f-ignore-case`: ignore case while filtering.
    pub f_ignore_case: bool,
    /// `--f-invert-match`: edit only records that do not match.
    pub f_invert_match: bool,
    /// `--f-only-positive-strand`: do not search the reverse strand.
    pub f_only_positive: bool,
    /// `--f-pattern`: filter patterns.
    pub f_patterns: Vec<String>,
    /// `--f-pattern-file`: filter pattern file.
    pub f_pattern_file: Option<String>,
    /// `--f-use-regexp`: interpret filter patterns as regular expressions.
    pub f_use_regexp: bool,
    /// `-w`: FASTA line width.
    pub line_width: usize,
    /// `-o`: output file.
    pub out_file: Option<String>,
}

fn filter_options(options: &ReplaceOptions) -> GrepOptions {
    GrepOptions {
        compress_level: options.compress_level,
        inputs: Vec::new(),
        patterns: options.f_patterns.clone(),
        pattern_file: options.f_pattern_file.clone(),
        by_name: options.f_by_name,
        by_seq: options.f_by_seq,
        invert: false,
        count: false,
        ignore_case: options.f_ignore_case,
        allow_duplicated: false,
        circular: false,
        delete_matched: false,
        max_mismatch: 0,
        only_positive: options.f_only_positive,
        region: None,
        use_regexp: options.f_use_regexp,
        degenerate: false,
        infile_list: None,
        skip_file_check: false,
        id_regexp: None,
        line_width: options.line_width,
        out_file: None,
    }
}

fn load_kv(path: &Path) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, CommandError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CommandError::io(format!("{}: {error}", path.display())))?;
    let mut values = BTreeMap::new();
    for (line_number, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('\t').ok_or_else(|| {
            CommandError::input(format!(
                "{}:{}: expected key<TAB>value",
                path.display(),
                line_number + 1
            ))
        })?;
        values.insert(key.as_bytes().to_vec(), value.as_bytes().to_vec());
    }
    Ok(values)
}

fn file_parts(input: &str) -> (String, String, String) {
    let path = PathBuf::from(input);
    let basename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string());
    let stem = Path::new(&basename)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| basename.clone());
    (input.to_string(), basename, stem)
}

fn uuid(seed: u64) -> String {
    let mut state = seed.max(1);
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let bytes: Vec<u8> = (0..16).map(|_| next() as u8).collect();
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn expand_symbols(
    template: &str,
    input: &str,
    record_number: usize,
    nr_width: usize,
    uuid_seed: u64,
) -> String {
    let (filename, basename, stem) = file_parts(input);
    let number = format!("{record_number:0nr_width$}");
    template
        .replace("{nr}", &number)
        .replace("{fn}", &filename)
        .replace("{fbn}", &basename)
        .replace("{fbne}", &stem)
        .replace("{uuid}", &uuid(uuid_seed))
}

fn replacement_for_record(
    options: &ReplaceOptions,
    regex: &Regex,
    record: &OwnedRecord,
    input: &str,
    record_number: usize,
    uuid_seed: u64,
    kv: Option<&BTreeMap<Vec<u8>, Vec<u8>>>,
) -> Result<Option<String>, CommandError> {
    let name = String::from_utf8_lossy(&record.name);
    let template = expand_symbols(
        &options.replacement,
        input,
        record_number,
        options.nr_width,
        uuid_seed,
    );
    let Some(kv_values) = kv else {
        return Ok(Some(regex.replace_all(&name, &template).into_owned()));
    };
    let Some(captures) = regex.captures(&name) else {
        return Ok(Some(regex.replace_all(&name, &template).into_owned()));
    };
    let key = captures
        .get(options.key_capt_idx)
        .map(|match_| match_.as_str().as_bytes().to_vec());
    let value = key.as_ref().and_then(|key| kv_values.get(key));
    if value.is_none() {
        if options.keep_untouch {
            return Ok(None);
        }
        if options.keep_key {
            let key = key.unwrap_or_default();
            return Ok(Some(
                regex
                    .replace_all(&name, String::from_utf8_lossy(&key).as_ref())
                    .into_owned(),
            ));
        }
        if let Some(replacement) = &options.key_miss_repl {
            return Ok(Some(
                regex.replace_all(&name, replacement.as_str()).into_owned(),
            ));
        }
    }
    let replacement = value
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .unwrap_or_default();
    Ok(Some(regex.replace_all(&name, replacement).into_owned()))
}

/// Run `replace`.
pub fn run(options: ReplaceOptions) -> Result<(), CommandError> {
    if options.by_seq && options.kv_file.is_some() {
        return Err(CommandError::usage(
            "replacing with key-value pairs is not supported for sequence",
        )
        .with_code(255));
    }
    let regex = RegexBuilder::new(&options.pattern)
        .case_insensitive(options.ignore_case)
        .build()
        .map_err(|error| {
            CommandError::usage(format!("invalid --pattern: {error}")).with_code(255)
        })?;
    let filters = filter_options(&options);
    let patterns = if filters.patterns.is_empty() && filters.pattern_file.is_none() {
        Vec::new()
    } else {
        grep::load_patterns(&filters)?
    };
    let kv = options
        .kv_file
        .as_deref()
        .map(|path| load_kv(Path::new(path)))
        .transpose()?;
    let inputs = if options.inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        options.inputs.clone()
    };
    let mut out = Output::create_with_level(options.out_file.as_deref(), options.compress_level)?;
    let mut record_number = 0usize;
    let uuid_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64 ^ u64::from(std::process::id()))
        .unwrap_or(1);
    for input in &inputs {
        let mut reader = RecordReader::open(input)?;
        let layout = match reader.format() {
            InputFormat::Fastq => Layout::Fastq,
            InputFormat::Fasta => Layout::Fasta,
        };
        while let Some(record) = reader.next_record() {
            let mut record = record?;
            record_number += 1;
            if !patterns.is_empty() {
                let target = grep::target_for_record(&record, &filters, None::<&Region>)?;
                let matched = patterns
                    .iter()
                    .any(|pattern| pattern.matches(&target, &filters));
                if matched == options.f_invert_match {
                    record.write_to(&mut out, layout, options.line_width)?;
                    continue;
                }
            }
            if options.by_seq {
                let original = record.sequence.len();
                let sequence_text = String::from_utf8_lossy(&record.sequence).into_owned();
                let sequence = regex.replace_all(&sequence_text, options.replacement.as_str());
                if record.has_quality_line && sequence.len() != original {
                    return Err(CommandError::input(
                        "replace -s cannot change the sequence length of FASTQ records",
                    ));
                }
                record.sequence = sequence.into_owned().into_bytes();
            } else if let Some(name) = replacement_for_record(
                &options,
                &regex,
                &record,
                input,
                record_number,
                uuid_seed.wrapping_add(record_number as u64),
                kv.as_ref(),
            )? {
                record.name = name.into_bytes();
                let marker = record.raw_header.first().copied().unwrap_or(b'>');
                record.raw_header = format!(
                    "{}{}",
                    char::from(marker),
                    String::from_utf8_lossy(&record.name)
                )
                .into_bytes();
            }
            record.write_to(&mut out, layout, options.line_width)?;
        }
    }
    out.finish()
}
