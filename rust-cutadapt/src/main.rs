//! rust-cutadapt command-line entry point.

use std::process::ExitCode;

use rust_cutadapt::cli::{self, Cli, Parsed, ReportKind, USAGE_EXIT_CODE};
use rust_cutadapt::pipeline::{self, Config, RunError};
use rust_cutadapt::report;
use rust_cutadapt::runreport;

/// Exit code used for runtime failures (I/O errors, malformed input).
const FAILURE_EXIT_CODE: u8 = 1;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(raw.clone()) {
        Ok(Parsed::Help(text)) | Ok(Parsed::Version(text)) => {
            // --help/--version are successful requests: stdout, exit 0.
            print!("{text}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(cli)) => {
            // cutadapt loads adapter lists while parsing arguments: the banner
            // and the "Command line parameters" line are printed, then the error,
            // and the "Processing ..." line never appears.
            let show_banner = !cli.quiet && cli.report != ReportKind::Minimal;
            if show_banner {
                print_banner(&raw);
            }
            if let Some(message) = &cli.adapter_file_error {
                println!("ERROR: {message}");
                return ExitCode::from(FAILURE_EXIT_CODE);
            }
            if show_banner {
                print_processing(&cli);
            }
            match run(&cli, &raw) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    // cutadapt emits its diagnostics on stdout once processing
                    // has begun (the banner above is already on stdout), so the
                    // error follows it there.
                    let cleaned = message
                        .strip_prefix("error: ")
                        .unwrap_or(&message)
                        .to_string();
                    // The --max-aer empty-read abort is a Python traceback on
                    // cutadapt's STDERR; its own ERROR lines go to stdout.
                    if cleaned.contains("--max-average-error-rate") {
                        eprintln!("ERROR: {cleaned}");
                    } else {
                        println!("ERROR: {cleaned}");
                    }
                    ExitCode::from(FAILURE_EXIT_CODE)
                }
            }
        }
        Err(error) => {
            // cutadapt's own option-validation errors ("ERROR: When trimming
            // paired-end data, ...") go to STDOUT, unlike clap's usage
            // diagnostics; both exit 2.
            if error.0.starts_with("ERROR:") {
                println!("{}", error.0);
            } else {
                eprintln!("{error}");
            }
            ExitCode::from(USAGE_EXIT_CODE as u8)
        }
    }
}

/// True when `-o` (or the paired `-p`) contains `{name}`, which switches on
/// cutadapt's demultiplexing mode: one output file per adapter plus an
/// "unknown" file for reads without an adapter.
fn output_template_has_name(cli: &Cli) -> bool {
    // `-o -` writes everything to stdout; a `{name}` pattern there is ignored.
    let uses_template = |path: &Option<std::path::PathBuf>| {
        path.as_ref()
            .map(|p| p.as_os_str() != "-" && p.to_string_lossy().contains("{name}"))
            .unwrap_or(false)
    };
    let writes_to_stdout = cli
        .output
        .as_ref()
        .map(|p| p.as_os_str() == "-")
        .unwrap_or(false);
    !writes_to_stdout && (uses_template(&cli.output) || uses_template(&cli.paired_output))
}

fn run(cli: &Cli, raw_args: &[String]) -> Result<(), String> {
    // An adapter list that could not be loaded is a runtime failure (exit 1),
    // matching cutadapt.
    if let Some(message) = &cli.adapter_file_error {
        return Err(format!("error: {message}"));
    }
    let mut config = Config::new(
        cli.input.clone(),
        cli.output.clone().unwrap_or_else(|| "-".into()),
        Vec::new(),
    );
    config.adapters = cli.adapters.clone();
    config.match_config = cli.match_config;
    config.match_config.match_read_wildcards = cli.match_read_wildcards;
    config.match_config.no_adapter_wildcards = cli.no_match_adapter_wildcards;
    config.times = cli.times;
    config.json = cli.json.clone();
    // -Z selects gzip compression level 1 (fastest); the default is level 3.
    config.compression_level = if cli.compression_level_1 {
        1
    } else {
        pipeline::configured_compression_level()
    };
    config.reverse_complement = cli.reverse_complement;
    config.renaming = rust_cutadapt::modify::Renaming {
        prefix: cli.prefix.clone(),
        suffix: cli.suffix.clone(),
        rename: cli.rename.clone(),
        strip_suffixes: cli.strip_suffixes.clone(),
        length_tag: cli.length_tag.clone(),
    };
    config.aux = rust_cutadapt::auxout::AuxOutputs {
        info_file: cli.info_file.clone(),
        rest_file: cli.rest_file.clone(),
        wildcard_file: cli.wildcard_file.clone(),
        too_short_output: cli.too_short_output.clone(),
        too_long_output: cli.too_long_output.clone(),
        untrimmed_output: cli.untrimmed_output.clone(),
        fasta: cli.fasta,
        demultiplex: output_template_has_name(cli),
    };
    config.action = match cli.action {
        cli::Action::Trim => rust_cutadapt::modify::Action::Trim,
        cli::Action::Retain => rust_cutadapt::modify::Action::Retain,
        cli::Action::Mask => rust_cutadapt::modify::Action::Mask,
        cli::Action::Lowercase => rust_cutadapt::modify::Action::Lowercase,
        cli::Action::None => rust_cutadapt::modify::Action::None,
    };
    config.pair_adapters = cli.pair_adapters;
    config.filters = rust_cutadapt::filter::Filters {
        minimum_length: cli.minimum_length_given.then_some(cli.minimum_length),
        maximum_length: cli.maximum_length,
        max_n: cli.max_n,
        max_expected_errors: cli.max_expected_errors,
        max_average_error_rate: cli.max_average_error_rate,
        discard_trimmed: cli.discard_trimmed,
        discard_untrimmed: cli.discard_untrimmed,
        discard_casava: cli.discard_casava,
        quality_base: cli.quality_base,
    };
    config.modifications = rust_cutadapt::modify::Modifications {
        cut_5prime: cli.cut_5prime,
        cut_3prime: cli.cut_3prime,
        quality_5prime: cli.quality_cutoff_5prime,
        quality_3prime: cli.quality_cutoff_3prime,
        quality_base: cli.quality_base,
        nextseq_trim: cli.nextseq_trim,
        poly_a: cli.poly_a,
        length: cli.length,
        trim_n: cli.trim_n,
        zero_cap: cli.zero_cap,
    };
    config.minimum_length = cli.minimum_length;
    // `-m LEN:LEN2` gives each mate its own minimum; LEN2 defaults to LEN.
    config.minimum_length_r2 = cli.minimum_length_r2;
    config.maximum_length_r2 = cli.maximum_length_r2;
    config.threads = configure_threads(cli.cores);
    // --- paired-end wiring -------------------------------------------------
    config.input2 = cli.input2.clone();
    config.interleaved = cli.interleaved;
    config.paired_output = cli.paired_output.clone();
    config.adapters2 = cli.adapters2.clone();
    config.pair_filter = match cli.pair_filter {
        cli::PairFilter::Any => rust_cutadapt::pair::PairFilter::Any,
        cli::PairFilter::Both => rust_cutadapt::pair::PairFilter::Both,
        cli::PairFilter::First => rust_cutadapt::pair::PairFilter::First,
    };
    config.modifications2 = Some(rust_cutadapt::modify::Modifications {
        cut_5prime: cli.cut_5prime_r2,
        cut_3prime: cli.cut_3prime_r2,
        ..config.modifications.clone()
    });
    // R2 demultiplexing is driven by the -p template.
    let r2_demux = cli
        .paired_output
        .as_ref()
        .map(|p| p.to_string_lossy().contains("{name}"))
        .unwrap_or(false);
    config.aux2 = rust_cutadapt::auxout::AuxOutputs {
        demultiplex: r2_demux,
        too_short_output: cli.too_short_paired_output.clone(),
        too_long_output: cli.too_long_paired_output.clone(),
        untrimmed_output: cli.untrimmed_paired_output.clone(),
        ..config.aux.clone()
    };

    let started = std::time::Instant::now();
    let stats = if pipeline::is_paired(&config) {
        pipeline::run_paired(&config).map_err(|e| describe(&e))?
    } else {
        pipeline::run(&config).map_err(|e| describe(&e))?
    };
    if let Some(path) = &cli.json {
        write_json_report(path, cli, raw_args, &stats)?;
    }
    let elapsed = started.elapsed().as_secs_f64();
    if !cli.quiet {
        print_cutadapt_report(cli, &config.match_config, raw_args, &stats, elapsed);
    }
    Ok(())
}

/// Number of worker threads: `-j 0` means auto-detect; the default is four
/// (cutadapt defaults to one, so pass `-j 1` for exact single-thread parity).
pub fn configure_threads(cores: usize) -> usize {
    if cores == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        cores
    }
}

fn describe(error: &RunError) -> String {
    // A fatal condition can arrive wrapped in `Partial`; look through it so the
    // diagnostic is classified correctly.
    let inner = match error {
        RunError::Partial { source, .. } => source.as_ref(),
        other => other,
    };
    let detail = match inner {
        RunError::Parse(e) if e.message.contains(rust_cutadapt::engine::FATAL_MARKER) => {
            // A fatal condition cutadapt also aborts on; report the message
            // without the "malformed FASTQ" framing.
            format!(
                "error: {}",
                e.message
                    .split(rust_cutadapt::engine::FATAL_MARKER)
                    .last()
                    .unwrap_or(&e.message)
                    .trim()
            )
        }
        RunError::Parse(e) => format!("error: {e}\nThe input is not valid FASTQ."),
        RunError::Io(e) => format!("error: {e}"),
        other => format!("error: {other}"),
    };
    // cutadapt prints only the diagnostic. The one case where extra honesty is
    // required is a PARTIAL output file: that is a real hazard a user must know
    // about, and cutadapt does not report it either, so it is called out here.
    let records = error.records_written();
    if records == 0 {
        detail
    } else {
        format!(
            "{detail}\nNote: the output file is incomplete: {records} record(s) had already \
             been written before the error."
        )
    }
}

/// Print cutadapt 4.6's report: banner, preamble, then the summary block.
///
/// The banner and the `Command line parameters` / `Processing ...` lines are
/// part of cutadapt's observable output on stdout, so they are reproduced here.
fn print_cutadapt_report(
    cli: &Cli,
    match_config: &rust_cutadapt::adapter::matcher::MatchConfig,
    raw_args: &[String],
    run: &pipeline::Stats,
    elapsed: f64,
) {
    let stats = rust_cutadapt::runreport::build_stats(
        run,
        &cli.adapters,
        &cli.adapters2,
        match_config,
        cli.minimum_length_given.then_some(cli.minimum_length),
        &rust_cutadapt::runreport::FiltersGiven {
            maximum_length: cli.maximum_length.is_some(),
            max_n: cli.max_n.is_some(),
            max_expected_errors: cli.max_expected_errors.is_some(),
            max_average_error_rate: cli.max_average_error_rate.is_some(),
            discard_casava: cli.discard_casava,
            discard_trimmed: cli.discard_trimmed,
            discard_untrimmed: cli.discard_untrimmed,
            reverse_complement: cli.reverse_complement,
            untrimmed_output: cli.untrimmed_output.is_some(),
            demultiplex: output_template_has_name(cli),
        },
        &rust_cutadapt::runreport::ModifiersGiven {
            quality: cli.quality_cutoff_5prime.is_some()
                || cli.quality_cutoff_3prime.is_some()
                || cli.nextseq_trim.is_some(),
            poly_a: cli.poly_a,
        },
    );
    // --report minimal prints only the TSV table: no banner, no summary.
    if cli.report == ReportKind::Minimal {
        // Only the table: cutadapt prints no banner and no timing here.
        let table = report::text::minimal_report(&stats);
        if output_is_stdout(cli) {
            eprint!("{table}");
        } else {
            print!("{table}");
        }
        return;
    }
    let to_stderr = output_is_stdout(cli);
    // The banner and the "Processing ..." line were printed by `print_startup`
    // before the run, so only the "Finished in ..." line is added here.
    let _ = (match_config, raw_args);
    let mut buf = String::new();
    render_finished(elapsed, &stats, &mut buf);
    if stats.n == 0 {
        // cutadapt prints the preamble, then "No reads processed!".
        buf.push_str("No reads processed!\n");
        emit(&buf, to_stderr);
        return;
    }
    let body = match cli.report {
        ReportKind::Full => report::text::full_report(&stats),
        ReportKind::Minimal => report::text::minimal_report(&stats),
    };
    buf.push_str(&body);
    if !buf.ends_with('\n') {
        buf.push('\n');
    }
    emit(&buf, to_stderr);
}

/// Write cutadapt's `--json` report.
fn write_json_report(
    path: &std::path::Path,
    cli: &Cli,
    raw_args: &[String],
    run: &pipeline::Stats,
) -> Result<(), String> {
    let mut effective_config = cli.match_config;
    effective_config.match_read_wildcards = cli.match_read_wildcards;
    effective_config.no_adapter_wildcards = cli.no_match_adapter_wildcards;
    let stats = runreport::build_stats(
        run,
        &cli.adapters,
        &cli.adapters2,
        &effective_config,
        cli.minimum_length_given.then_some(cli.minimum_length),
        &runreport::FiltersGiven {
            maximum_length: cli.maximum_length.is_some(),
            max_n: cli.max_n.is_some(),
            max_expected_errors: cli.max_expected_errors.is_some(),
            max_average_error_rate: cli.max_average_error_rate.is_some(),
            discard_casava: cli.discard_casava,
            discard_trimmed: cli.discard_trimmed,
            discard_untrimmed: cli.discard_untrimmed,
            reverse_complement: cli.reverse_complement,
            untrimmed_output: cli.untrimmed_output.is_some(),
            demultiplex: output_template_has_name(cli),
        },
        &runreport::ModifiersGiven {
            quality: cli.quality_cutoff_5prime.is_some()
                || cli.quality_cutoff_3prime.is_some()
                || cli.nextseq_trim.is_some(),
            poly_a: cli.poly_a,
        },
    );
    let mut arguments = vec!["rust-cutadapt".to_string()];
    arguments.extend(raw_args.iter().cloned());
    let meta = report::json::Meta::new(cli.input.to_string_lossy().into_owned(), arguments);
    let body = report::json::to_json(&stats, &meta);
    std::fs::write(path, body)
        .map_err(|e| format!("error: cannot write JSON report {path:?}: {e}"))?;
    Ok(())
}

/// Print the banner cutadapt emits before processing starts.
fn print_banner(raw_args: &[String]) {
    println!(
        "This is cutadapt {} with Python 3",
        env!("CARGO_PKG_VERSION")
    );
    let args = if raw_args.is_empty() {
        String::new()
    } else {
        format!(" {}", raw_args.join(" "))
    };
    println!("Command line parameters:{args}");
}

/// The "Processing ... reads on N core ..." line, printed once adapter lists
/// have been validated (cutadapt's ordering).
fn print_processing(cli: &Cli) {
    let kind = if cli.input2.is_some() || cli.interleaved {
        "paired-end"
    } else {
        "single-end"
    };
    let cores = configure_threads(cli.cores);
    println!(
        "Processing {kind} reads on {cores} core{} ...",
        if cores == 1 { "" } else { "s" }
    );
    // `--debug` prints cutadapt's pipeline diagnostics. The Python-specific
    // lines (interpreter path, dnaio/xopen versions) have no Rust equivalent and
    // are reported honestly as such; the adapter and step lines do have one.
    //
    // The word `liblzma` is part of the observed output and stays, even though
    // the crate that binds it is now `xz2` (which links the same liblzma
    // library through `lzma-sys`). Renaming it would change a stdout byte; see
    // docs/EVIDENCE-W1-1.md DR-2.
    if cli.debug > 0 {
        println!(
            "DEBUG: rust-cutadapt {} with std + clap + flate2 + bzip2 + liblzma",
            env!("CARGO_PKG_VERSION")
        );
        println!("DEBUG: Adapters ({}):", cli.adapters.len());
        for spec in &cli.adapters {
            println!(
                "DEBUG: - <Adapter(name='{}', sequence='{}', type={}, max_error_rate={:?}, \
                 min_overlap={}>",
                spec.name,
                spec.display_sequence(),
                spec.adapter_type.report_name(),
                cli.match_config.error_rate,
                cli.match_config.min_overlap
            );
        }
    }
}

/// True when the trimmed reads are written to stdout (no `-o`, or `-o -`).
fn output_is_stdout(cli: &Cli) -> bool {
    cli.output
        .as_ref()
        .map(|p| p.as_os_str() == "-")
        .unwrap_or(true)
}

fn emit(text: &str, to_stderr: bool) {
    if to_stderr {
        eprint!("{text}");
    } else {
        print!("{text}");
    }
}

/// Render just the "Finished in ..." line and the blank line after it.
fn render_finished(elapsed: f64, stats: &report::stats::Stats, out: &mut String) {
    let n = stats.n.max(1) as f64;
    let per_read = elapsed / n;
    out.push_str(&format!(
        "Finished in {:.3} s ({:.3} \u{00b5}s/read; {:.2} M reads/minute).\n",
        elapsed,
        per_read * 1e6,
        n / elapsed.max(1e-9) * 60.0 / 1e6
    ));
    out.push('\n');
}
