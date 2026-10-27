//! `rust-fx` command-line entry point.
//!
//! The parse-and-dispatch shell only. Every command returns a
//! [`CommandError`](rust_fx::io::CommandError) carrying a shared exit-code class,
//! so the exit status is decided in one place.
//!
//! Exit codes follow the workspace contract (`bioinfo_common::exit`):
//! 0 success, 2 usage error, 3 input precondition violated, 4 I/O or resource
//! failure. A few upstream-specific validation paths carry an explicit override,
//! such as SeqKit head's exit 255 for `-n 0`.

use bioinfo_common::diagnostic::error_line;
use bioinfo_common::exit::ExitClass;
use bioinfo_common::provenance::Provenance;
use clap::ArgMatches;
use rust_fx::commands::{
    self, CommonOptions, DuplicateOptions, Fa2fqOptions, FaidxOptions, Fq2faOptions, Fx2tabOptions,
    GrepOptions, HeadOptions, LintOptions, RangeOptions, RenameOptions, ReplaceOptions,
    RestartOptions, RmdupOptions, SampleOptions, SeqOptions, SlidingOptions, SortOptions,
    SplitOptions, StatsOptions, SubseqOptions, Tab2fxOptions, TranslateOptions,
};
use rust_fx::io::CommandError;
use rust_fx::lint::Level;
use rust_fx::split::SplitBy;
use rust_fx::transform::Transforms;

/// The classes this binary declares.
const DECLARED_EXIT_CLASSES: [ExitClass; 4] = [
    ExitClass::Success,
    ExitClass::Usage,
    ExitClass::Input,
    ExitClass::Io,
];

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // `--version --verbose` wins over clap's short version, as in the other
    // workspace binaries.
    if wants_verbose_version(&raw) {
        print!("{}", provenance().render());
        std::process::exit(ExitClass::Success.code());
    }
    let matches = match rust_fx::cli::command().try_get_matches_from(
        std::iter::once(rust_fx::cli::PROGRAM.to_string()).chain(raw.iter().cloned()),
    ) {
        Ok(matches) => matches,
        Err(error) => {
            // clap prints help/version to stdout with exit 0 and diagnostics to
            // stderr with exit 2, which this preserves.
            let _ = error.print();
            std::process::exit(match error.kind() {
                clap::error::ErrorKind::DisplayHelp
                | clap::error::ErrorKind::DisplayVersion
                | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                    ExitClass::Success.code()
                }
                _ => ExitClass::Usage.code(),
            });
        }
    };

    let result = dispatch(&matches);
    match result {
        Ok(()) => std::process::exit(ExitClass::Success.code()),
        Err(error) => {
            eprintln!("{}", error_line(&error.message));
            std::process::exit(error.exit_code());
        }
    }
}

fn dispatch(matches: &ArgMatches) -> Result<(), CommandError> {
    let (name, sub) = matches
        .subcommand()
        .ok_or_else(|| CommandError::usage("a subcommand is required"))?;
    match name {
        "stats" => commands::stats(stats_options(sub)?),
        "seq" => commands::seq(seq_options(sub)?),
        "fq2fa" => commands::fq2fa(fq2fa_options(sub)?),
        "fx2tab" => commands::fx2tab(fx2tab_options(sub)?),
        "sample" => commands::sample(sample_options(sub)?),
        "split" => commands::split(split_options(sub)?),
        "head" => commands::head(head_options(sub)?),
        "subseq" => commands::subseq(subseq_options(sub)?),
        "range" => commands::range(range_options(sub)?),
        "sliding" => commands::sliding(sliding_options(sub)?),
        "duplicate" => commands::duplicate(duplicate_options(sub)?),
        "rmdup" => commands::rmdup(rmdup_options(sub)?),
        "faidx" => commands::faidx(faidx_options(sub)?),
        "rename" => commands::rename(rename_options(sub)?),
        "replace" => commands::replace(replace_options(sub)?),
        "tab2fx" => commands::tab2fx(tab2fx_options(sub)?),
        "restart" => commands::restart(restart_options(sub)?),
        "fa2fq" => commands::fa2fq(fa2fq_options(sub)?),
        "translate" => commands::translate(translate_options(sub)?),
        "common" => commands::common(common_options(sub)?),
        "grep" => commands::grep(grep_options(sub)?),
        "sort" => commands::sort(sort_options(sub)?),
        "lint" => commands::lint(lint_options(sub)?),
        other => Err(CommandError::usage(format!("unknown subcommand '{other}'"))),
    }
}

fn inputs(sub: &ArgMatches) -> Vec<String> {
    sub.get_many::<String>("inputs")
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

fn out_file(sub: &ArgMatches) -> Option<String> {
    sub.get_one::<String>("out_file").cloned()
}

fn compression_level(sub: &ArgMatches) -> Result<Option<i32>, CommandError> {
    let raw = sub
        .get_one::<String>("compress_level")
        .map(String::as_str)
        .unwrap_or("-1");
    let level = raw.parse::<i32>().map_err(|_| {
        CommandError::usage(format!(
            "invalid --compress-level '{raw}'; expected an integer"
        ))
    })?;
    Ok((level != -1).then_some(level))
}

fn line_width(sub: &ArgMatches) -> usize {
    sub.get_one::<String>("line_width")
        .and_then(|value| value.parse().ok())
        .unwrap_or(60)
}

/// Parse a signed integer flag where `-1` means "no limit".
fn optional_u64(sub: &ArgMatches, name: &str) -> Option<u64> {
    let raw = sub.get_one::<String>(name)?;
    match raw.parse::<i64>() {
        Ok(value) if value >= 0 => Some(value as u64),
        _ => None,
    }
}

/// Parse a signed float flag where a negative value means "no limit".
fn optional_f64(sub: &ArgMatches, name: &str) -> Option<f64> {
    let raw = sub.get_one::<String>(name)?;
    match raw.parse::<f64>() {
        Ok(value) if value >= 0.0 => Some(value),
        _ => None,
    }
}

/// Map `-E/--fq-encoding` onto an ASCII offset.
///
/// The documented values are `sanger`, `solexa`, `illumina-1.3+`,
/// `illumina-1.5+`, `illumina-1.8+`. Only the ASCII base matters for `stats`
/// arithmetic, and `solexa` shares Phred+64 with the Illumina 1.3+/1.5+ scales
/// for the small scores that appear in practice; that approximation is recorded in
/// `docs/EVIDENCE-W1-3.md` DR-7.
fn quality_base(encoding: &str) -> Result<u8, CommandError> {
    match encoding.to_ascii_lowercase().as_str() {
        "sanger" | "illumina-1.8+" => Ok(33),
        "solexa" | "illumina-1.3+" | "illumina-1.5+" => Ok(64),
        other => Err(CommandError::usage(format!(
            "unknown --fq-encoding '{other}'; choose one of sanger, solexa, illumina-1.3+, \
             illumina-1.5+, illumina-1.8+"
        ))),
    }
}

fn stats_options(sub: &ArgMatches) -> Result<StatsOptions, CommandError> {
    let encoding = sub
        .get_one::<String>("fq_encoding")
        .map(String::as_str)
        .unwrap_or("sanger");
    Ok(StatsOptions {
        inputs: inputs(sub),
        tabular: sub.get_flag("tabular"),
        all: sub.get_flag("all"),
        basename: sub.get_flag("basename"),
        gap_letters: sub
            .get_one::<String>("gap_letters")
            .map(|value| value.as_bytes().to_vec())
            .unwrap_or_else(|| b"- \t.".to_vec()),
        stdin_label: sub
            .get_one::<String>("stdin_label")
            .cloned()
            .unwrap_or_else(|| "-".to_string()),
        skip_err: sub.get_flag("skip_err"),
        quality_base: quality_base(encoding)?,
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn seq_options(sub: &ArgMatches) -> Result<SeqOptions, CommandError> {
    Ok(SeqOptions {
        inputs: inputs(sub),
        name: sub.get_flag("name"),
        only_id: sub.get_flag("only_id"),
        seq_only: sub.get_flag("seq_only"),
        qual_only: sub.get_flag("qual_only"),
        transforms: Transforms {
            remove_gaps: sub.get_flag("remove_gaps"),
            upper: sub.get_flag("upper"),
            lower: sub.get_flag("lower"),
            reverse: sub.get_flag("reverse"),
            complement: sub.get_flag("complement"),
            dna2rna: sub.get_flag("dna2rna"),
            rna2dna: sub.get_flag("rna2dna"),
        },
        gap_letters: sub
            .get_one::<String>("gap_letters")
            .map(|value| value.as_bytes().to_vec())
            .unwrap_or_else(|| b"- \t.".to_vec()),
        min_len: optional_u64(sub, "min_len"),
        max_len: optional_u64(sub, "max_len"),
        min_qual: optional_f64(sub, "min_qual"),
        max_qual: optional_f64(sub, "max_qual"),
        quality_base: sub
            .get_one::<String>("qual_ascii_base")
            .and_then(|value| value.parse().ok())
            .unwrap_or(33),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn fq2fa_options(sub: &ArgMatches) -> Result<Fq2faOptions, CommandError> {
    Ok(Fq2faOptions {
        inputs: inputs(sub),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn parse_length(text: &str) -> Result<u64, CommandError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(CommandError::usage("--length requires a number"));
    }
    let split = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, suffix) = text.split_at(split);
    let value: u64 = digits
        .parse()
        .map_err(|_| CommandError::usage(format!("invalid --length value: {text}")))?;
    let multiplier = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => {
            return Err(CommandError::usage(format!(
                "invalid --length suffix: {text}"
            )))
        }
    };
    value
        .checked_mul(multiplier)
        .ok_or_else(|| CommandError::usage(format!("--length overflows: {text}")))
}

fn parse_buffer_size(text: &str) -> Result<i64, CommandError> {
    let invalid = || {
        CommandError::runtime("invalid value of buffer size. supported unit: K, M, G")
            .with_code(255)
    };
    let (digits, multiplier) = match text.as_bytes().last() {
        Some(b'K') => (&text[..text.len() - 1], 1024_i64),
        Some(b'M') => (&text[..text.len() - 1], 1024_i64 * 1024),
        Some(b'G') => (&text[..text.len() - 1], 1024_i64 * 1024 * 1024),
        _ => (text, 1),
    };
    let unsigned = digits.strip_prefix('-').unwrap_or(digits);
    if unsigned.is_empty() || !unsigned.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    digits
        .parse::<i64>()
        .ok()
        .and_then(|value| value.checked_mul(multiplier))
        .ok_or_else(invalid)
}

fn head_options(sub: &ArgMatches) -> Result<HeadOptions, CommandError> {
    let number = sub
        .get_one::<String>("number")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| CommandError::usage("--number must be a non-negative integer"))?
        .unwrap_or(10);
    if number == 0 {
        return Err(
            CommandError::usage("value of flag --number should be greater than 0").with_code(255),
        );
    }
    Ok(HeadOptions {
        inputs: inputs(sub),
        number,
        length: sub
            .get_one::<String>("length")
            .map(|value| parse_length(value))
            .transpose()?,
        infile_list: sub.get_one::<String>("infile_list").cloned(),
        skip_file_check: sub.get_flag("skip_file_check"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn common_options(sub: &ArgMatches) -> Result<CommonOptions, CommandError> {
    Ok(CommonOptions {
        inputs: inputs(sub),
        by_name: sub.get_flag("by_name"),
        by_seq: sub.get_flag("by_seq"),
        embedded: sub.get_flag("embedded"),
        ignore_case: sub.get_flag("ignore_case"),
        only_positive: sub.get_flag("only_positive"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn translate_options(sub: &ArgMatches) -> Result<TranslateOptions, CommandError> {
    let frames = sub
        .get_many::<String>("frame")
        .map(|values| {
            values
                .map(|value| value.parse::<i32>())
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
        .map_err(|_| CommandError::usage("--frame must be an integer"))?
        .unwrap_or_else(|| vec![1]);
    let table = sub
        .get_one::<String>("transl_table")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(1);
    let list_table = sub
        .get_one::<String>("list_table")
        .map(|value| value.parse::<i32>())
        .transpose()
        .map_err(|_| CommandError::usage("--list-transl-table must be an integer"))?
        .unwrap_or(-1);
    let list_ambiguous = sub
        .get_one::<String>("list_ambiguous")
        .map(|value| value.parse::<i32>())
        .transpose()
        .map_err(|_| CommandError::usage("--list-transl-table-with-amb-codons must be an integer"))?
        .unwrap_or(-1);
    Ok(TranslateOptions {
        inputs: inputs(sub),
        frames,
        table,
        list_table,
        list_ambiguous,
        allow_unknown: sub.get_flag("allow_unknown"),
        init_m: sub.get_flag("init_m"),
        append_frame: sub.get_flag("append_frame"),
        out_subseqs: sub.get_flag("out_subseqs"),
        clean: sub.get_flag("clean"),
        min_len: sub
            .get_one::<String>("min_len")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| CommandError::usage("--min-len must be a non-negative integer"))?
            .unwrap_or(0),
        skip_errors: sub.get_flag("skip_errors"),
        trim: sub.get_flag("trim"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn fa2fq_options(sub: &ArgMatches) -> Result<Fa2fqOptions, CommandError> {
    Ok(Fa2fqOptions {
        fasta: sub.get_one::<String>("fasta").cloned().unwrap_or_default(),
        inputs: inputs(sub),
        only_positive: sub.get_flag("only_positive"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn restart_options(sub: &ArgMatches) -> Result<RestartOptions, CommandError> {
    let new_start = sub
        .get_one::<String>("new_start")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1);
    if new_start == 0 {
        return Err(CommandError::usage("--new-start must not be 0"));
    }
    let max_mismatch = sub
        .get_one::<String>("max_mismatch")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| CommandError::usage("--max-mismatch must be a non-negative integer"))?
        .unwrap_or(0);
    Ok(RestartOptions {
        inputs: inputs(sub),
        new_start,
        start_with: sub.get_one::<String>("start_with").cloned(),
        ignore_case: sub.get_flag("ignore_case"),
        max_mismatch,
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn tab2fx_options(sub: &ArgMatches) -> Result<Tab2fxOptions, CommandError> {
    Ok(Tab2fxOptions {
        inputs: inputs(sub),
        comment_prefixes: sub
            .get_many::<String>("comment_prefix")
            .map(|values| values.cloned().collect())
            .unwrap_or_else(|| vec!["#".to_string(), "//".to_string()]),
        buffer_size: parse_buffer_size(
            sub.get_one::<String>("buffer_size")
                .map(String::as_str)
                .unwrap_or("1G"),
        )?,
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn replace_options(sub: &ArgMatches) -> Result<ReplaceOptions, CommandError> {
    let key_capt_idx = sub
        .get_one::<String>("key_capt_idx")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| CommandError::usage("--key-capt-idx must be a positive integer"))?
        .unwrap_or(1);
    if key_capt_idx == 0 {
        return Err(CommandError::usage(
            "--key-capt-idx must be greater than zero",
        ));
    }
    Ok(ReplaceOptions {
        inputs: inputs(sub),
        pattern: sub
            .get_one::<String>("pattern")
            .cloned()
            .ok_or_else(|| CommandError::usage("replace needs -p/--pattern"))?,
        replacement: sub
            .get_one::<String>("replacement")
            .cloned()
            .ok_or_else(|| CommandError::usage("replace needs -r/--replacement"))?,
        by_seq: sub.get_flag("by_seq"),
        ignore_case: sub.get_flag("ignore_case"),
        nr_width: sub
            .get_one::<String>("nr_width")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| CommandError::usage("--nr-width must be a positive integer"))?
            .unwrap_or(1),
        kv_file: sub.get_one::<String>("kv_file").cloned(),
        keep_key: sub.get_flag("keep_key"),
        keep_untouch: sub.get_flag("keep_untouch"),
        key_capt_idx,
        key_miss_repl: sub.get_one::<String>("key_miss_repl").cloned(),
        f_by_name: sub.get_flag("f_by_name"),
        f_by_seq: sub.get_flag("f_by_seq"),
        f_ignore_case: sub.get_flag("f_ignore_case"),
        f_invert_match: sub.get_flag("f_invert_match"),
        f_only_positive: sub.get_flag("f_only_positive"),
        f_patterns: sub
            .get_many::<String>("f_patterns")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        f_pattern_file: sub.get_one::<String>("f_pattern_file").cloned(),
        f_use_regexp: sub.get_flag("f_use_regexp"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn rename_options(sub: &ArgMatches) -> Result<RenameOptions, CommandError> {
    let start_num = sub
        .get_one::<String>("start_num")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2);
    if start_num == 0 {
        return Err(CommandError::usage("--start-num must be greater than zero"));
    }
    Ok(RenameOptions {
        inputs: inputs(sub),
        by_name: sub.get_flag("by_name"),
        rename_first: sub.get_flag("rename_first"),
        separator: sub
            .get_one::<String>("separator")
            .cloned()
            .unwrap_or_else(|| "_".to_string()),
        start_num,
        multiple_outfiles: sub.get_flag("multiple_outfiles"),
        out_dir: sub.get_one::<String>("out_dir").cloned(),
        force: sub.get_flag("force"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn faidx_options(sub: &ArgMatches) -> Result<FaidxOptions, CommandError> {
    Ok(FaidxOptions {
        fasta: sub
            .get_one::<String>("fasta")
            .cloned()
            .ok_or_else(|| CommandError::usage("faidx needs FASTA"))?,
        regions: sub
            .get_many::<String>("regions")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        full_head: sub.get_flag("full_head"),
        ignore_case: sub.get_flag("ignore_case"),
        region_file: sub.get_one::<String>("region_file").cloned(),
        regexp: sub.get_flag("regexp"),
        update_faidx: sub.get_flag("update_faidx"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn rmdup_options(sub: &ArgMatches) -> Result<RmdupOptions, CommandError> {
    Ok(RmdupOptions {
        inputs: inputs(sub),
        by_name: sub.get_flag("by_name"),
        by_seq: sub.get_flag("by_seq"),
        ignore_case: sub.get_flag("ignore_case"),
        only_positive: sub.get_flag("only_positive"),
        dup_num_file: sub.get_one::<String>("dup_num_file").cloned(),
        dup_seqs_file: sub.get_one::<String>("dup_seqs_file").cloned(),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn duplicate_options(sub: &ArgMatches) -> Result<DuplicateOptions, CommandError> {
    let times = sub
        .get_one::<String>("times")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    Ok(DuplicateOptions {
        inputs: inputs(sub),
        times,
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn sliding_options(sub: &ArgMatches) -> Result<SlidingOptions, CommandError> {
    let window = sub
        .get_one::<String>("window")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let step = sub
        .get_one::<String>("step")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if window == 0 || step == 0 {
        return Err(CommandError::usage(
            "sliding needs positive -W/--window and -s/--step",
        ));
    }
    Ok(SlidingOptions {
        inputs: inputs(sub),
        window,
        step,
        suffix: sub
            .get_one::<String>("suffix")
            .cloned()
            .unwrap_or_else(|| "_sliding".to_string()),
        greedy: sub.get_flag("greedy"),
        circular: sub.get_flag("circular") || sub.get_flag("circular_genome"),
        infile_list: sub.get_one::<String>("infile_list").cloned(),
        skip_file_check: sub.get_flag("skip_file_check"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn range_options(sub: &ArgMatches) -> Result<RangeOptions, CommandError> {
    Ok(RangeOptions {
        inputs: inputs(sub),
        region: sub
            .get_one::<String>("region")
            .cloned()
            .ok_or_else(|| CommandError::usage("range needs -r/--range"))?,
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn subseq_options(sub: &ArgMatches) -> Result<SubseqOptions, CommandError> {
    Ok(SubseqOptions {
        inputs: inputs(sub),
        region: sub.get_one::<String>("region").cloned(),
        bed: sub.get_one::<String>("bed").cloned(),
        gtf: sub.get_one::<String>("gtf").cloned(),
        features: sub
            .get_many::<String>("feature")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        gtf_tag: sub
            .get_one::<String>("gtf_tag")
            .cloned()
            .unwrap_or_else(|| "gene_id".to_string()),
        chr: sub
            .get_many::<String>("chr")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        up_stream: sub
            .get_one::<String>("up_stream")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        down_stream: sub
            .get_one::<String>("down_stream")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        only_flank: sub.get_flag("only_flank"),
        region_coord: sub.get_flag("region_coord"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn grep_options(sub: &ArgMatches) -> Result<GrepOptions, CommandError> {
    Ok(GrepOptions {
        inputs: inputs(sub),
        patterns: sub
            .get_many::<String>("pattern")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        pattern_file: sub.get_one::<String>("pattern_file").cloned(),
        by_name: sub.get_flag("by_name"),
        by_seq: sub.get_flag("by_seq"),
        invert: sub.get_flag("invert"),
        count: sub.get_flag("count"),
        ignore_case: sub.get_flag("ignore_case"),
        allow_duplicated: sub.get_flag("allow_duplicated"),
        circular: sub.get_flag("circular"),
        delete_matched: sub.get_flag("delete_matched"),
        max_mismatch: sub
            .get_one::<String>("max_mismatch")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| CommandError::usage("--max-mismatch must be a non-negative integer"))?
            .unwrap_or(0),
        only_positive: sub.get_flag("only_positive"),
        region: sub.get_one::<String>("region").cloned(),
        use_regexp: sub.get_flag("use_regexp"),
        degenerate: sub.get_flag("degenerate"),
        infile_list: sub.get_one::<String>("infile_list").cloned(),
        skip_file_check: sub.get_flag("skip_file_check"),
        id_regexp: sub.get_one::<String>("id_regexp").cloned(),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn sort_options(sub: &ArgMatches) -> Result<SortOptions, CommandError> {
    Ok(SortOptions {
        inputs: inputs(sub),
        by_length: sub.get_flag("by_length"),
        by_name: sub.get_flag("by_name"),
        by_seq: sub.get_flag("by_seq"),
        by_bases: sub.get_flag("by_bases"),
        ignore_case: sub.get_flag("ignore_case"),
        reverse: sub.get_flag("reverse"),
        gap_letters: sub
            .get_one::<String>("gap_letters")
            .map(|value| value.as_bytes().to_vec())
            .unwrap_or_else(|| b"- \t.".to_vec()),
        seq_prefix_length: sub
            .get_one::<String>("seq_prefix_length")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| CommandError::usage("--seq-prefix-length must be a non-negative integer"))?
            .unwrap_or(10_000),
        natural_order: sub.get_flag("natural_order"),
        two_pass: sub.get_flag("two_pass"),
        update_faidx: sub.get_flag("update_faidx"),
        keep_temp: sub.get_flag("keep_temp"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn fx2tab_options(sub: &ArgMatches) -> Result<Fx2tabOptions, CommandError> {
    Ok(Fx2tabOptions {
        inputs: inputs(sub),
        header_line: sub.get_flag("header_line"),
        length: sub.get_flag("length"),
        gc: sub.get_flag("gc"),
        gc_skew: sub.get_flag("gc_skew"),
        name: sub.get_flag("name"),
        only_id: sub.get_flag("only_id"),
        no_qual: sub.get_flag("no_qual"),
        avg_qual: sub.get_flag("avg_qual"),
        seq_hash: sub.get_flag("seq_hash"),
        alphabet: sub.get_flag("alphabet"),
        case_sensitive: sub.get_flag("case_sensitive"),
        base_content: sub
            .get_many::<String>("base_content")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        base_count: sub
            .get_many::<String>("base_count")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
        basename: sub.get_flag("basename"),
        file_name: sub.get_flag("file_name"),
        quality_base: sub
            .get_one::<String>("qual_ascii_base")
            .and_then(|value| value.parse().ok())
            .unwrap_or(33),
        stdin_label: sub
            .get_one::<String>("stdin_label")
            .cloned()
            .unwrap_or_else(|| "-".to_string()),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn sample_options(sub: &ArgMatches) -> Result<SampleOptions, CommandError> {
    Ok(SampleOptions {
        inputs: inputs(sub),
        proportion: sub
            .get_one::<String>("proportion")
            .map(|value| value.parse())
            .transpose()
            .map_err(|_| CommandError::usage("--proportion must be a number"))?,
        number: optional_u64(sub, "number"),
        rand_seed: sub
            .get_one::<String>("rand_seed")
            .and_then(|value| value.parse().ok())
            .unwrap_or(11),
        non_deterministic: sub.get_flag("non_deterministic"),
        line_width: line_width(sub),
        out_file: out_file(sub),
        compress_level: compression_level(sub)?,
    })
}

fn split_options(sub: &ArgMatches) -> Result<SplitOptions, CommandError> {
    let by_size = optional_u64(sub, "by_size");
    let by_part = optional_u64(sub, "by_part");
    let by = match (by_size, by_part) {
        (Some(size), None) => SplitBy::Size(size),
        (None, Some(parts)) => SplitBy::Parts(parts),
        (Some(_), Some(_)) => {
            return Err(CommandError::usage(
                "only one of -s/--by-size and -p/--by-part may be given",
            ))
        }
        (None, None) => {
            return Err(CommandError::usage(
                "one of -s/--by-size and -p/--by-part is required",
            ))
        }
    };
    let input = sub
        .get_one::<String>("input")
        .cloned()
        .ok_or_else(|| CommandError::usage("split needs an input file"))?;
    Ok(SplitOptions {
        input,
        by,
        out_dir: sub.get_one::<String>("out_dir").cloned(),
        out_prefix: sub.get_one::<String>("out_prefix").cloned(),
        part_width: sub
            .get_one::<String>("part_width")
            .and_then(|value| value.parse().ok())
            .unwrap_or(3),
        extension: sub.get_one::<String>("extension").cloned(),
        dry_run: sub.get_flag("dry_run"),
        force: sub.get_flag("force"),
        compress_level: compression_level(sub)?,
    })
}

fn lint_options(sub: &ArgMatches) -> Result<LintOptions, CommandError> {
    // lint has no compressed output, but the shared global option still must
    // reject malformed numeric values rather than being silently ignored.
    let _ = compression_level(sub)?;
    let parse_level = |name: &str, default: Level| -> Result<Level, CommandError> {
        match sub.get_one::<String>(name) {
            Some(value) => Level::parse(value)
                .ok_or_else(|| CommandError::usage(format!("unknown level '{value}'"))),
            None => Ok(default),
        }
    };
    Ok(LintOptions {
        r1: sub
            .get_one::<String>("r1")
            .cloned()
            .ok_or_else(|| CommandError::usage("lint needs R1_SRC"))?,
        r2: sub.get_one::<String>("r2").cloned(),
        log_mode: sub.get_one::<String>("lint_mode").map(String::as_str) == Some("log"),
        single_level: parse_level("single_read_validation_level", Level::High)?,
        paired_level: parse_level("paired_read_validation_level", Level::High)?,
        disable: sub
            .get_many::<String>("disable_validator")
            .map(|values| values.cloned().collect())
            .unwrap_or_default(),
    })
}

/// `--version --verbose` is handled before clap so the detailed report wins.
fn wants_verbose_version(raw: &[String]) -> bool {
    let mut version = false;
    let mut verbose = false;
    for argument in raw {
        match argument.as_str() {
            "--version" | "-V" => version = true,
            "--verbose" => verbose = true,
            _ => {}
        }
    }
    version && verbose
}

/// The provenance block, built with the shared builder.
fn provenance() -> Provenance {
    Provenance::new(rust_fx::cli::PROGRAM, env!("CARGO_PKG_VERSION"))
        .with_field(
            "reference",
            "SeqKit 2.14.0 and fq 0.12.0 (documented behaviour)",
        )
        .with_field(
            "verification",
            "NOT differentially verified: the oracles are unavailable (docs/EVIDENCE-W1-3.md)",
        )
        .with_field("status", "experimental")
        .with_field("fastx", "bioinfo-fastx (strict parser, gzip/bzip2/xz/zstd)")
        .with_field(
            "commands",
            "stats, seq, fq2fa, fx2tab, sample, split, head, grep, sort, subseq, range, sliding, duplicate, rmdup, faidx, rename, replace, tab2fx, restart, fa2fq, translate, common, lint",
        )
        .with_exit_codes(&DECLARED_EXIT_CLASSES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_records_the_unverified_status() {
        let text = provenance().render();
        assert!(text.starts_with("rust-fx 0.1.0\n"), "{text}");
        assert!(text.contains("experimental"), "{text}");
        assert!(
            text.contains("NOT differentially verified"),
            "the provenance block must not imply oracle parity: {text}"
        );
    }

    #[test]
    fn provenance_lists_every_command() {
        let text = provenance().render();
        for command in [
            "stats", "seq", "fq2fa", "fx2tab", "sample", "split", "head", "grep", "sort", "subseq",
            "lint",
        ] {
            assert!(text.contains(command), "{command} missing from {text}");
        }
    }

    #[test]
    fn declared_classes_render_the_shared_line() {
        assert_eq!(
            bioinfo_common::exit::describe_exit_codes(&DECLARED_EXIT_CLASSES),
            "0 ok, 2 usage, 3 input, 4 io/resource"
        );
    }

    #[test]
    fn verbose_version_detection() {
        assert!(wants_verbose_version(&[
            "--version".into(),
            "--verbose".into()
        ]));
        assert!(wants_verbose_version(&["-V".into(), "--verbose".into()]));
        assert!(!wants_verbose_version(&["--version".into()]));
        assert!(!wants_verbose_version(&["--verbose".into()]));
        assert!(!wants_verbose_version(&[]));
    }

    #[test]
    fn quality_encoding_maps_to_the_documented_bases() {
        assert_eq!(quality_base("sanger").unwrap(), 33);
        assert_eq!(quality_base("illumina-1.8+").unwrap(), 33);
        assert_eq!(quality_base("solexa").unwrap(), 64);
        assert_eq!(quality_base("illumina-1.3+").unwrap(), 64);
        assert_eq!(quality_base("illumina-1.5+").unwrap(), 64);
        assert_eq!(quality_base("SANGER").unwrap(), 33);
        assert!(quality_base("bogus").is_err());
    }
}
