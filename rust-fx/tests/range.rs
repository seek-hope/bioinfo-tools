//! `range -r` regressions: 1-based and negative record ranges.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-range-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nA\n>r2\nC\n>r3\nG\n>r4\nT\n").unwrap();
        path
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
fn multiple_files_have_independent_ranges_and_bounded_negative_tails() {
    let scratch = Scratch::new();
    let first = scratch.0.join("first.fa");
    let second = scratch.0.join("second.fa");
    fs::write(&first, b">a1\nA\n>a2\nAA\n").unwrap();
    fs::write(&second, b">b1\nC\n>b2\nCC\n>b3\nCCC\n").unwrap();
    let args = [
        "range",
        "-r",
        "2:4",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ];
    let output = scratch.run(&args);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">a2\nAA\n>b2\nCC\n>b3\nCCC\n"
    );

    let tail = scratch.run(&[
        "range",
        "-r",
        "-2:-1",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(tail.status.success(), "{tail:?}");
    assert_eq!(
        String::from_utf8_lossy(&tail.stdout),
        ">a1\nA\n>a2\nAA\n>b2\nCC\n>b3\nCCC\n"
    );
}

#[test]
fn invalid_ranges_fail_before_output_is_created() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    for region in ["0:1", "1:0", "-2:3", "2:-2", "-1:-2", "1:", ":"] {
        let output = scratch.0.join("rejected.fa");
        let result = scratch.run(&[
            "range",
            "-r",
            region,
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert_eq!(result.status.code(), Some(255), "{region}: {result:?}");
        assert!(!output.exists());
    }
}

#[test]
fn short_negative_ranges_follow_the_upstream_circular_tail() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    for (range, expected) in [
        ("-9:-6", ">r1\nA\n>r2\nC\n>r3\nG\n"),
        ("-9:-5", ">r1\nA\n>r2\nC\n>r3\nG\n>r4\nT\n"),
        ("-9:-4", ">r1\nA\n"),
        ("-3:-3", ">r2\nC\n"),
        ("-1000000000:-1", ">r1\nA\n>r2\nC\n>r3\nG\n>r4\nT\n"),
    ] {
        let result = scratch.run(&["range", "-r", range, input.to_str().unwrap()]);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, expected.as_bytes());
    }
}

#[test]
fn wrapped_fastq_and_explicit_output_width_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.0.join("wrapped.fq");
    fs::write(&input, b"@q description\nAC\nGT\n+discard\nII\nJJ\n").unwrap();
    for (args, expected) in [
        (
            vec!["range", "-r", "1:-1"],
            b"@q description\nACGT\n+\nIIJJ\n".as_slice(),
        ),
        (
            vec!["range", "-r", "1:-1", "-w", "2"],
            b"@q description\nAC\nGT\n+\nII\nJJ\n".as_slice(),
        ),
    ] {
        let mut args = args;
        args.push(input.to_str().unwrap());
        let result = scratch.run(&args);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, expected);
    }
}

#[test]
fn mixed_files_never_reuse_another_records_quality() {
    let scratch = Scratch::new();
    let fastq = scratch.0.join("first.fq");
    let fasta = scratch.0.join("second.fa");
    fs::write(&fastq, b"@q\nACGT\n+\nJKLM\n").unwrap();
    fs::write(&fasta, b">f\nAAAA\n").unwrap();
    let result = scratch.run(&[
        "range",
        "-r",
        "1:-1",
        "-w",
        "2",
        fastq.to_str().unwrap(),
        fasta.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    // Deliberate deviation: the pinned upstream may retain JKLM from its pooled
    // FASTQ reader when forcing the subsequent FASTA record into FASTQ.
    assert_eq!(result.stdout, b"@q\nAC\nGT\n+\nJK\nLM\n>f\nAA\nAA\n");
}

#[test]
fn positive_and_negative_record_ranges_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let input = input.to_str().unwrap();

    let first = scratch.run(&["range", "-r", "1:2", input]);
    assert_eq!(String::from_utf8_lossy(&first.stdout), ">r1\nA\n>r2\nC\n");

    let middle = scratch.run(&["range", "-r", "2:3", input]);
    assert_eq!(String::from_utf8_lossy(&middle.stdout), ">r2\nC\n>r3\nG\n");

    let tail = scratch.run(&["range", "-r", "-2:-1", input]);
    assert_eq!(String::from_utf8_lossy(&tail.stdout), ">r3\nG\n>r4\nT\n");

    let from_second = scratch.run(&["range", "-r", "2:-1", input]);
    assert_eq!(
        String::from_utf8_lossy(&from_second.stdout),
        ">r2\nC\n>r3\nG\n>r4\nT\n"
    );
}
