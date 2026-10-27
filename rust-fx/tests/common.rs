//! `common` regressions: ID/name/sequence intersection across multiple files.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-common-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn inputs(&self) -> (PathBuf, PathBuf) {
        let first = self.0.join("1.fa");
        fs::write(&first, b">a\nAC\n>b\nGT\n").unwrap();
        let second = self.0.join("2.fa");
        fs::write(&second, b">a\nTT\n>c\nGG\n").unwrap();
        (first, second)
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
fn id_and_name_intersection_match_seqkit() {
    let scratch = Scratch::new();
    let (first, second) = scratch.inputs();
    let first = first.to_str().unwrap();
    let second = second.to_str().unwrap();

    let by_id = scratch.run(&["common", first, second]);
    assert_eq!(String::from_utf8_lossy(&by_id.stdout), ">a\nAC\n");

    let by_name = scratch.run(&["common", "-n", first, second]);
    assert_eq!(String::from_utf8_lossy(&by_name.stdout), ">a\nAC\n");
}

#[test]
fn sequence_intersection_is_empty_for_different_sequences() {
    let scratch = Scratch::new();
    let (first, second) = scratch.inputs();
    let output = scratch.run(&[
        "common",
        "-s",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn embedded_subsequence_uses_coordinates_and_comment() {
    let scratch = Scratch::new();
    let first = scratch.0.join("a.fa");
    let second = scratch.0.join("b.fa");
    fs::write(&first, b">a1 desc\nTTGGGGTT\n").unwrap();
    fs::write(&second, b">b1\nCCCC\n").unwrap();
    let output = scratch.run(&[
        "common",
        "-s",
        "-e",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">a1:3-6 desc\nGGGG\n"
    );
}

#[test]
fn embedded_skips_shorter_common_sequences() {
    let scratch = Scratch::new();
    let first = scratch.0.join("m.fa");
    let second = scratch.0.join("n.fa");
    fs::write(&first, b">m1\nTTGGGGTT\n").unwrap();
    fs::write(&second, b">n1\nGGGG\n>n2\nGGG\n>n3\nCCCC\n").unwrap();
    let output = scratch.run(&[
        "common",
        "-s",
        "-e",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">m1:3-6\nGGGG\n");
}

#[test]
fn embedded_three_files_uses_first_occurrence() {
    let scratch = Scratch::new();
    let first = scratch.0.join("c.fa");
    let second = scratch.0.join("d.fa");
    let third = scratch.0.join("e.fa");
    fs::write(&first, b">c1\nTTGGGGTT\n").unwrap();
    fs::write(&second, b">d1\nGGGG\n").unwrap();
    fs::write(&third, b">e1\nGG\n").unwrap();
    let output = scratch.run(&[
        "common",
        "-s",
        "-e",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        third.to_str().unwrap(),
    ]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">c1:3-4\nGG\n");
}

#[test]
fn embedded_multiple_candidates_are_sorted_by_coordinate() {
    let scratch = Scratch::new();
    let first = scratch.0.join("a.fa");
    let second = scratch.0.join("b.fa");
    fs::write(&first, b">a1 desc\nTTGGGGTT\n>a2\nAAAACCCC\n").unwrap();
    fs::write(&second, b">b1\nCCCC\n>b2\nGGGG\n>b3\nTTTT\n").unwrap();
    let output = scratch.run(&[
        "common",
        "-s",
        "-e",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">a1:3-6 desc\nGGGG\n>a2:1-4\nAAAA\n>a2:5-8\nCCCC\n"
    );
}

#[test]
fn embedded_positive_only_does_not_match_reverse_complement() {
    let scratch = Scratch::new();
    let first = scratch.0.join("r.fa");
    let second = scratch.0.join("s.fa");
    fs::write(&first, b">r1\nCCCC\n").unwrap();
    fs::write(&second, b">s1\nGGGG\n").unwrap();
    let output = scratch.run(&[
        "common",
        "-s",
        "-e",
        "-P",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn embedded_requires_by_seq_and_rejects_by_name() {
    let scratch = Scratch::new();
    let (first, second) = scratch.inputs();
    let first = first.to_str().unwrap();
    let second = second.to_str().unwrap();

    let no_seq = scratch.run(&["common", "-e", first, second]);
    assert!(!no_seq.status.success());

    let both = scratch.run(&["common", "-s", "-n", first, second]);
    assert!(!both.status.success());
}
