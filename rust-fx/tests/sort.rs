//! `sort` regressions for the in-memory SeqKit-compatible subset.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-sort-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fastq(&self) -> PathBuf {
        let path = self.0.join("input.fastq");
        fs::write(
            &path,
            b"@b beta\nAA\n+\nII\n@a alpha\nAAAA\n+\nIIII\n@c gamma\nA\n+\nI\n",
        )
        .unwrap();
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

const A: &str = "@a alpha\nAAAA\n+\nIIII\n";
const B: &str = "@b beta\nAA\n+\nII\n";
const C: &str = "@c gamma\nA\n+\nI\n";

#[test]
fn sorts_by_id_name_length_and_sequence() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let input = input.to_str().unwrap();

    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["sort", input]).stdout),
        format!("{A}{B}{C}")
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["sort", "-l", input]).stdout),
        format!("{C}{B}{A}")
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["sort", "-n", input]).stdout),
        format!("{A}{B}{C}")
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["sort", "-s", input]).stdout),
        format!("{C}{B}{A}")
    );
}

#[test]
fn reverse_reverses_the_result() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let result = scratch.run(&["sort", "-r", input.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        format!("{C}{B}{A}")
    );
}

#[test]
fn natural_order_and_sequence_prefix_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.0.join("natural.fa");
    fs::write(
        &input,
        b">r10\nAAAA\n>r2\nAA\n>r1\nAAA\n>r20\nA\n>r02\nAAAAA\n>r3\nACGT\n",
    )
    .unwrap();
    let input = input.to_str().unwrap();
    let natural = scratch.run(&["sort", "-N", input]);
    assert!(natural.status.success(), "{natural:?}");
    assert_eq!(
        String::from_utf8_lossy(&natural.stdout),
        ">r1\nAAA\n>r02\nAAAAA\n>r2\nAA\n>r3\nACGT\n>r10\nAAAA\n>r20\nA\n"
    );
    let prefix = scratch.run(&["sort", "-s", "-L", "1", input]);
    assert!(prefix.status.success(), "{prefix:?}");
    assert_eq!(
        String::from_utf8_lossy(&prefix.stdout),
        ">r20\nA\n>r2\nAA\n>r1\nAAA\n>r10\nAAAA\n>r02\nAAAAA\n>r3\nACGT\n"
    );
}

#[test]
fn two_pass_writes_seqkit_index_and_reuses_offsets() {
    let scratch = Scratch::new();
    let input = scratch.0.join("indexed.fa");
    fs::write(&input, b">r2\nAA\n>r1\nAAA\n").unwrap();
    let output = scratch.run(&["sort", "-2", input.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">r1\nAAA\n>r2\nAA\n"
    );
    let index = PathBuf::from(format!("{}.seqkit.fai", input.display()));
    assert_eq!(
        fs::read_to_string(index).unwrap(),
        "r2\t2\t4\t2\t3\nr1\t3\t11\t3\t4\n"
    );
    let second = scratch.run(&["sort", "-2", input.to_str().unwrap()]);
    assert!(second.status.success(), "{second:?}");
    assert_eq!(second.stdout, output.stdout);
}

#[test]
fn two_pass_rejects_by_bases_before_output() {
    let scratch = Scratch::new();
    let input = scratch.0.join("indexed.fa");
    fs::write(&input, b">r2\nAA\n").unwrap();
    let result = scratch.run(&["sort", "-2", "-b", input.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(255), "{result:?}");
    assert!(result.stdout.is_empty());
}

#[test]
fn two_pass_rejects_multiple_inputs_and_fastq() {
    let scratch = Scratch::new();
    let first = scratch.0.join("first.fa");
    let second = scratch.0.join("second.fa");
    fs::write(&first, b">r2\nAA\n").unwrap();
    fs::write(&second, b">r1\nAAA\n").unwrap();
    let multiple = scratch.run(&[
        "sort",
        "-2",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert_eq!(multiple.status.code(), Some(255), "{multiple:?}");
    assert!(multiple.stdout.is_empty());

    let fastq = scratch.fastq();
    let result = scratch.run(&["sort", "-2", fastq.to_str().unwrap()]);
    assert_ne!(result.status.code(), Some(0), "{result:?}");
    assert!(result.stdout.is_empty());
}

#[test]
fn conflicting_sort_modes_are_a_usage_error() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let result = scratch.run(&["sort", "-l", "-n", input.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(2), "{result:?}");
    assert!(result.stdout.is_empty());
}
