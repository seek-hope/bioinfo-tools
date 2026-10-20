//! CLI parsing tests: the full cutadapt 4.6 surface, and exit-code-2 rejections.

use rust_cutadapt::adapter::{parse as parse_adapter, AdapterType};
use rust_cutadapt::cli::{parse, Action, PairFilter, Parsed, ReportKind};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Parse a valid invocation, panicking if parsing fails or if the arguments
/// unexpectedly asked for help/version instead of a run.
fn parse_run(list: Vec<String>) -> rust_cutadapt::cli::Cli {
    match parse(list).expect("valid invocation") {
        Parsed::Run(cli) => *cli,
        other => panic!("expected a run, got {other:?}"),
    }
}

#[test]
fn parses_documented_subset() {
    let cli = parse_run(args(&[
        "-a",
        "AGATCGGAAGAGC",
        "-m",
        "10",
        "-o",
        "out.fastq.gz",
        "in.fastq.gz",
    ]));
    assert_eq!(cli.adapters.len(), 1);
    assert_eq!(cli.adapters[0].sequence, b"AGATCGGAAGAGC");
    assert_eq!(cli.minimum_length, 10);
    assert_eq!(
        cli.output.as_ref().and_then(|p| p.to_str()),
        Some("out.fastq.gz")
    );
    assert_eq!(cli.input.to_str(), Some("in.fastq.gz"));
    assert!(!cli.quiet);
}

#[test]
fn accepts_long_option_forms() {
    let cli = parse_run(args(&[
        "--adapter",
        "AGATCGGAAGAGC",
        "--minimum-length",
        "0",
        "--output",
        "out.fastq",
        "--quiet",
        "-",
    ]));
    assert_eq!(cli.minimum_length, 0);
    assert!(cli.quiet);
    assert_eq!(cli.input.to_str(), Some("-"));
}

#[test]
fn errors_exit_with_code_two() {
    assert_eq!(rust_cutadapt::cli::USAGE_EXIT_CODE, 2);
}

#[test]
fn all_adapter_notations_parse() {
    for (option, text, expected) in [
        ('a', "AGATCGGAAGAGC", AdapterType::RegularThreePrime),
        ('a', "AGATCGGAAGAGC$", AdapterType::AnchoredThreePrime),
        ('g', "ACGTACGT", AdapterType::RegularFivePrime),
        ('g', "^ACGTACGT", AdapterType::AnchoredFivePrime),
        ('b', "ACGTACGT", AdapterType::Anywhere),
        ('a', "^AAA...TTT", AdapterType::Linked),
    ] {
        let spec = parse_adapter(option, text, 0, None).expect("valid adapter");
        assert_eq!(spec.adapter_type, expected, "{option} {text}");
    }
}

#[test]
fn rejects_both_streams_being_stdin_stdout() {
    // `-o -` with stdin input is supported (reads go to stdout), so the only
    // conflict is a missing input.
    let err = parse(args(&["-a", "ACGT"])).unwrap_err();
    assert!(err.0.contains("INPUT"), "{}", err.0);
}

#[test]
fn allows_missing_adapter() {
    // cutadapt allows a run with no adapter (pure length/quality filtering).
    let cli = parse_run(args(&["-m", "10", "-o", "x.fastq", "in.fastq"]));
    assert!(cli.adapters.is_empty());
    assert_eq!(cli.minimum_length, 10);
}

#[test]
fn accepts_iupac_wildcards_but_rejects_other_characters() {
    // Oracle: IUPAC wildcards in an adapter are accepted (adapter wildcards are
    // on by default), while characters outside the IUPAC set are usage errors.
    assert!(parse(args(&["-a", "AGATCGGAAGAGCN", "in.fastq"])).is_ok());
    let err = parse(args(&["-a", "AGATCGGAAGAGC.", "in.fastq"])).unwrap_err();
    assert!(err.0.contains("invalid character"), "{}", err.0);
}

#[test]
fn rejects_empty_adapter() {
    let err = parse(args(&["-a", "", "in.fastq"])).unwrap_err();
    assert!(err.0.contains("must not be empty"), "{}", err.0);
}

#[test]
fn rejects_negative_error_rate() {
    let err = parse(args(&["-a", "ACGT", "-e", "-0.5", "in.fastq"])).unwrap_err();
    assert!(err.0.contains("not be negative"), "{}", err.0);
}

#[test]
fn rejects_unknown_option() {
    let err = parse(args(&["-a", "ACGT", "--frobnicate", "in.fastq"])).unwrap_err();
    assert!(!err.0.is_empty());
}

#[test]
fn every_documented_option_parses() {
    // One representative invocation per option group in cutadapt --help.
    let invocations: Vec<Vec<&str>> = vec![
        vec!["-a", "ACGT", "--debug", "in.fastq"],
        vec!["-a", "ACGT", "-j", "4", "in.fastq"],
        vec!["-g", "ACGT", "-b", "TTTT", "in.fastq"],
        vec!["-a", "ACGT", "-e", "0.2", "--no-indels", "in.fastq"],
        vec!["-a", "ACGT", "-n", "2", "-O", "5", "in.fastq"],
        vec!["-a", "ACGT", "--match-read-wildcards", "in.fastq"],
        vec!["-a", "ACGT", "-N", "in.fastq"],
        vec!["-a", "ACGT", "--action", "mask", "in.fastq"],
        vec!["-a", "ACGT", "--rc", "in.fastq"],
        vec!["-a", "ACGT", "-u", "5", "-u", "-3", "in.fastq"],
        vec!["-a", "ACGT", "--nextseq-trim", "20", "in.fastq"],
        vec!["-a", "ACGT", "-q", "20,25", "in.fastq"],
        vec!["-a", "ACGT", "--quality-base", "64", "in.fastq"],
        vec!["-a", "ACGT", "--poly-a", "in.fastq"],
        vec!["-a", "ACGT", "-l", "30", "in.fastq"],
        vec!["-a", "ACGT", "--trim-n", "in.fastq"],
        vec!["-a", "ACGT", "--length-tag", "length=", "in.fastq"],
        vec!["-a", "ACGT", "--strip-suffix", ":1", "in.fastq"],
        vec!["-a", "ACGT", "-x", "PRE_", "-y", "_SUF", "in.fastq"],
        vec!["-a", "ACGT", "--rename", "{id}_{adapter_name}", "in.fastq"],
        vec!["-a", "ACGT", "-z", "in.fastq"],
        vec!["-a", "ACGT", "-m", "10:20", "in.fastq"],
        vec!["-a", "ACGT", "-M", "100", "in.fastq"],
        vec!["-a", "ACGT", "--max-n", "2", "in.fastq"],
        vec!["-a", "ACGT", "--max-ee", "2", "in.fastq"],
        vec!["-a", "ACGT", "--max-aer", "0.1", "in.fastq"],
        vec!["-a", "ACGT", "--discard-trimmed", "in.fastq"],
        vec!["-a", "ACGT", "--discard-untrimmed", "in.fastq"],
        vec!["-a", "ACGT", "--discard-casava", "in.fastq"],
        vec!["-a", "ACGT", "--quiet", "in.fastq"],
        vec!["-a", "ACGT", "--report", "minimal", "in.fastq"],
        vec!["-a", "ACGT", "--json", "r.json", "in.fastq"],
        vec!["-a", "ACGT", "-o", "out.fastq", "-Z", "in.fastq"],
        vec!["-a", "ACGT", "--fasta", "in.fastq"],
        vec!["-a", "ACGT", "--info-file", "i.txt", "in.fastq"],
        vec!["-a", "ACGT", "-r", "rest.txt", "in.fastq"],
        vec!["-a", "ACGT", "--wildcard-file", "w.txt", "in.fastq"],
        vec!["-a", "ACGT", "--too-short-output", "ts.fastq", "in.fastq"],
        vec!["-a", "ACGT", "--too-long-output", "tl.fastq", "in.fastq"],
        vec!["-a", "ACGT", "--untrimmed-output", "u.fastq", "in.fastq"],
        vec![
            "-a",
            "ACGT",
            "-A",
            "TTTT",
            "-p",
            "r2.fastq",
            "--pair-filter",
            "both",
            "r1.fastq",
            "r2_in.fastq",
        ],
        vec![
            "-a", "ACGT", "-G", "TTTT", "-B", "GGGG", "-U", "5", "-Q", "20", "in.fastq",
        ],
        vec!["-a", "ACGT", "--interleaved", "-o", "out.fastq", "in.fastq"],
        vec!["-a", "ACGT", "--pair-adapters", "in.fastq"],
        vec![
            "-a",
            "ACGT",
            "--untrimmed-paired-output",
            "u2.fastq",
            "in.fastq",
        ],
        vec![
            "-a",
            "ACGT",
            "--too-short-paired-output",
            "t2.fastq",
            "in.fastq",
        ],
        vec![
            "-a",
            "ACGT",
            "--too-long-paired-output",
            "l2.fastq",
            "in.fastq",
        ],
    ];
    for invocation in invocations {
        let parsed = parse(args(&invocation));
        assert!(
            parsed.is_ok(),
            "invocation {invocation:?} should parse, got {:?}",
            parsed.err()
        );
    }
}

#[test]
fn parses_enums() {
    let cli = parse_run(args(&["-a", "ACGT", "--action", "lowercase", "in.fastq"]));
    assert_eq!(cli.action, Action::Lowercase);
    let cli = parse_run(args(&["-a", "ACGT", "--report", "minimal", "in.fastq"]));
    assert_eq!(cli.report, ReportKind::Minimal);
    let cli = parse_run(args(&["-a", "ACGT", "--pair-filter", "first", "in.fastq"]));
    assert_eq!(cli.pair_filter, PairFilter::First);
}

#[test]
fn defaults_match_documented_values() {
    let cli = parse_run(args(&["-a", "ACGT", "in.fastq"]));
    // Documented defaults: -m 0, no output file (stdout), 4 cores (cutadapt
    // defaults to 1), full report, indels allowed, error rate 0.1, overlap 3,
    // trim action.
    assert_eq!(cli.minimum_length, 0);
    assert_eq!(cli.output, None);
    assert_eq!(cli.cores, 4);
    assert_eq!(cli.report, ReportKind::Full);
    assert!(cli.match_config.allow_indels);
    assert_eq!(cli.match_config.min_overlap, 3);
    assert_eq!(cli.action, Action::Trim);
    assert!(!cli.quiet);
}

#[test]
fn help_and_version_are_successful_requests() {
    for flag in ["--help", "-h"] {
        match parse(args(&[flag])).expect("help must not be a usage error") {
            Parsed::Help(text) => assert!(
                text.contains("Usage:") || text.contains("usage:"),
                "help text should contain usage information, got {text:?}"
            ),
            other => panic!("{flag} should produce Parsed::Help, got {other:?}"),
        }
    }
    for flag in ["--version", "-V"] {
        match parse(args(&[flag])).expect("version must not be a usage error") {
            Parsed::Version(text) => assert!(
                text.contains(env!("CARGO_PKG_VERSION")),
                "version text should contain the crate version, got {text:?}"
            ),
            other => panic!("{flag} should produce Parsed::Version, got {other:?}"),
        }
    }
}
