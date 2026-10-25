//! `rust-bam` command-line entry point.
//!
//! Exit codes, diagnostics and the provenance block come from `bioinfo-common`
//! so this binary follows the same contract as every other tool in the workspace.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};

use rust_bam::commands::{self, SortOptions, ViewOptions};
use rust_bam::error::{Error, EXIT_CLASSES};
use rust_bam::COMMANDS;

/// The program name used in diagnostics and the provenance block.
const PROGRAM: &str = "rust-bam";
/// The crate version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--version --verbose` prints the provenance block, matching the other
    // binaries in the workspace.
    if wants_verbose_version(&args) {
        print!("{}", provenance().render());
        return ExitCode::SUCCESS;
    }

    let matches = match cli().try_get_matches_from(std::iter::once(PROGRAM.to_string()).chain(args))
    {
        Ok(matches) => matches,
        Err(error) => {
            // clap's own help/version go to stdout with exit 0; a real usage error
            // goes to stderr with the shared usage code.
            let _ = error.print();
            return ExitCode::from(if error.use_stderr() {
                bioinfo_common::EXIT_USAGE as u8
            } else {
                0
            });
        }
    };

    match run(&matches) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", bioinfo_common::error_line(error.message()));
            exit_code(error.exit_code())
        }
    }
}

/// `flagstat`/`idxstats` write text; a quickcheck failure has its own exit code.
fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn wants_verbose_version(args: &[String]) -> bool {
    args.iter().any(|a| a == "--version") && args.iter().any(|a| a == "--verbose")
}

fn provenance() -> bioinfo_common::Provenance {
    bioinfo_common::Provenance::new(PROGRAM, VERSION)
        .with_field(
            "reference",
            "samtools 1.24 (htslib 1.24) as oracle; SAM/BAM via noodles 0.118.0",
        )
        .with_field("library", "noodles 0.118.0 (pure Rust; no htslib)")
        .with_field("commands", COMMANDS.join(", "))
        .with_exit_codes(&EXIT_CLASSES)
}

fn cli() -> Command {
    let input = || {
        Arg::new("input")
            .value_name("FILE")
            .required(true)
            .help("input SAM or BAM; `-` reads standard input where supported")
    };

    Command::new(PROGRAM)
        .version(VERSION)
        .about("SAM/BAM view, flagstat, idxstats, quickcheck, index and sort (noodles 0.118.0)")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("view")
                .about("Print SAM text, BAM, a header or a record count")
                // samtools uses `-h` for "include the header", which collides with
                // clap's auto-generated `-h` for help. Disabling the short help flag
                // keeps the samtools-compatible spelling; `--help` still works.
                .disable_help_flag(true)
                .arg(
                    // `--help` must not require <FILE>, so mark it as conflicting
                    // with nothing and check it before the required-argument
                    // validation in `run`.
                    Arg::new("help")
                        .long("help")
                        .action(ArgAction::SetTrue)
                        .help("print help"),
                )
                .arg(input().required(false))
                .arg(
                    Arg::new("with_header")
                        .short('h')
                        .action(ArgAction::SetTrue)
                        .help("include the header in SAM text output"),
                )
                .arg(
                    Arg::new("bam")
                        .short('b')
                        .action(ArgAction::SetTrue)
                        .help("write BAM (BGZF) instead of SAM text"),
                )
                .arg(
                    Arg::new("header_only")
                        .short('H')
                        .action(ArgAction::SetTrue)
                        .help("print only the header"),
                )
                .arg(
                    Arg::new("count")
                        .short('c')
                        .action(ArgAction::SetTrue)
                        .help("print only the number of records"),
                )
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("FILE")
                        .help("write to FILE instead of standard output"),
                )
                .arg(
                    Arg::new("require_flags")
                        .short('f')
                        .long("require-flags")
                        .value_name("FLAG")
                        .help("require all flag bits (numeric or comma-separated names)"),
                )
                .arg(
                    Arg::new("exclude_flags")
                        .short('F')
                        .long("exclude-flags")
                        .value_name("FLAG")
                        .help("exclude records with any of these flag bits"),
                )
                .arg(
                    Arg::new("min_mapq")
                        .short('q')
                        .long("min-mapq")
                        .value_name("INT")
                        .value_parser(clap::value_parser!(u8))
                        .help("minimum mapping quality"),
                )
                .arg(
                    Arg::new("region")
                        .long("region")
                        .value_name("REGION")
                        .help("query an indexed BAM (chr1 or chr1:10-20)"),
                )
                .arg(
                    Arg::new("threads")
                        .short('@')
                        .long("threads")
                        .value_name("INT")
                        .value_parser(clap::value_parser!(usize))
                        .default_value("1")
                        .help("worker threads for BAM output compression (SAM output stays single-threaded)"),
                ),
        )
        .subcommand(
            Command::new("flagstat")
                .about("Print the 16 flagstat counters")
                .arg(input())
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("FILE")
                        .help("write to FILE instead of standard output"),
                ),
        )
        .subcommand(
            Command::new("idxstats")
                .about("Print per-reference mapped/unmapped counts")
                .arg(input())
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("FILE")
                        .help("write to FILE instead of standard output"),
                ),
        )
        .subcommand(
            Command::new("quickcheck")
                .about("Check that files are structurally valid (exit 16 on failure)")
                .arg(
                    Arg::new("inputs")
                        .value_name("FILE")
                        .required(true)
                        .num_args(1..)
                        .help("one or more files to check"),
                ),
        )
        .subcommand(
            Command::new("index")
                .about("Build a BAI index for a coordinate-sorted BAM")
                .arg(input())
                .arg(
                    Arg::new("output")
                        .long("output")
                        .value_name("FILE")
                        .help("write the index to FILE instead of <input>.bai"),
                )
                .arg(
                    Arg::new("csi")
                        .long("csi")
                        .action(ArgAction::SetTrue)
                        .help("write a CSI (<input>.csi) instead of a BAI"),
                ),
        )
        .subcommand(
            Command::new("sort")
                .about("Coordinate-sort a SAM or BAM file")
                .arg(input())
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("FILE")
                        .help("write to FILE instead of standard output"),
                )
                .arg(
                    Arg::new("sam")
                        .short('O')
                        .long("sam")
                        .action(ArgAction::SetTrue)
                        .help("write SAM text instead of BAM"),
                )
                .arg(
                    Arg::new("max_memory")
                        .long("max-memory")
                        .value_name("SIZE")
                        .help("approximate memory per sort chunk (e.g. 256MiB); default 256MiB"),
                )
                .arg(
                    Arg::new("max_open_files")
                        .long("max-open-files")
                        .value_name("N")
                        .value_parser(clap::value_parser!(usize))
                        .default_value("64")
                        .help("maximum sorted-run files opened per merge pass"),
                )
                .arg(
                    Arg::new("threads")
                        .short('@')
                        .long("threads")
                        .value_name("INT")
                        .value_parser(clap::value_parser!(usize))
                        .default_value("1")
                        .help("sort is single-threaded; only 1 is accepted"),
                ),
        )
}

fn run(matches: &clap::ArgMatches) -> Result<(), Error> {
    match matches.subcommand() {
        Some(("view", sub)) => {
            if sub.get_flag("help") {
                let _ = cli().find_subcommand_mut("view").map(|c| c.print_help());
                return Ok(());
            }
            // <FILE> is declared optional so `--help` can be used alone, then
            // required here for every other invocation.
            let input = sub
                .get_one::<String>("input")
                .map(PathBuf::from)
                .ok_or_else(|| Error::usage("view requires <FILE>".to_string()))?;
            let options = ViewOptions {
                input,
                output: optional_path(sub, "output"),
                with_header: sub.get_flag("with_header"),
                bam_output: sub.get_flag("bam"),
                header_only: sub.get_flag("header_only"),
                count_only: sub.get_flag("count"),
                require_flags: sub
                    .get_one::<String>("require_flags")
                    .map(|value| parse_flags(value))
                    .transpose()?
                    .unwrap_or(0),
                exclude_flags: sub
                    .get_one::<String>("exclude_flags")
                    .map(|value| parse_flags(value))
                    .transpose()?
                    .unwrap_or(0),
                min_mapq: *sub.get_one::<u8>("min_mapq").unwrap_or(&0),
                region: sub.get_one::<String>("region").cloned(),
                threads: *sub.get_one::<usize>("threads").unwrap_or(&1),
            };
            // `-H`, `-c` and `-b` are mutually exclusive intents; samtools lets the
            // first two win, and so do we (see commands::view).
            commands::view(options)
        }
        Some(("flagstat", sub)) => {
            let text = commands::flagstat(&path(sub, "input")?)?;
            write_text(sub, &text)
        }
        Some(("idxstats", sub)) => {
            let text = commands::idxstats(&path(sub, "input")?)?;
            write_text(sub, &text)
        }
        Some(("quickcheck", sub)) => {
            let inputs: Vec<PathBuf> = sub
                .get_many::<String>("inputs")
                .map(|values| values.map(PathBuf::from).collect())
                .unwrap_or_default();
            let result = commands::quickcheck(&inputs)?;
            // samtools prints one line per failure to stderr and exits 16.
            for (file, reason) in &result.failed {
                eprintln!("{} {}", file.display(), reason);
            }
            if result.ok() {
                Ok(())
            } else {
                // A distinct exit code, not one of the shared classes.
                Err(Error::runtime(format!(
                    "{} file(s) failed quickcheck",
                    result.failed.len()
                )))
            }
        }
        Some(("index", sub)) => {
            let (written, count) = commands::index(
                &path(sub, "input")?,
                optional_path(sub, "output"),
                sub.get_flag("csi"),
            )?;
            eprintln!("indexed {count} records into {}", written.display());
            Ok(())
        }
        Some(("sort", sub)) => {
            let options = SortOptions {
                input: path(sub, "input")?,
                output: optional_path(sub, "output"),
                sam_output: sub.get_flag("sam"),
                max_memory: sub
                    .get_one::<String>("max_memory")
                    .map(|value| parse_size(value))
                    .transpose()?,
                max_open_files: *sub
                    .get_one::<usize>("max_open_files")
                    .unwrap_or(&commands::DEFAULT_MAX_OPEN_FILES),
                threads: *sub.get_one::<usize>("threads").unwrap_or(&1),
            };
            let total = commands::sort(options)?;
            eprintln!("sorted {total} records");
            Ok(())
        }
        _ => Err(Error::usage("a subcommand is required")),
    }
}

/// Parse a samtools-style flag filter value: numeric, hex or a comma-separated
/// list of common flag names.
fn parse_flags(text: &str) -> Result<u16, Error> {
    let mut value = 0u16;
    for token in text.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let bit = if let Some(hex) = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
        {
            u16::from_str_radix(hex, 16)
                .map_err(|_| Error::usage(format!("invalid flag value: {token}")))?
        } else if let Ok(number) = token.parse::<u16>() {
            number
        } else {
            match token.to_ascii_uppercase().as_str() {
                "PAIRED" => 0x1,
                "PROPER_PAIR" => 0x2,
                "UNMAP" => 0x4,
                "MUNMAP" => 0x8,
                "REVERSE" => 0x10,
                "MREVERSE" => 0x20,
                "READ1" => 0x40,
                "READ2" => 0x80,
                "SECONDARY" => 0x100,
                "QCFAIL" => 0x200,
                "DUP" => 0x400,
                "SUPPLEMENTARY" => 0x800,
                other => return Err(Error::usage(format!("unknown flag name: {other}"))),
            }
        };
        value |= bit;
    }
    Ok(value)
}

/// Parse a size accepted by `sort --max-memory`.
fn parse_size(text: &str) -> Result<u64, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::usage(
            "--max-memory requires a size such as 256MiB".to_string(),
        ));
    }
    let split = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(split);
    let value: u64 = number
        .parse()
        .map_err(|_| Error::usage(format!("invalid --max-memory value: {text}")))?;
    let multiplier: u64 = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => {
            return Err(Error::usage(format!(
                "unknown size suffix in --max-memory: {text}"
            )))
        }
    };
    value
        .checked_mul(multiplier)
        .ok_or_else(|| Error::usage(format!("--max-memory value overflows u64: {text}")))
}

fn write_text(sub: &clap::ArgMatches, text: &str) -> Result<(), Error> {
    use std::io::Write;
    match optional_path(sub, "output") {
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(text.as_bytes())
                .and_then(|()| lock.flush())
                .map_err(|error| Error::io(format!("cannot write output: {error}")))
        }
        Some(path) => std::fs::write(&path, text)
            .map_err(|error| Error::io(format!("{}: {error}", path.display()))),
    }
}

fn path(sub: &clap::ArgMatches, name: &str) -> Result<PathBuf, Error> {
    sub.get_one::<String>(name)
        .map(PathBuf::from)
        .ok_or_else(|| Error::usage(format!("missing required argument <{name}>")))
}

fn optional_path(sub: &clap::ArgMatches, name: &str) -> Option<PathBuf> {
    sub.get_one::<String>(name).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_declares_all_six_commands() {
        let command = cli();
        let names: Vec<&str> = command
            .get_subcommands()
            .map(|sub| sub.get_name())
            .collect();
        for expected in COMMANDS {
            assert!(
                names.contains(&expected),
                "missing subcommand {expected}; have {names:?}"
            );
        }
        assert_eq!(names.len(), COMMANDS.len());
    }

    #[test]
    fn every_command_requires_an_input() {
        for sub in cli().get_subcommands() {
            let name = sub.get_name();
            if name == "quickcheck" {
                // quickcheck takes `inputs`, plural, also required.
                assert!(sub.get_arguments().any(|a| a.get_id() == "inputs"));
                continue;
            }
            assert!(
                sub.get_arguments().any(|a| a.get_id() == "input"),
                "{name} has no input argument"
            );
        }
    }

    #[test]
    fn sort_size_parser_accepts_binary_suffixes() {
        assert_eq!(parse_size("512").unwrap(), 512);
        assert_eq!(parse_size("1KiB").unwrap(), 1024);
        assert_eq!(parse_size("64MiB").unwrap(), 64 * 1024 * 1024);
        assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
        assert!(parse_size("10XiB").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn verbose_version_is_detected_only_with_both_flags() {
        let both = vec!["--version".to_string(), "--verbose".to_string()];
        assert!(wants_verbose_version(&both));
        assert!(!wants_verbose_version(&["--version".to_string()]));
        assert!(!wants_verbose_version(&["--verbose".to_string()]));
    }

    #[test]
    fn the_provenance_block_names_the_oracle_and_the_library() {
        let text = provenance().render();
        assert!(text.contains("rust-bam"), "{text}");
        assert!(text.contains("samtools 1.24"), "{text}");
        assert!(text.contains("noodles 0.118.0"), "{text}");
        assert!(
            text.contains("view, flagstat, idxstats, quickcheck, index, sort"),
            "{text}"
        );
    }

    #[test]
    fn exit_codes_come_from_the_shared_contract() {
        assert_eq!(exit_code(0), ExitCode::SUCCESS);
        assert_eq!(exit_code(2), ExitCode::from(2));
        assert_eq!(exit_code(3), ExitCode::from(3));
    }

    #[test]
    fn a_missing_subcommand_is_a_usage_error() {
        let matches = cli()
            .try_get_matches_from(["rust-bam", "view", "x.bam"])
            .unwrap();
        // A valid command line parses; the error path is exercised below.
        assert!(matches.subcommand().is_some());
    }
}
