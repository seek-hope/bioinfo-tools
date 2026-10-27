//! `tab2fx` regressions: 2-column FASTA, 3-column FASTQ and comment skipping.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-tab2fx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rust-fx"))
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn two_and_three_column_tables_match_seqkit() {
    let scratch = Scratch::new();
    let fasta = scratch.0.join("a.tsv");
    fs::write(&fasta, b"r1\tACGT\nr2\tTTTT\n").unwrap();
    let fasta_out = scratch.run(&["tab2fx", fasta.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&fasta_out.stdout),
        ">r1\nACGT\n>r2\nTTTT\n"
    );

    let fastq = scratch.0.join("q.tsv");
    fs::write(&fastq, b"r1\tACGT\tIIII\nr2\tTTTT\tJJJJ\n").unwrap();
    let fastq_out = scratch.run(&["tab2fx", fastq.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&fastq_out.stdout),
        "@r1\nACGT\n+\nIIII\n@r2\nTTTT\n+\nJJJJ\n"
    );
}

#[test]
fn custom_comment_prefixes_replace_the_defaults() {
    let scratch = Scratch::new();
    let table = scratch.0.join("custom.tsv");
    fs::write(&table, b"; skip\n# keep\tACGT\nr1\tACGT\n").unwrap();
    let output = scratch.run(&["tab2fx", "-p", ";", table.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "># keep\nACGT\n>r1\nACGT\n"
    );
}

#[test]
fn comment_lines_and_bad_quality_lengths() {
    let scratch = Scratch::new();
    let comments = scratch.0.join("c.tsv");
    fs::write(&comments, b"# comment\n// other\nr1\tACGT\n").unwrap();
    let output = scratch.run(&["tab2fx", comments.to_str().unwrap()]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">r1\nACGT\n");

    let bad = scratch.0.join("bad.tsv");
    fs::write(&bad, b"bad\tACGT\tII\n").unwrap();
    let failure = scratch.run(&["tab2fx", bad.to_str().unwrap()]);
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("lengths differ"));
}

#[test]
fn buffer_size_checks_whole_input_before_writing() {
    let scratch = Scratch::new();
    for (length, succeeds) in [(1020, true), (1021, false)] {
        let input = scratch.0.join(format!("line-{length}.tsv"));
        fs::write(&input, format!("r1\t{}\n", "A".repeat(length))).unwrap();
        let output = scratch.0.join(format!("line-{length}.fa"));
        let result = scratch.run(&[
            "tab2fx",
            "-b",
            "1K",
            "-w",
            "0",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert_eq!(result.status.success(), succeeds, "{result:?}");
        if succeeds {
            assert_eq!(
                fs::read_to_string(output).unwrap(),
                format!(">r1\n{}\n", "A".repeat(length))
            );
        } else {
            assert_eq!(result.status.code(), Some(255));
            assert_eq!(fs::read(output).unwrap(), b"");
        }
    }

    let partial = scratch.0.join("partial.tsv");
    fs::write(&partial, format!("r1\tACGT\nr2\t{}\n", "A".repeat(2000))).unwrap();
    let output = scratch.0.join("partial.fa");
    let result = scratch.run(&[
        "tab2fx",
        "-b",
        "1K",
        "-o",
        output.to_str().unwrap(),
        partial.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(fs::read(output).unwrap(), b"");

    let invalid = scratch.0.join("invalid.fa");
    let result = scratch.run(&[
        "tab2fx",
        "-b",
        "1KB",
        "-o",
        invalid.to_str().unwrap(),
        partial.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(255));
    assert!(!invalid.exists());
}
