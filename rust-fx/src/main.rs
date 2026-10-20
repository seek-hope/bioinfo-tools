//! `rust-fx` command-line entry point.
//!
//! The parse-and-dispatch shell only. Every command returns a
//! [`CommandError`](rust_fx::io::CommandError) carrying a shared exit-code class,
//! so the exit status is decided in one place.
//!
//! Exit codes follow the workspace contract (`bioinfo_common::exit`):
//! 0 success, 2 usage error, 3 input precondition violated, 4 I/O or resource
//! failure. `rust-fx` does not use 1: a data problem is an input or I/O failure.

use bioinfo_common::diagnostic::error_line;
use bioinfo_common::exit::ExitClass;
use bioinfo_common::provenance::Provenance;
use clap::ArgMatches;
use rust_fx::commands::{
    self, Fq2faOptions, Fx2tabOptions, LintOptions, SampleOptions, SeqOptions, SplitOptions,
    StatsOptions,
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
        "fq2fa" => commands::fq2fa(fq2fa_options(sub)),
        "fx2tab" => commands::fx2tab(fx2tab_options(sub)?),
        "sample" => commands::sample(sample_options(sub)?),
        "split" => commands::split(split_options(sub)?),
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
    })
}

fn fq2fa_options(sub: &ArgMatches) -> Fq2faOptions {
    Fq2faOptions {
        inputs: inputs(sub),
        line_width: line_width(sub),
        out_file: out_file(sub),
    }
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
    })
}

fn lint_options(sub: &ArgMatches) -> Result<LintOptions, CommandError> {
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
        .with_field("commands", "stats, seq, fq2fa, fx2tab, sample, split, lint")
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
        for command in ["stats", "seq", "fq2fa", "fx2tab", "sample", "split", "lint"] {
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
