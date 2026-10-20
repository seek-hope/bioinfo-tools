//! Command-line interface for the full cutadapt 4.6 surface.
//!
//! Every documented option is accepted here. Options that are parsed but whose
//! *behaviour* is not implemented yet are still represented in [`Cli`], so the
//! parser is complete and only the runtime needs to catch up; a run that uses
//! such an option must fail loudly (never silently ignore it).
//!
//! Exit codes follow cutadapt: `0` success (including `--help`/`--version`),
//! `2` usage error, `1` runtime error (reported by `main`).

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Arg, ArgAction, Command};

use crate::adapter::matcher::{ErrorRate, MatchConfig};
use crate::adapter::{self, AdapterSpec};

/// Exit code used for usage errors (matches cutadapt / click).
pub const USAGE_EXIT_CODE: i32 = 2;

/// Default worker count when `-j` is omitted.
///
/// cutadapt defaults to one core; rust-cutadapt keeps four workers so the
/// documented benchmark command stays parallel. `-j 1` still requests exact
/// cutadapt-style single-threaded execution.
pub const DEFAULT_CORES: usize = 4;

/// Which report cutadapt prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReportKind {
    #[default]
    Full,
    Minimal,
}

/// What to do with a read once an adapter matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Action {
    #[default]
    Trim,
    Retain,
    Mask,
    Lowercase,
    None,
}

/// `--pair-filter` mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PairFilter {
    #[default]
    Any,
    Both,
    First,
}

/// Fully parsed command line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cli {
    // --- adapters ---------------------------------------------------------
    /// Adapters for read 1 (`-a`/`-g`/`-b`).
    pub adapters: Vec<AdapterSpec>,
    /// Adapters for read 2 (`-A`/`-G`/`-B`). cutadapt numbers these after the
    /// read-1 adapters, which `AdapterSpec::index` already reflects.
    pub adapters2: Vec<AdapterSpec>,
    pub match_config: MatchConfig,
    pub times: usize,
    pub action: Action,
    pub reverse_complement: bool,
    pub match_read_wildcards: bool,
    pub no_match_adapter_wildcards: bool,
    pub pair_adapters: bool,

    // --- read modifications ----------------------------------------------
    pub cut_5prime: usize,
    pub cut_3prime: usize,
    pub cut_5prime_r2: usize,
    pub cut_3prime_r2: usize,
    pub quality_cutoff_5prime: Option<u8>,
    pub quality_cutoff_3prime: Option<u8>,
    pub quality_cutoff_5prime_r2: Option<u8>,
    pub quality_cutoff_3prime_r2: Option<u8>,
    pub nextseq_trim: Option<u8>,
    pub quality_base: u8,
    pub poly_a: bool,
    pub length: Option<i64>,
    pub trim_n: bool,
    pub length_tag: Option<String>,
    pub strip_suffixes: Vec<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub rename: Option<String>,
    pub zero_cap: bool,

    // --- filters ----------------------------------------------------------
    pub minimum_length: usize,
    pub minimum_length_r2: Option<usize>,
    /// Whether `-m` appeared on the command line (cutadapt prints the
    /// too-short counter whenever it did, even for `-m 0`).
    pub minimum_length_given: bool,
    pub maximum_length: Option<usize>,
    pub maximum_length_r2: Option<usize>,
    pub max_n: Option<f64>,
    pub max_expected_errors: Option<f64>,
    pub max_average_error_rate: Option<f64>,
    pub discard_trimmed: bool,
    pub discard_untrimmed: bool,
    pub discard_casava: bool,
    pub pair_filter: PairFilter,

    // --- output -----------------------------------------------------------
    pub output: Option<PathBuf>,
    pub paired_output: Option<PathBuf>,
    pub interleaved: bool,
    pub fasta: bool,
    pub compression_level_1: bool,
    pub info_file: Option<PathBuf>,
    pub rest_file: Option<PathBuf>,
    pub wildcard_file: Option<PathBuf>,
    pub too_short_output: Option<PathBuf>,
    pub too_long_output: Option<PathBuf>,
    pub untrimmed_output: Option<PathBuf>,
    pub too_short_paired_output: Option<PathBuf>,
    pub too_long_paired_output: Option<PathBuf>,
    pub untrimmed_paired_output: Option<PathBuf>,
    pub report: ReportKind,
    pub json: Option<PathBuf>,
    pub quiet: bool,
    pub debug: u8,

    // --- runtime ----------------------------------------------------------
    pub cores: usize,
    pub input: PathBuf,
    pub input2: Option<PathBuf>,
    /// Set when an adapter list could not be loaded; reported at run time with
    /// exit 1, matching cutadapt.
    pub adapter_file_error: Option<String>,
}

/// Outcome of command-line parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    Run(Box<Cli>),
    Help(String),
    Version(String),
}

/// A usage error with a message suitable for stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UsageError {}

fn value_arg(
    name: &'static str,
    long: &'static str,
    short: Option<char>,
    help: &'static str,
) -> Arg {
    // cutadapt allows a single-value option to repeat, with the last occurrence
    // winning (oracle: `-m 0 -m 30` behaves like `-m 30`). clap rejects repeats
    // by default, so each option overrides its earlier occurrence instead.
    let arg = Arg::new(name)
        .long(long)
        .value_name(name)
        .overrides_with(name)
        .help(help);
    match short {
        Some(c) => arg.short(c),
        None => arg,
    }
}

fn flag_arg(
    name: &'static str,
    long: &'static str,
    short: Option<char>,
    help: &'static str,
) -> Arg {
    let arg = Arg::new(name)
        .long(long)
        .action(ArgAction::SetTrue)
        .help(help);
    match short {
        Some(c) => arg.short(c),
        None => arg,
    }
}

/// The clap command definition mirroring `cutadapt --help` (4.6).
pub fn command() -> Command {
    Command::new("rust-cutadapt")
        .bin_name("rust-cutadapt")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Trim adapters from FASTQ reads (cutadapt 4.6 compatible)")
        .arg(flag_arg("debug", "debug", None, "Print debug log").action(ArgAction::Count))
        .arg(value_arg(
            "cores",
            "cores",
            Some('j'),
            "Number of CPU cores to use (0 = auto; default 4)",
        ))
        // --- finding adapters
        .arg(
            value_arg(
                "adapter",
                "adapter",
                Some('a'),
                "3' adapter; '$' anchors to the end",
            )
            .action(ArgAction::Append),
        )
        .arg(
            value_arg(
                "front",
                "front",
                Some('g'),
                "5' adapter; '^' anchors to the start",
            )
            .action(ArgAction::Append),
        )
        .arg(
            value_arg("anywhere", "anywhere", Some('b'), "5'/3' adapter").action(ArgAction::Append),
        )
        .arg(
            value_arg("adapter2", "adapter2", Some('A'), "3' adapter for R2")
                .action(ArgAction::Append),
        )
        .arg(
            value_arg("front2", "front2", Some('G'), "5' adapter for R2").action(ArgAction::Append),
        )
        .arg(
            value_arg("anywhere2", "anywhere2", Some('B'), "5'/3' adapter for R2")
                .action(ArgAction::Append),
        )
        .arg(
            value_arg(
                "error_rate",
                "error-rate",
                Some('e'),
                "Maximum error rate or absolute count",
            )
            .allow_hyphen_values(true)
            .visible_alias("errors"),
        )
        .arg(flag_arg(
            "no_indels",
            "no-indels",
            None,
            "Only mismatches, no indels",
        ))
        .arg(value_arg(
            "times",
            "times",
            Some('n'),
            "Remove up to COUNT adapters per read",
        ))
        .arg(value_arg(
            "overlap",
            "overlap",
            Some('O'),
            "Minimum overlap",
        ))
        .arg(flag_arg(
            "match_read_wildcards",
            "match-read-wildcards",
            None,
            "IUPAC wildcards in reads",
        ))
        .arg(flag_arg(
            "no_match_adapter_wildcards",
            "no-match-adapter-wildcards",
            Some('N'),
            "Do not interpret IUPAC wildcards in adapters",
        ))
        .arg(value_arg(
            "action",
            "action",
            None,
            "trim|retain|mask|lowercase|none",
        ))
        .arg(
            flag_arg(
                "revcomp",
                "revcomp",
                None,
                "Check the reverse complement too",
            )
            .visible_alias("rc"),
        )
        // --- read modifications
        .arg(
            // -u is genuinely repeatable (accumulating), so it keeps Append and
            // must NOT inherit the last-wins override.
            Arg::new("cut")
                .long("cut")
                .short('u')
                .value_name("cut")
                .allow_hyphen_values(true)
                .action(ArgAction::Append)
                .help("Remove LEN bases (signed)"),
        )
        .arg(
            Arg::new("cut2")
                .long("cut2")
                .short('U')
                .value_name("cut2")
                .allow_hyphen_values(true)
                .action(ArgAction::Append)
                .help("Remove LEN bases from R2"),
        )
        .arg(value_arg(
            "nextseq_trim",
            "nextseq-trim",
            None,
            "NextSeq quality trimming",
        ))
        .arg(value_arg(
            "quality_cutoff",
            "quality-cutoff",
            Some('q'),
            "[5'CUTOFF,]3'CUTOFF",
        ))
        .arg(value_arg(
            "quality_cutoff2",
            "quality-cutoff2",
            Some('Q'),
            "Quality cutoff for R2",
        ))
        .arg(value_arg(
            "quality_base",
            "quality-base",
            None,
            "Quality base (default 33)",
        ))
        .arg(flag_arg("poly_a", "poly-a", None, "Trim poly-A tails"))
        .arg(
            value_arg("length", "length", Some('l'), "Shorten reads to LENGTH")
                .allow_hyphen_values(true),
        )
        .arg(flag_arg("trim_n", "trim-n", None, "Trim Ns at both ends"))
        .arg(value_arg(
            "length_tag",
            "length-tag",
            None,
            "Correct a length in the read name",
        ))
        .arg(
            value_arg(
                "strip_suffix",
                "strip-suffix",
                None,
                "Remove suffix from read names",
            )
            .action(ArgAction::Append),
        )
        .arg(value_arg(
            "prefix",
            "prefix",
            Some('x'),
            "Add prefix to read names",
        ))
        .arg(value_arg(
            "suffix",
            "suffix",
            Some('y'),
            "Add suffix to read names",
        ))
        .arg(value_arg(
            "rename",
            "rename",
            None,
            "Rename reads using TEMPLATE",
        ))
        .arg(flag_arg(
            "zero_cap",
            "zero-cap",
            Some('z'),
            "Change negative qualities to zero",
        ))
        // --- filters
        .arg(value_arg(
            "minimum_length",
            "minimum-length",
            Some('m'),
            "Discard reads shorter than LEN",
        ))
        .arg(value_arg(
            "maximum_length",
            "maximum-length",
            Some('M'),
            "Discard reads longer than LEN",
        ))
        .arg(value_arg(
            "max_n",
            "max-n",
            None,
            "Discard reads with more than COUNT Ns",
        ))
        .arg(
            value_arg(
                "max_expected_errors",
                "max-expected-errors",
                None,
                "Expected-error filter",
            )
            .visible_alias("max-ee"),
        )
        .arg(
            value_arg(
                "max_average_error_rate",
                "max-average-error-rate",
                None,
                "Average error rate filter",
            )
            .visible_alias("max-aer"),
        )
        .arg(
            flag_arg(
                "discard_trimmed",
                "discard-trimmed",
                None,
                "Discard reads with an adapter",
            )
            .visible_alias("discard"),
        )
        .arg(
            flag_arg(
                "discard_untrimmed",
                "discard-untrimmed",
                None,
                "Discard reads without an adapter",
            )
            .visible_alias("trimmed-only"),
        )
        .arg(flag_arg(
            "discard_casava",
            "discard-casava",
            None,
            "Discard reads failing CASAVA filter",
        ))
        // --- output
        .arg(flag_arg(
            "quiet",
            "quiet",
            None,
            "Print only error messages",
        ))
        .arg(value_arg("report", "report", None, "full|minimal"))
        .arg(value_arg(
            "json",
            "json",
            None,
            "Write a JSON report to FILE",
        ))
        .arg(value_arg(
            "output",
            "output",
            Some('o'),
            "Output file (default stdout)",
        ))
        .arg(value_arg(
            "paired_output",
            "paired-output",
            Some('p'),
            "Write R2 to FILE",
        ))
        .arg(flag_arg(
            "interleaved",
            "interleaved",
            None,
            "Interleaved paired-end input/output",
        ))
        .arg(flag_arg("fasta", "fasta", None, "Output FASTA"))
        .arg(flag_arg(
            "compression_z",
            "compression-z",
            Some('Z'),
            "Use compression level 1",
        ))
        .arg(value_arg(
            "info_file",
            "info-file",
            None,
            "Write per-read info to FILE",
        ))
        .arg(value_arg(
            "rest_file",
            "rest-file",
            Some('r'),
            "Write the rest after the adapter to FILE",
        ))
        .arg(value_arg(
            "wildcard_file",
            "wildcard-file",
            None,
            "Write wildcard bases to FILE",
        ))
        .arg(value_arg(
            "too_short_output",
            "too-short-output",
            None,
            "Write too-short reads to FILE",
        ))
        .arg(value_arg(
            "too_long_output",
            "too-long-output",
            None,
            "Write too-long reads to FILE",
        ))
        .arg(value_arg(
            "untrimmed_output",
            "untrimmed-output",
            None,
            "Write untrimmed reads to FILE",
        ))
        .arg(value_arg(
            "too_short_paired_output",
            "too-short-paired-output",
            None,
            "R2 of too-short pairs",
        ))
        .arg(value_arg(
            "too_long_paired_output",
            "too-long-paired-output",
            None,
            "R2 of too-long pairs",
        ))
        .arg(value_arg(
            "untrimmed_paired_output",
            "untrimmed-paired-output",
            None,
            "R2 of untrimmed pairs",
        ))
        .arg(flag_arg(
            "pair_adapters",
            "pair-adapters",
            None,
            "Treat -a/-A as adapter pairs",
        ))
        .arg(value_arg(
            "pair_filter",
            "pair-filter",
            None,
            "any|both|first",
        ))
        .arg(
            Arg::new("input")
                .value_name("INPUT")
                .help("Input file or - for stdin"),
        )
        .arg(
            Arg::new("input2")
                .value_name("INPUT2")
                .help("R2 input for paired-end data"),
        )
}

fn get_string(m: &clap::ArgMatches, name: &str) -> Option<String> {
    m.get_one::<String>(name).cloned()
}

fn get_usize(m: &clap::ArgMatches, name: &str, default: usize) -> usize {
    m.get_one::<String>(name)
        .map(|v| v.parse().unwrap_or(default))
        .unwrap_or(default)
}

fn get_u8(m: &clap::ArgMatches, name: &str, default: u8) -> u8 {
    m.get_one::<String>(name)
        .map(|v| v.parse().unwrap_or(default))
        .unwrap_or(default)
}

fn flag(m: &clap::ArgMatches, name: &str) -> bool {
    m.get_flag(name)
}

/// Parse `[5'CUTOFF,]3'CUTOFF` into `(5', 3')`.
fn parse_quality_cutoff(value: &str) -> Result<(Option<u8>, Option<u8>), UsageError> {
    let parts: Vec<&str> = value.split(',').collect();
    match parts.as_slice() {
        [one] => {
            let v: u8 = one
                .trim()
                .parse()
                .map_err(|_| UsageError(format!("invalid quality cutoff '{one}'")))?;
            Ok((None, Some(v)))
        }
        [five, three] => {
            let f: u8 = five
                .trim()
                .parse()
                .map_err(|_| UsageError(format!("invalid 5' quality cutoff '{five}'")))?;
            let t: u8 = three
                .trim()
                .parse()
                .map_err(|_| UsageError(format!("invalid 3' quality cutoff '{three}'")))?;
            Ok((Some(f), Some(t)))
        }
        _ => Err(UsageError(format!(
            "invalid quality cutoff '{value}': expected [5'CUTOFF,]3'CUTOFF"
        ))),
    }
}

/// Parse `LEN[:LEN2]`.
fn parse_length_pair(value: &str) -> Result<(usize, Option<usize>), UsageError> {
    let parts: Vec<&str> = value.split(':').collect();
    let first: usize = parts[0]
        .trim()
        .parse()
        .map_err(|_| UsageError(format!("invalid length '{}'", parts[0])))?;
    match parts.len() {
        1 => Ok((first, None)),
        2 => {
            let second: usize = parts[1]
                .trim()
                .parse()
                .map_err(|_| UsageError(format!("invalid length '{}'", parts[1])))?;
            Ok((first, Some(second)))
        }
        _ => Err(UsageError(format!(
            "invalid length specification '{value}'"
        ))),
    }
}

/// Parse absolute or rate error specification.
fn parse_error_rate(value: &str) -> Result<ErrorRate, UsageError> {
    let v: f64 = value
        .parse()
        .map_err(|_| UsageError(format!("invalid error rate '{value}'")))?;
    if v < 0.0 {
        return Err(UsageError("the error rate must not be negative".into()));
    }
    Ok(ErrorRate::from_cli(v))
}

/// Parse and validate the command line.
pub fn parse<I, T>(args: I) -> Result<Parsed, UsageError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let raw: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut argv: Vec<OsString> = Vec::with_capacity(raw.len() + 1);
    argv.push(OsString::from("rust-cutadapt"));
    argv.extend(raw.iter().cloned());

    let matches = match command().try_get_matches_from(argv) {
        Ok(m) => m,
        Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp => {
            return Ok(Parsed::Help(e.render().to_string()))
        }
        Err(e) if e.kind() == clap::error::ErrorKind::DisplayVersion => {
            return Ok(Parsed::Version(e.render().to_string()))
        }
        Err(e) => return Err(UsageError(e.render().to_string())),
    };

    let mut cli = Cli {
        times: 1,
        quality_base: 33,
        cores: DEFAULT_CORES,
        ..Default::default()
    };

    // --- adapters ---------------------------------------------------------
    // Adapters must keep their command-line order across -a/-g/-b (and
    // -A/-G/-B), because that order breaks ties between equally good matches.
    // clap stores each option separately, so walk the raw arguments instead.
    {
        let mut pending: Option<char> = None;
        for (position, arg) in raw.iter().enumerate() {
            let text = arg.to_string_lossy();
            let value = if let Some(spec) = pending.take() {
                Some((spec, text.to_string()))
            } else if text == "-a" || text == "--adapter" {
                pending = Some('a');
                None
            } else if text == "-g" || text == "--front" {
                pending = Some('g');
                None
            } else if text == "-b" || text == "--anywhere" {
                pending = Some('b');
                None
            } else if let Some(rest) = text.strip_prefix("-a") {
                if !rest.is_empty() {
                    Some(('a', rest.to_string()))
                } else {
                    None
                }
            } else if let Some(rest) = text.strip_prefix("-g") {
                if !rest.is_empty() {
                    Some(('g', rest.to_string()))
                } else {
                    None
                }
            } else if let Some(rest) = text.strip_prefix("-b") {
                if !rest.is_empty() {
                    Some(('b', rest.to_string()))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((option, text)) = value {
                // `-a file:FILE` loads one adapter per FASTA record.
                if let Some(path) = text.strip_prefix("file:") {
                    let first = cli.adapters.len();
                    match adapter::parse_fasta_file(option, path, first) {
                        Ok(loaded) => cli.adapters.extend(loaded),
                        // cutadapt reports an unreadable/malformed adapter list
                        // as a runtime error (exit 1), not a usage error.
                        Err(message) => {
                            cli.adapter_file_error = Some(message);
                            return Ok(Parsed::Run(Box::new(cli)));
                        }
                    }
                } else {
                    // `-a NAME=SEQ` names the adapter inline.
                    let (inline_name, sequence) = adapter::split_inline_name(&text);
                    let index = cli.adapters.len();
                    let spec = adapter::parse(option, sequence, index, inline_name)
                        .map_err(|e| UsageError(format!("{e}")))?;
                    cli.adapters.push(spec);
                }
            }
            let _ = position;
        }
    }

    // Read-2 adapters are kept in their own list but numbered continuously, so
    // the report section for the first `-A` adapter follows the last `-a` one
    // (oracle: `-a A -A B` reports "Adapter 1" then "Adapter 2").
    let read2_offset = cli.adapters.len();
    for (option, name) in [('A', "adapter2"), ('G', "front2"), ('B', "anywhere2")] {
        if let Some(values) = matches.get_many::<String>(name) {
            for raw in values {
                if let Some(path) = raw.strip_prefix("file:") {
                    let first = cli.adapters.len() + cli.adapters2.len();
                    let loaded =
                        adapter::parse_fasta_file(option, path, first).map_err(UsageError)?;
                    cli.adapters2.extend(loaded);
                } else {
                    let (inline_name, sequence) = adapter::split_inline_name(raw);
                    let index = read2_offset + cli.adapters2.len();
                    let spec = adapter::parse(option, sequence, index, inline_name)
                        .map_err(|e| UsageError(format!("{e}")))?;
                    cli.adapters2.push(spec);
                }
            }
        }
    }

    // cutadapt allows a run with no adapter at all: it then acts purely as a
    // length/quality filter. `-m`, `-q`, `--max-n` and friends still apply, so
    // this is not an error.

    let mut match_config = MatchConfig::default();
    if let Some(v) = get_string(&matches, "error_rate") {
        match_config.error_rate = parse_error_rate(&v)?;
    }
    if let Some(v) = get_string(&matches, "overlap") {
        let overlap: usize = v
            .parse()
            .map_err(|_| UsageError(format!("invalid minimum overlap '{v}'")))?;
        if overlap < 1 {
            return Err(UsageError(
                "ERROR: The overlap must be at least 1.".to_string(),
            ));
        }
        match_config.min_overlap = overlap;
    }
    match_config.allow_indels = !flag(&matches, "no_indels");
    cli.match_config = match_config;
    // `-n 0` disables trimming entirely, which cutadapt reports as
    // "Trimmed: 0 times" while still writing the reads.
    cli.times = get_usize(&matches, "times", 1);
    cli.match_read_wildcards = flag(&matches, "match_read_wildcards");
    cli.no_match_adapter_wildcards = flag(&matches, "no_match_adapter_wildcards");
    cli.pair_adapters = flag(&matches, "pair_adapters");
    cli.reverse_complement = flag(&matches, "revcomp");

    cli.action = match get_string(&matches, "action").as_deref() {
        None | Some("trim") => Action::Trim,
        Some("retain") => Action::Retain,
        Some("mask") => Action::Mask,
        Some("lowercase") => Action::Lowercase,
        Some("none") => Action::None,
        Some(other) => {
            return Err(UsageError(format!(
            "invalid value '{other}' for --action: choose from trim, retain, mask, lowercase, none"
        )))
        }
    };

    // --- read modifications ----------------------------------------------
    let (mut cut5, mut cut3) = (0usize, 0usize);
    for value in matches.get_many::<String>("cut").into_iter().flatten() {
        let v: i64 = value
            .parse()
            .map_err(|_| UsageError(format!("invalid --cut value '{value}'")))?;
        if v >= 0 {
            cut5 += v as usize;
        } else {
            cut3 += (-v) as usize;
        }
    }
    cli.cut_5prime = cut5;
    cli.cut_3prime = cut3;
    let (mut cut5r2, mut cut3r2) = (0usize, 0usize);
    for value in matches.get_many::<String>("cut2").into_iter().flatten() {
        let v: i64 = value
            .parse()
            .map_err(|_| UsageError(format!("invalid -U value '{value}'")))?;
        if v >= 0 {
            cut5r2 += v as usize;
        } else {
            cut3r2 += (-v) as usize;
        }
    }
    cli.cut_5prime_r2 = cut5r2;
    cli.cut_3prime_r2 = cut3r2;

    if let Some(v) = get_string(&matches, "quality_cutoff") {
        let (five, three) = parse_quality_cutoff(&v)?;
        cli.quality_cutoff_5prime = five;
        cli.quality_cutoff_3prime = three;
    }
    if let Some(v) = get_string(&matches, "quality_cutoff2") {
        let (five, three) = parse_quality_cutoff(&v)?;
        cli.quality_cutoff_5prime_r2 = five;
        cli.quality_cutoff_3prime_r2 = three;
    }
    if let Some(v) = get_string(&matches, "nextseq_trim") {
        cli.nextseq_trim = Some(
            v.parse()
                .map_err(|_| UsageError(format!("invalid --nextseq-trim value '{v}'")))?,
        );
    }
    cli.quality_base = get_u8(&matches, "quality_base", 33);
    cli.poly_a = flag(&matches, "poly_a");
    if let Some(v) = get_string(&matches, "length") {
        cli.length = Some(
            v.parse()
                .map_err(|_| UsageError(format!("invalid --length value '{v}'")))?,
        );
    }
    cli.trim_n = flag(&matches, "trim_n");
    cli.length_tag = get_string(&matches, "length_tag");
    cli.strip_suffixes = matches
        .get_many::<String>("strip_suffix")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    cli.prefix = get_string(&matches, "prefix");
    cli.suffix = get_string(&matches, "suffix");
    cli.rename = get_string(&matches, "rename");
    cli.zero_cap = flag(&matches, "zero_cap");

    // --- filters ---------------------------------------------------------
    if let Some(v) = get_string(&matches, "minimum_length") {
        let (a, b) = parse_length_pair(&v)?;
        cli.minimum_length = a;
        cli.minimum_length_r2 = b;
        cli.minimum_length_given = true;
    }
    if let Some(v) = get_string(&matches, "maximum_length") {
        let (a, b) = parse_length_pair(&v)?;
        cli.maximum_length = Some(a);
        cli.maximum_length_r2 = b;
    }
    if let Some(v) = get_string(&matches, "max_n") {
        cli.max_n = Some(
            v.parse()
                .map_err(|_| UsageError(format!("invalid --max-n value '{v}'")))?,
        );
    }
    if let Some(v) = get_string(&matches, "max_expected_errors") {
        cli.max_expected_errors = Some(
            v.parse()
                .map_err(|_| UsageError(format!("invalid --max-expected-errors value '{v}'")))?,
        );
    }
    if let Some(v) = get_string(&matches, "max_average_error_rate") {
        cli.max_average_error_rate =
            Some(v.parse().map_err(|_| {
                UsageError(format!("invalid --max-average-error-rate value '{v}'"))
            })?);
    }
    cli.discard_trimmed = flag(&matches, "discard_trimmed");
    cli.discard_untrimmed = flag(&matches, "discard_untrimmed");
    cli.discard_casava = flag(&matches, "discard_casava");
    cli.pair_filter = match get_string(&matches, "pair_filter").as_deref() {
        None | Some("any") => PairFilter::Any,
        Some("both") => PairFilter::Both,
        Some("first") => PairFilter::First,
        Some(other) => {
            return Err(UsageError(format!(
                "invalid value '{other}' for --pair-filter: choose from any, both, first"
            )))
        }
    };

    // --- output ----------------------------------------------------------
    cli.output = get_string(&matches, "output").map(PathBuf::from);
    cli.paired_output = get_string(&matches, "paired_output").map(PathBuf::from);
    cli.interleaved = flag(&matches, "interleaved");
    cli.fasta = flag(&matches, "fasta");
    cli.compression_level_1 = flag(&matches, "compression_z");
    cli.info_file = get_string(&matches, "info_file").map(PathBuf::from);
    cli.rest_file = get_string(&matches, "rest_file").map(PathBuf::from);
    cli.wildcard_file = get_string(&matches, "wildcard_file").map(PathBuf::from);
    cli.too_short_output = get_string(&matches, "too_short_output").map(PathBuf::from);
    cli.too_long_output = get_string(&matches, "too_long_output").map(PathBuf::from);
    cli.untrimmed_output = get_string(&matches, "untrimmed_output").map(PathBuf::from);
    cli.too_short_paired_output =
        get_string(&matches, "too_short_paired_output").map(PathBuf::from);
    cli.too_long_paired_output = get_string(&matches, "too_long_paired_output").map(PathBuf::from);
    cli.untrimmed_paired_output =
        get_string(&matches, "untrimmed_paired_output").map(PathBuf::from);
    cli.report = match get_string(&matches, "report").as_deref() {
        None | Some("full") => ReportKind::Full,
        Some("minimal") => ReportKind::Minimal,
        Some(other) => {
            return Err(UsageError(format!(
                "invalid value '{other}' for --report: choose from full, minimal"
            )))
        }
    };
    cli.json = get_string(&matches, "json").map(PathBuf::from);
    cli.quiet = flag(&matches, "quiet");
    cli.debug = matches.get_count("debug");

    // --- runtime ---------------------------------------------------------
    cli.cores = get_usize(&matches, "cores", DEFAULT_CORES);
    cli.input = PathBuf::from(
        matches
            .get_one::<String>("input")
            .ok_or_else(|| UsageError("error: missing INPUT file".to_string()))?
            .clone(),
    );
    cli.input2 = matches
        .get_one::<String>("input2")
        .cloned()
        .map(PathBuf::from);

    // Paired auxiliary outputs must be used in matching pairs, exactly as
    // cutadapt validates them (its messages are reproduced verbatim).
    let is_paired_run = cli.input2.is_some() || cli.interleaved || cli.paired_output.is_some();
    for (unpaired, paired, flag) in [
        (
            cli.untrimmed_output.is_some(),
            cli.untrimmed_paired_output.is_some(),
            "--untrimmed-output/--untrimmed-paired-output",
        ),
        (
            cli.too_short_output.is_some(),
            cli.too_short_paired_output.is_some(),
            "--too-short-output/--too-short-paired-output",
        ),
        (
            cli.too_long_output.is_some(),
            cli.too_long_paired_output.is_some(),
            "--too-long-output/--too-long-paired-output",
        ),
    ] {
        if is_paired_run && unpaired != paired {
            return Err(UsageError(format!(
                "ERROR: When trimming paired-end data, you must use either none or both of the {flag} options."
            )));
        }
    }

    Ok(Parsed::Run(Box::new(cli)))
}

/// Formats the command line the way cutadapt's report does.
pub fn command_line_string(args: &[String]) -> String {
    args.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(list: &[&str]) -> Cli {
        let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        match parse(args).expect("valid command line") {
            Parsed::Run(cli) => *cli,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn minimal_invocation() {
        let cli = parse_args(&["-a", "AGATCGGAAGAGC", "in.fastq"]);
        assert_eq!(cli.adapters.len(), 1);
        assert_eq!(cli.adapters[0].sequence, b"AGATCGGAAGAGC");
        assert_eq!(cli.minimum_length, 0);
        assert_eq!(cli.output, None);
        assert_eq!(cli.times, 1);
    }

    #[test]
    fn parses_all_adapter_options_in_order() {
        let cli = parse_args(&[
            "-a", "AAAA", "-g", "CCCC", "-b", "GGGG", "-a", "TTTT", "in.fastq",
        ]);
        assert_eq!(cli.adapters.len(), 4);
        assert_eq!(cli.adapters[0].sequence, b"AAAA");
        assert_eq!(cli.adapters[1].sequence, b"CCCC");
        assert_eq!(cli.adapters[2].sequence, b"GGGG");
        assert_eq!(cli.adapters[3].sequence, b"TTTT");
        assert_eq!(cli.adapters[3].index, 3);
    }

    #[test]
    fn parses_anchored_and_linked() {
        let cli = parse_args(&["-a", "ACGT$", "-a", "^AAA...TTT", "in.fastq"]);
        assert_eq!(
            cli.adapters[0].adapter_type,
            crate::adapter::AdapterType::AnchoredThreePrime
        );
        assert_eq!(
            cli.adapters[1].adapter_type,
            crate::adapter::AdapterType::Linked
        );
    }

    #[test]
    fn parses_error_rate_forms() {
        let cli = parse_args(&["-a", "ACGT", "-e", "0.2", "in.fastq"]);
        assert_eq!(cli.match_config.error_rate, ErrorRate::Rate(0.2));
        let cli = parse_args(&["-a", "ACGT", "--errors", "2", "in.fastq"]);
        assert_eq!(cli.match_config.error_rate, ErrorRate::Absolute(2));
    }

    #[test]
    fn indels_on_by_default_and_off_with_flag() {
        assert!(
            parse_args(&["-a", "ACGT", "in.fastq"])
                .match_config
                .allow_indels
        );
        assert!(
            !parse_args(&["-a", "ACGT", "--no-indels", "in.fastq"])
                .match_config
                .allow_indels
        );
    }

    #[test]
    fn parses_quality_cutoffs() {
        let cli = parse_args(&["-a", "ACGT", "-q", "20", "in.fastq"]);
        assert_eq!(cli.quality_cutoff_5prime, None);
        assert_eq!(cli.quality_cutoff_3prime, Some(20));
        let cli = parse_args(&["-a", "ACGT", "-q", "15,25", "in.fastq"]);
        assert_eq!(cli.quality_cutoff_5prime, Some(15));
        assert_eq!(cli.quality_cutoff_3prime, Some(25));
    }

    #[test]
    fn parses_signed_cuts() {
        let cli = parse_args(&["-a", "ACGT", "-u", "5", "-u", "-3", "in.fastq"]);
        assert_eq!(cli.cut_5prime, 5);
        assert_eq!(cli.cut_3prime, 3);
    }

    #[test]
    fn parses_length_pairs() {
        let cli = parse_args(&["-a", "ACGT", "-m", "10:20", "in.fastq"]);
        assert_eq!(cli.minimum_length, 10);
        assert_eq!(cli.minimum_length_r2, Some(20));
    }

    #[test]
    fn parses_actions() {
        for (name, expected) in [
            ("trim", Action::Trim),
            ("retain", Action::Retain),
            ("mask", Action::Mask),
            ("lowercase", Action::Lowercase),
            ("none", Action::None),
        ] {
            let cli = parse_args(&["-a", "ACGT", "--action", name, "in.fastq"]);
            assert_eq!(cli.action, expected, "action {name}");
        }
    }

    #[test]
    fn rejects_invalid_enum_values() {
        let args: Vec<String> = ["-a", "ACGT", "--action", "bogus", "in.fastq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(parse(args).is_err());
        let args: Vec<String> = ["-a", "ACGT", "--pair-filter", "bogus", "in.fastq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(parse(args).is_err());
    }

    #[test]
    fn allows_a_run_without_adapters() {
        // cutadapt allows a run with no adapter at all (a pure length filter),
        // verified with the oracle: `cutadapt -m 0 -o out.fastq in.fastq` exits 0.
        let args: Vec<String> = ["in.fastq"].iter().map(|s| s.to_string()).collect();
        let cli = match parse(args).expect("no adapter is legal") {
            Parsed::Run(cli) => *cli,
            other => panic!("expected a run, got {other:?}"),
        };
        assert!(cli.adapters.is_empty());
        assert_eq!(cli.input.to_str(), Some("in.fastq"));
    }

    #[test]
    fn parses_output_options() {
        let cli = parse_args(&[
            "-a",
            "ACGT",
            "-o",
            "out.fastq",
            "--info-file",
            "i.txt",
            "--rest-file",
            "r.txt",
            "--wildcard-file",
            "w.txt",
            "--too-short-output",
            "ts.fastq",
            "--too-long-output",
            "tl.fastq",
            "--untrimmed-output",
            "u.fastq",
            "in.fastq",
        ]);
        assert_eq!(cli.output, Some(PathBuf::from("out.fastq")));
        assert_eq!(cli.info_file, Some(PathBuf::from("i.txt")));
        assert_eq!(cli.rest_file, Some(PathBuf::from("r.txt")));
        assert_eq!(cli.wildcard_file, Some(PathBuf::from("w.txt")));
        assert_eq!(cli.too_short_output, Some(PathBuf::from("ts.fastq")));
        assert_eq!(cli.too_long_output, Some(PathBuf::from("tl.fastq")));
        assert_eq!(cli.untrimmed_output, Some(PathBuf::from("u.fastq")));
    }

    #[test]
    fn parses_paired_end_options() {
        let cli = parse_args(&[
            "-a",
            "AAAA",
            "-A",
            "CCCC",
            "-o",
            "o1.fastq",
            "-p",
            "o2.fastq",
            "--pair-filter",
            "both",
            "in1.fastq",
            "in2.fastq",
        ]);
        // R1 and R2 adapters are kept in separate sets: -a goes to adapters,
        // -A to adapters2.
        assert_eq!(cli.adapters.len(), 1);
        assert_eq!(cli.adapters2.len(), 1);
        assert_eq!(cli.adapters2[0].sequence, b"CCCC");
        assert_eq!(cli.input2, Some(PathBuf::from("in2.fastq")));
        assert_eq!(cli.paired_output, Some(PathBuf::from("o2.fastq")));
        assert_eq!(cli.pair_filter, PairFilter::Both);
    }

    #[test]
    fn help_and_version_are_successful_requests() {
        for flag in ["--help", "-h"] {
            let args = vec![flag.to_string()];
            assert!(matches!(parse(args), Ok(Parsed::Help(_))), "{flag}");
        }
        for flag in ["--version", "-V"] {
            let args = vec![flag.to_string()];
            assert!(matches!(parse(args), Ok(Parsed::Version(_))), "{flag}");
        }
    }

    #[test]
    fn accepts_attached_short_values() {
        let cli = parse_args(&["-aAGATCGGAAGAGC", "-m10", "in.fastq"]);
        assert_eq!(cli.adapters[0].sequence, b"AGATCGGAAGAGC");
        assert_eq!(cli.minimum_length, 10);
    }
}
