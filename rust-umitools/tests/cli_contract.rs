//! CLI contract tests: help text must describe what the binary actually does.
//!
//! These are intentionally small semantic checks plus checked-in snapshots for
//! the commands whose help previously drifted from the implementation.

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rust-umitools");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("NO_COLOR", "1")
        .env("COLUMNS", "100")
        .output()
        .expect("run rust-umitools")
}

fn stdout(args: &[&str]) -> String {
    let output = run(args);
    assert!(
        output.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help output is UTF-8")
}

#[test]
fn dedup_help_matches_implemented_features() {
    let help = stdout(&["dedup", "--help"]);
    assert!(help.contains("Group by gene instead of by position (requires --gene-tag)"));
    assert!(help.contains("Cell barcode tag (used with --per-cell)"));
    assert!(help.contains("count --per-contig only; dedup rejects it"));
    assert!(help.contains("(rejected: null model not implemented)"));
    assert!(help.contains("--umi-whitelist"));
    assert!(help.contains("--buffer-whole-contig"));
    assert!(help.contains("--compresslevel"));
    assert!(!help.contains("which is not implemented"));
}

#[test]
fn count_help_describes_tsv_output() {
    let help = stdout(&["count", "--help"]);
    assert!(help.contains("output is a TSV table, not BAM"));
    assert!(help.contains("Output file; the format depends on the command"));
    assert!(!help.contains("Output BAM"));
    assert!(help.contains("--gene-transcript-map"));
}

#[test]
fn group_help_uses_generic_output_description() {
    let help = stdout(&["group", "--help"]);
    assert!(help.contains("Output file; the format depends on the command"));
    assert!(help.contains("--progress"));
}

#[test]
fn invalid_progress_is_rejected_before_reading_input() {
    let cases: [&[&str]; 2] = [
        &[
            "group",
            "-I",
            "missing.bam",
            "-S",
            "out.bam",
            "--progress",
            "sometimes",
        ],
        &[
            "count",
            "-I",
            "missing.bam",
            "-S",
            "out.tsv",
            "--per-gene",
            "--progress",
            "sometimes",
        ],
    ];
    for args in cases {
        let output = run(args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "expected usage exit for {args:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unknown --progress value 'sometimes'"),
            "unexpected stderr for {args:?}: {stderr}"
        );
    }
}

#[test]
fn command_help_snapshots_match() {
    assert_eq!(
        stdout(&["dedup", "--help"]),
        include_str!("cli_help/dedup.txt")
    );
    assert_eq!(
        stdout(&["count", "--help"]),
        include_str!("cli_help/count.txt")
    );
    assert_eq!(
        stdout(&["group", "--help"]),
        include_str!("cli_help/group.txt")
    );
}
