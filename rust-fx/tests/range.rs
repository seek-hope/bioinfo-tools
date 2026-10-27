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
