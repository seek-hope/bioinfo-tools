//! `rust-fastqc` command-line entry point.
//!
//! Exit codes, diagnostics and the provenance block come from `bioinfo-common`,
//! matching every other binary in the workspace.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};

use rust_fastqc::absent;
use rust_fastqc::error::{Error, EXIT_CLASSES};
use rust_fastqc::stats::QcStats;
use rust_fastqc::{Report, ReportWriter, COMPUTED_MODULES, FASTQC_VERSION};

/// Program name in diagnostics and the provenance block.
const PROGRAM: &str = "rust-fastqc";
/// Crate version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if wants_verbose_version(&args) {
        print!("{}", provenance().render());
        return ExitCode::SUCCESS;
    }

    let matches = match cli().try_get_matches_from(std::iter::once(PROGRAM.to_string()).chain(args))
    {
        Ok(matches) => matches,
        Err(error) => {
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
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

fn wants_verbose_version(args: &[String]) -> bool {
    args.iter().any(|a| a == "--version") && args.iter().any(|a| a == "--verbose")
}

fn provenance() -> bioinfo_common::Provenance {
    bioinfo_common::Provenance::new(PROGRAM, VERSION)
        .with_field(
            "reference",
            format!("FastQC {FASTQC_VERSION} (fastqc_data.txt only; no HTML or plots)"),
        )
        .with_field("parser", "bioinfo-fastx (shared strict FASTX reader)")
        .with_field("modules", COMPUTED_MODULES.join(", "))
        .with_field("absent modules", absent::absent_module_names().join(", "))
        .with_exit_codes(&EXIT_CLASSES)
}

fn cli() -> Command {
    Command::new(PROGRAM)
        .version(VERSION)
        .about("FastQC-compatible QC: emits fastqc_data.txt")
        .arg_required_else_help(true)
        .arg(
            Arg::new("inputs")
                .value_name("FASTQ")
                .required(true)
                .num_args(1..)
                .help("FASTQ files to analyse"),
        )
        .arg(
            Arg::new("outdir")
                .short('o')
                .long("outdir")
                .value_name("DIR")
                .help("write <sample>_fastqc/fastqc_data.txt under DIR"),
        )
        .arg(
            Arg::new("stdout")
                .long("stdout")
                .action(ArgAction::SetTrue)
                .help("write to standard output instead of a file"),
        )
        .arg(
            Arg::new("phred64")
                .long("phred64")
                .action(ArgAction::SetTrue)
                .help("qualities are Phred+64 rather than Phred+33"),
        )
}

fn run(matches: &clap::ArgMatches) -> Result<(), Error> {
    let inputs: Vec<PathBuf> = matches
        .get_many::<String>("inputs")
        .map(|values| values.map(PathBuf::from).collect())
        .unwrap_or_default();
    if inputs.is_empty() {
        return Err(Error::usage("at least one FASTQ file is required"));
    }
    let offset = if matches.get_flag("phred64") { 64 } else { 33 };
    let to_stdout = matches.get_flag("stdout");
    let outdir = matches.get_one::<String>("outdir").map(PathBuf::from);

    if let Some(dir) = &outdir {
        if !dir.is_dir() {
            return Err(Error::usage(format!(
                "output directory does not exist: {}",
                dir.display()
            )));
        }
    }

    for input in &inputs {
        let stats = accumulate(input, offset)?;
        let report = Report::from_stats(&stats);
        if to_stdout {
            print!("{}", report.render());
        } else {
            let path = report_path(input, outdir.as_deref());
            ReportWriter::write(&report, &path)?;
            eprintln!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// Where a report goes: `<outdir|input-dir>/<sample>_fastqc/fastqc_data.txt`.
///
/// This mirrors FastQC's directory layout, which is also what MultiQC looks for.
pub fn report_path(input: &Path, outdir: Option<&Path>) -> PathBuf {
    let stem = sample_name(input);
    let base = match outdir {
        Some(dir) => dir.to_path_buf(),
        None => input.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    base.join(format!("{stem}_fastqc")).join("fastqc_data.txt")
}

/// The sample name FastQC uses: the file name with compression and FASTQ
/// extensions stripped.
pub fn sample_name(input: &Path) -> String {
    let name = input
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string_lossy().into_owned());
    for suffix in [".gz", ".bz2", ".xz", ".zst"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return strip_fastq_extension(stripped);
        }
    }
    strip_fastq_extension(&name)
}

fn strip_fastq_extension(name: &str) -> String {
    for suffix in [".fastq", ".fq", ".txt", ".seq"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    name.to_string()
}

/// Read `input` through the shared FASTX layer and accumulate statistics.
///
/// Streaming: one record is decoded at a time and folded into fixed-size counters,
/// so memory does not grow with the number of records. The per-read sequence map
/// used by the duplication module is the one exception, and it is bounded by the
/// number of *distinct* sequences.
fn accumulate(input: &Path, quality_offset: u8) -> Result<QcStats, Error> {
    use bioinfo_fastx::{Mode, Parser};

    let bytes =
        std::fs::read(input).map_err(|error| Error::io(format!("{}: {error}", input.display())))?;
    let mut parser = Parser::new(&bytes, 0, 1).with_mode(Mode::Strict);

    // `Filename` is the input's *base name including its extension* — FastQC
    // prints `sample.fastq`, not `sample`, and not the path. The bare stem is
    // used only for the output directory name.
    let filename = input
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string_lossy().into_owned());
    let mut stats = QcStats::new(filename);
    loop {
        match parser.next_record() {
            Ok(Some(record)) => {
                if record.qualities.is_empty() {
                    return Err(Error::input(format!(
                        "{}: FASTQ input required, but a record has no quality line",
                        input.display()
                    )));
                }
                stats.add_read(record.sequence, record.qualities, quality_offset);
            }
            Ok(None) => break,
            Err(error) => {
                return Err(Error::runtime(format!("{}: {error}", input.display())));
            }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_name_strips_compression_and_fastq_extensions() {
        assert_eq!(sample_name(Path::new("/x/sample.fastq")), "sample");
        assert_eq!(sample_name(Path::new("/x/sample.fq")), "sample");
        assert_eq!(sample_name(Path::new("/x/sample.fastq.gz")), "sample");
        assert_eq!(sample_name(Path::new("/x/sample.fq.bz2")), "sample");
        assert_eq!(sample_name(Path::new("/x/sample")), "sample");
    }

    #[test]
    fn report_path_mirrors_fastqc_layout() {
        assert_eq!(
            report_path(Path::new("/data/s.fastq"), None),
            PathBuf::from("/data/s_fastqc/fastqc_data.txt")
        );
        assert_eq!(
            report_path(Path::new("/data/s.fastq"), Some(Path::new("/out"))),
            PathBuf::from("/out/s_fastqc/fastqc_data.txt")
        );
    }

    #[test]
    fn the_cli_requires_at_least_one_input() {
        assert!(cli().try_get_matches_from(["rust-fastqc"]).is_err());
        assert!(cli()
            .try_get_matches_from(["rust-fastqc", "a.fq", "b.fq"])
            .is_ok());
    }

    #[test]
    fn verbose_version_needs_both_flags() {
        let both = vec!["--version".to_string(), "--verbose".to_string()];
        assert!(wants_verbose_version(&both));
        assert!(!wants_verbose_version(&["--version".to_string()]));
    }

    #[test]
    fn provenance_names_the_reference_and_the_absent_modules() {
        let text = provenance().render();
        assert!(text.contains("rust-fastqc"), "{text}");
        assert!(text.contains("FastQC 0.13.0"), "{text}");
        assert!(text.contains("Adapter Content"), "{text}");
    }

    #[test]
    fn accumulating_counts_reads_and_bases() {
        let dir = std::env::temp_dir().join(format!("rfq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.fq");
        std::fs::write(&path, b"@a\nACGT\n+\nIIII\n@b\nACGT\n+\nIIII\n").unwrap();
        let stats = accumulate(&path, 33).unwrap();
        assert_eq!(stats.total_sequences, 2);
        assert_eq!(stats.total_bases, 8);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_missing_input_is_an_io_error() {
        let error = accumulate(Path::new("/nonexistent/x.fq"), 33).unwrap_err();
        assert_eq!(error.exit_code(), 4);
    }
}
