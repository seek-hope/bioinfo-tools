//! `rmdup` regressions: ID/name/sequence modes and reverse-complement deduplication.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-rmdup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">a\nAC\n>b\nAC\n>c\nGT\n>a\nTT\n").unwrap();
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
fn id_and_name_modes_keep_first_occurrence() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let by_id = scratch.run(&["rmdup", input]);
    assert_eq!(
        String::from_utf8_lossy(&by_id.stdout),
        ">a\nAC\n>b\nAC\n>c\nGT\n"
    );

    let by_name = scratch.run(&["rmdup", "-n", input]);
    assert_eq!(
        String::from_utf8_lossy(&by_name.stdout),
        ">a\nAC\n>b\nAC\n>c\nGT\n"
    );
}

#[test]
fn duplicate_reports_preserve_first_occurrence_groups() {
    let scratch = Scratch::new();
    let input = scratch.0.join("groups.fa");
    fs::write(
        &input,
        b">z1\nAAAA\n>a1\nTTTT\n>z2\nAAAA\n>a2\nTTTT\n>z3\nCCCC\n>a3\nCCCC\n",
    )
    .unwrap();
    let counts = scratch.0.join("counts.tsv");
    let duplicates = scratch.0.join("duplicates.fa");
    let output = scratch.run(&[
        "rmdup",
        "-s",
        "-D",
        counts.to_str().unwrap(),
        "-d",
        duplicates.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">z1\nAAAA\n>z3\nCCCC\n"
    );
    assert_eq!(
        fs::read_to_string(counts).unwrap(),
        "4\tz1, a1, z2, a2\n2\tz3, a3\n"
    );
    assert_eq!(
        fs::read_to_string(duplicates).unwrap(),
        ">a1\nTTTT\n>z2\nAAAA\n>a2\nTTTT\n>a3\nCCCC\n"
    );
}

#[test]
fn sequence_mode_compares_both_strands_and_ignore_case() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let by_seq = scratch.run(&["rmdup", "-s", input]);
    assert_eq!(String::from_utf8_lossy(&by_seq.stdout), ">a\nAC\n>a\nTT\n");

    let ignore_case = scratch.run(&["rmdup", "-s", "-i", input]);
    assert_eq!(
        String::from_utf8_lossy(&ignore_case.stdout),
        ">a\nAC\n>a\nTT\n"
    );
}
