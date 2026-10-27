//! `grep` regressions for ID/name/sequence matching.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-grep-{}-{}",
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
            b"@r1 desc\nACGT\n+\nIIII\n@r2 other\nTTTT\n+\nIIII\n@R3 third\nACGTAC\n+\nIIIIII\n",
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

const R2: &str = "@r2 other\nTTTT\n+\nIIII\n";
const R1: &str = "@r1 desc\nACGT\n+\nIIII\n";
const R3: &str = "@R3 third\nACGTAC\n+\nIIIIII\n";

#[test]
fn id_name_and_sequence_modes() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let input = input.to_str().unwrap();

    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["grep", "-p", "r2", input]).stdout),
        R2
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["grep", "-n", "-p", "r1 desc", input]).stdout),
        R1
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["grep", "-s", "-p", "ACG", input]).stdout),
        format!("{R1}{R3}")
    );
}

#[test]
fn invert_count_case_and_pattern_file() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let input = input.to_str().unwrap();

    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["grep", "-v", "-p", "r2", input]).stdout),
        format!("{R1}{R3}")
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["grep", "-C", "-p", "r2", input]).stdout),
        "1\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&scratch.run(&["grep", "-i", "-p", "r3", input]).stdout),
        R3
    );
    let patterns = scratch.0.join("patterns.txt");
    fs::write(&patterns, b"r1\nR3\n").unwrap();
    assert_eq!(
        String::from_utf8_lossy(
            &scratch
                .run(&["grep", "-f", patterns.to_str().unwrap(), input])
                .stdout
        ),
        format!("{R1}{R3}")
    );
}

#[test]
fn sequence_strands_mismatch_degenerate_and_regexp_work_together() {
    let scratch = Scratch::new();
    let path = scratch.0.join("motifs.fa");
    fs::write(&path, b">r\nCCTTGGAA\n").unwrap();
    let input = path.to_str().unwrap();
    let both = scratch.run(&["grep", "-s", "-p", "AAGG", input]);
    assert!(both.status.success(), "{both:?}");
    assert_eq!(String::from_utf8_lossy(&both.stdout), ">r\nCCTTGGAA\n");
    let positive = scratch.run(&["grep", "-s", "-P", "-p", "AAGG", input]);
    assert!(positive.status.success(), "{positive:?}");
    assert!(positive.stdout.is_empty(), "{positive:?}");

    let regex = scratch.run(&["grep", "-s", "-r", "-p", "A.G", input]);
    assert!(regex.status.success(), "{regex:?}");
    assert_eq!(String::from_utf8_lossy(&regex.stdout), ">r\nCCTTGGAA\n");
    let regex_positive = scratch.run(&["grep", "-s", "-r", "-P", "-p", "A.G", input]);
    assert!(regex_positive.stdout.is_empty(), "{regex_positive:?}");

    let degenerate = scratch.run(&["grep", "-s", "-d", "-p", "KR", input]);
    assert!(degenerate.status.success(), "{degenerate:?}");
    assert!(!degenerate.stdout.is_empty());
}

#[test]
fn region_circular_and_empty_patterns_follow_seqkit() {
    let scratch = Scratch::new();
    let path = scratch.0.join("regions.fa");
    fs::write(&path, b">r\nACGTACGT\n>empty\n\n").unwrap();
    let input = path.to_str().unwrap();
    let region = scratch.run(&["grep", "-s", "-R", "-4:-1", "-p", "GT", input]);
    assert!(region.status.success(), "{region:?}");
    assert_eq!(String::from_utf8_lossy(&region.stdout), ">r\nACGTACGT\n");
    let empty = scratch.run(&["grep", "-s", "-p", "", input]);
    assert!(empty.status.success(), "{empty:?}");
    assert_eq!(String::from_utf8_lossy(&empty.stdout), ">empty\n\n");

    let circular = scratch.0.join("circular.fa");
    fs::write(&circular, b">c\nGTAC\n").unwrap();
    let output = scratch.run(&[
        "grep",
        "-s",
        "-c",
        "-r",
        "-p",
        "ACGT",
        circular.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">c\nGTAC\n");
}

#[test]
fn duplicated_patterns_and_delete_matched_control_output_multiplicity() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let input = input.to_str().unwrap();
    let duplicated = scratch.run(&["grep", "-p", "r1", "-p", "r1", "-D", input]);
    assert!(duplicated.status.success(), "{duplicated:?}");
    assert_eq!(
        String::from_utf8_lossy(&duplicated.stdout),
        format!("{R1}{R1}")
    );
    let deleted = scratch.run(&["grep", "-p", "r1", "-p", "r2", "--delete-matched", input]);
    assert!(deleted.status.success(), "{deleted:?}");
    assert_eq!(
        String::from_utf8_lossy(&deleted.stdout),
        format!("{R1}{R2}")
    );
}

#[test]
fn id_regexp_requires_a_capture_group() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let result = scratch.run(&[
        "grep",
        "-p",
        "r",
        "--id-regexp",
        "[0-9]+",
        input.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(255), "{result:?}");
    assert!(result.stdout.is_empty());
}

#[test]
fn missing_pattern_is_a_usage_error() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let result = scratch.run(&["grep", input.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(2), "{result:?}");
    assert!(result.stdout.is_empty());
}
