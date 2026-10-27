//! `subseq -r` regressions: 1-based/negative regions, coordinate suffix and FASTQ.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-subseq-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nACGTACGTACGTACGT\n").unwrap();
        path
    }

    fn fastq(&self) -> PathBuf {
        let path = self.0.join("input.fastq");
        fs::write(&path, b"@q1\nACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIII\n").unwrap();
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
fn positive_negative_and_full_regions_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let input = input.to_str().unwrap();

    let first = scratch.run(&["subseq", "-r", "1:12", input]);
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        ">r1\nACGTACGTACGT\n"
    );

    let inner = scratch.run(&["subseq", "-r", "2:-2", input]);
    assert_eq!(
        String::from_utf8_lossy(&inner.stdout),
        ">r1\nCGTACGTACGTACG\n"
    );

    let negative = scratch.run(&["subseq", "-r", "-4:-2", input]);
    assert_eq!(String::from_utf8_lossy(&negative.stdout), ">r1\nACG\n");

    let all = scratch.run(&["subseq", "-r", "1:-1", input]);
    assert_eq!(
        String::from_utf8_lossy(&all.stdout),
        ">r1\nACGTACGTACGTACGT\n"
    );
}

#[test]
fn region_coord_appends_the_requested_bounds() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let output = scratch.run(&["subseq", "-r", "2:-2", "-R", input.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">r1:2--2 \nCGTACGTACGTACG\n"
    );
}

#[test]
fn fastq_sequence_and_quality_are_sliced_together() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let output = scratch.run(&["subseq", "-r", "2:-2", input.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@q1\nCGTACGTACGTACG\n+\nIIIIIIIIIIIIII\n"
    );
}
