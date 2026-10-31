//! `rust-fastqc` command-line entry point.
//!
//! Exit codes, diagnostics and the provenance block come from `bioinfo-common`,
//! matching every other binary in the workspace.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};

use rust_fastqc::absent;
use rust_fastqc::contaminants::Contaminant;
use rust_fastqc::error::{Error, EXIT_CLASSES};
use rust_fastqc::limits::{check_read, QcLimits};
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
            format!("FastQC {FASTQC_VERSION} (data, summary, HTML and stored ZIP; plot images are not emitted)"),
        )
        .with_field("parser", "bioinfo-fastx (shared strict FASTX reader)")
        .with_field("modules", COMPUTED_MODULES.join(", "))
        .with_field("absent modules", absent::absent_module_names().join(", "))
        .with_exit_codes(&EXIT_CLASSES)
}

fn cli() -> Command {
    Command::new(PROGRAM)
        .version(VERSION)
        .about("FastQC-compatible QC: emits fastqc_data.txt, summary, HTML and ZIP")
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
                .help("write <sample>_fastqc.html and .zip under DIR; --extract also writes the report directory"),
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
        .arg(
            Arg::new("max_read_length")
                .long("max-read-length")
                .value_name("BASES")
                .value_parser(clap::value_parser!(u64))
                .default_value("1000000")
                .help("reject a read longer than this before growing per-position statistics"),
        )
        .arg(
            Arg::new("max_distinct_sequences")
                .long("max-distinct-sequences")
                .value_name("N")
                .value_parser(clap::value_parser!(u64))
                .default_value("10000000")
                .help("reject a new distinct 50-base prefix beyond this many before inserting it"),
        )
        .arg(
            Arg::new("nogroup")
                .long("nogroup")
                .action(ArgAction::SetTrue)
                .help("disable grouping of base positions for long reads"),
        )
        .arg(
            Arg::new("min_length")
                .long("min_length")
                .value_name("BASES")
                .value_parser(clap::value_parser!(usize))
                .help("ignore reads shorter than this length"),
        )
        .arg(
            Arg::new("max_length")
                .long("max_length")
                .value_name("BASES")
                .value_parser(clap::value_parser!(usize))
                .help("ignore reads longer than this length"),
        )
        .arg(
            Arg::new("dup_length")
                .long("dup_length")
                .value_name("BASES")
                .value_parser(clap::value_parser!(usize))
                .default_value("50")
                .help("truncate sequences to this length for duplication statistics"),
        )
        .arg(
            Arg::new("extract")
                .long("extract")
                .action(ArgAction::SetTrue)
                .help("extract the report directory after writing the ZIP"),
        )
        .arg(
            Arg::new("delete")
                .long("delete")
                .action(ArgAction::SetTrue)
                .help("with --extract, delete the ZIP after extraction"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .help("suppress progress messages"),
        )
        .arg(
            Arg::new("java")
                .short('j')
                .long("java")
                .value_name("JAVA")
                .help("recognized FastQC launcher option; rejected because this executable is native Rust"),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .action(ArgAction::SetTrue)
                .help("recognized JVM-launcher debug option; rejected because no JVM is launched"),
        )
        .arg(
            Arg::new("adapters")
                .short('a')
                .long("adapters")
                .value_name("TSV")
                .help("adapter list in name<TAB>sequence format"),
        )
        .arg(
            Arg::new("contaminants")
                .short('c')
                .long("contaminants")
                .value_name("TSV")
                .help("contaminant list in name<TAB>sequence format"),
        )
}

fn run(matches: &clap::ArgMatches) -> Result<(), Error> {
    if matches.get_one::<String>("java").is_some() || matches.get_flag("debug") {
        return Err(Error::usage(
            "-j/--java and --debug configure the upstream JVM launcher; rust-fastqc is native Rust and does not launch Java; remove these options",
        ));
    }
    let inputs: Vec<PathBuf> = matches
        .get_many::<String>("inputs")
        .map(|values| values.map(PathBuf::from).collect())
        .unwrap_or_default();
    if inputs.is_empty() {
        return Err(Error::usage("at least one FASTQ file is required"));
    }
    let offset = if matches.get_flag("phred64") { 64 } else { 33 };
    let limits = QcLimits {
        max_read_length: *matches
            .get_one::<u64>("max_read_length")
            .expect("clap supplies a default"),
        max_distinct_sequences: *matches
            .get_one::<u64>("max_distinct_sequences")
            .expect("clap supplies a default"),
    };
    let to_stdout = matches.get_flag("stdout");
    let outdir = matches.get_one::<String>("outdir").map(PathBuf::from);
    let adapters = match matches.get_one::<String>("adapters") {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| Error::io(format!("{path}: {error}")))?;
            let (adapters, warnings) = rust_fastqc::adapters::parse(&text).map_err(Error::input)?;
            for warning in warnings {
                eprintln!("{warning}");
            }
            adapters
        }
        None => rust_fastqc::adapters::defaults(),
    };
    let contaminants = match matches.get_one::<String>("contaminants") {
        Some(path) => read_contaminants(Path::new(path))?,
        None => rust_fastqc::contaminants::default_contaminants()
            .map_err(|error| Error::runtime(format!("built-in contaminant list: {error}")))?,
    };

    if let Some(dir) = &outdir {
        if !dir.is_dir() {
            return Err(Error::usage(format!(
                "output directory does not exist: {}",
                dir.display()
            )));
        }
    }

    for input in &inputs {
        let stats = accumulate_with_options(
            input,
            AccumulateOptions {
                quality_offset: offset,
                limits,
                contaminants: &contaminants,
                adapters: &adapters,
                min_length: matches.get_one::<usize>("min_length").copied(),
                max_length: matches.get_one::<usize>("max_length").copied(),
                dup_length: *matches
                    .get_one::<usize>("dup_length")
                    .expect("clap supplies a default"),
                nogroup: matches.get_flag("nogroup"),
            },
        )?;
        let report = Report::from_stats(&stats);
        if to_stdout {
            print!("{}", report.render());
        } else {
            let path = report_path(input, outdir.as_deref());
            let filename = input
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| input.to_string_lossy().into_owned());
            ReportWriter::write_bundle(
                &report,
                &path,
                &filename,
                matches.get_flag("extract"),
                matches.get_flag("delete"),
            )?;
            if !matches.get_flag("quiet") {
                eprintln!("wrote {}", path.display());
            }
        }
    }
    Ok(())
}

fn read_contaminants(path: &Path) -> Result<Vec<Contaminant>, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::io(format!("{}: {error}", path.display())))?;
    rust_fastqc::contaminants::parse(&text)
        .map_err(|error| Error::input(format!("{}: {error}", path.display())))
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
/// Input is decompressed by content and parsed in record-aligned chunks, not
/// loaded in full. Input buffers scale with the chunk size and longest record.
/// This is not a bound on total QC memory: per-position counters grow with the
/// longest sequence, and duplication counts grow with distinct sequence prefixes.
#[derive(Clone, Copy)]
struct AccumulateOptions<'a> {
    quality_offset: u8,
    limits: QcLimits,
    contaminants: &'a [Contaminant],
    adapters: &'a [rust_fastqc::adapters::Adapter],
    min_length: Option<usize>,
    max_length: Option<usize>,
    dup_length: usize,
    nogroup: bool,
}

#[cfg(test)]
fn accumulate(
    input: &Path,
    quality_offset: u8,
    limits: QcLimits,
    contaminants: &[Contaminant],
) -> Result<QcStats, Error> {
    accumulate_with_options(
        input,
        AccumulateOptions {
            quality_offset,
            limits,
            contaminants,
            adapters: &rust_fastqc::adapters::defaults(),
            min_length: None,
            max_length: None,
            dup_length: 50,
            nogroup: false,
        },
    )
}

fn accumulate_with_options(input: &Path, options: AccumulateOptions) -> Result<QcStats, Error> {
    use bioinfo_fastx::{Mode, Parser, Reader, DEFAULT_CHUNK_SIZE};

    let input_error = |error: std::io::Error| {
        Error::from(std::io::Error::new(
            error.kind(),
            format!("{}: {error}", input.display()),
        ))
    };
    // Keep the Path intact (including non-UTF8 names); Reader::open takes &str.
    let file = std::fs::File::open(input).map_err(input_error)?;
    let mut reader =
        Reader::from_stream(Box::new(file), input.display().to_string()).map_err(input_error)?;

    // `Filename` is the input's *base name including its extension* — FastQC
    // prints `sample.fastq`, not `sample`, and not the path. The bare stem is
    // used only for the output directory name.
    let filename = input
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string_lossy().into_owned());
    let mut stats = QcStats::new(filename);
    stats.set_contaminants(options.contaminants.to_vec());
    stats.set_adapters(options.adapters.to_vec());
    stats.set_dup_length(options.dup_length);
    stats.set_nogroup(options.nogroup);
    let mut buffer = Vec::new();
    while let Some(chunk) = reader
        .next_chunk(&mut buffer, DEFAULT_CHUNK_SIZE)
        .map_err(input_error)?
    {
        let mut parser = Parser::new(chunk.data, chunk.offset, chunk.line).with_mode(Mode::Strict);
        loop {
            match parser.next_record() {
                Ok(Some(record)) => {
                    if record.qualities.is_empty() {
                        return Err(Error::input(format!(
                            "{}: FASTQ input required, but a record has no quality line",
                            input.display()
                        )));
                    }
                    if options
                        .min_length
                        .is_some_and(|minimum| record.sequence.len() < minimum)
                    {
                        continue;
                    }
                    if options
                        .max_length
                        .is_some_and(|maximum| record.sequence.len() > maximum)
                    {
                        continue;
                    }
                    if options.quality_offset == 64
                        && record.qualities.iter().any(|byte| *byte < 64)
                    {
                        return Err(Error::runtime(format!(
                            "{}: Phred64 quality byte is below ASCII 64",
                            input.display()
                        )));
                    }
                    let prefix: Vec<u8> = record
                        .sequence
                        .iter()
                        .take(options.dup_length)
                        .map(|byte| byte.to_ascii_uppercase())
                        .collect();
                    check_read(
                        record.sequence.len(),
                        !stats.sequences.contains_key(&prefix),
                        stats.sequences.len(),
                        options.limits,
                    )
                    .map_err(|message| Error::input(format!("{}: {message}", input.display())))?;
                    if let Some(warning) = stats.tiles.add_read(
                        record.header,
                        record.qualities,
                        options.quality_offset,
                    ) {
                        eprintln!("{warning}");
                    }
                    stats.add_read(record.sequence, record.qualities, options.quality_offset);
                }
                Ok(None) => break,
                Err(error) => {
                    return Err(Error::runtime(format!("{}: {error}", input.display())));
                }
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
        assert!(text.contains("Kmer Content"), "{text}");
    }

    #[test]
    fn accumulating_counts_reads_and_bases() {
        let dir = std::env::temp_dir().join(format!("rfq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.fq");
        std::fs::write(&path, b"@a\nACGT\n+\nIIII\n@b\nACGT\n+\nIIII\n").unwrap();
        let stats = accumulate(
            &path,
            33,
            QcLimits {
                max_read_length: 1_000_000,
                max_distinct_sequences: 10_000_000,
            },
            &[],
        )
        .unwrap();
        assert_eq!(stats.total_sequences, 2);
        assert_eq!(stats.total_bases, 8);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_missing_input_is_an_io_error() {
        let error = accumulate(
            Path::new("/nonexistent/x.fq"),
            33,
            QcLimits {
                max_read_length: 1_000_000,
                max_distinct_sequences: 10_000_000,
            },
            &[],
        )
        .unwrap_err();
        assert_eq!(error.exit_code(), 4);
    }
}
