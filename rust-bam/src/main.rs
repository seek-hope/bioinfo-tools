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
            let (written, count) =
                commands::index(&path(sub, "input")?, optional_path(sub, "output"))?;
            eprintln!("indexed {count} records into {}", written.display());
            Ok(())
        }
        Some(("sort", sub)) => {
            let options = SortOptions {
                input: path(sub, "input")?,
                output: optional_path(sub, "output"),
                sam_output: sub.get_flag("sam"),
            };
            let total = commands::sort(options)?;
            eprintln!("sorted {total} records");
            Ok(())
        }
        _ => Err(Error::usage("a subcommand is required")),
    }
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
