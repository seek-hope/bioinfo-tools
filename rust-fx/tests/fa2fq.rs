//! `fa2fq` regressions: FASTA names select corresponding FASTQ records.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-fa2fq-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nACGT\n>r3\nTTTT\n").unwrap();
        path
    }

    fn fastq(&self) -> PathBuf {
        let path = self.0.join("input.fastq");
        fs::write(
            &path,
            b"@r1\nACGT\n+\nIIII\n@r2\nGGGG\n+\nJJJJ\n@r3\nTTTT\n+\nKKKK\n",
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

#[test]
fn fasta_names_select_fastq_records() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let fastq = scratch.fastq();
    let output = scratch.run(&[
        "fa2fq",
        "-f",
        fasta.to_str().unwrap(),
        fastq.to_str().unwrap(),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@r1\nACGT\n+\nIIII\n@r3\nTTTT\n+\nKKKK\n"
    );
}

#[test]
fn subsequence_and_reverse_complement_matches_include_quality_orientation() {
    let scratch = Scratch::new();
    let fastq = scratch.0.join("reads.fastq");
    fs::write(
        &fastq,
        b"@r1\nAAGGTTCC\n+\nABCDEFGH\n@r2\nCCTTGGAA\n+\nIJKLMNOP\n",
    )
    .unwrap();
    let fasta = scratch.0.join("queries.fa");
    fs::write(&fasta, b">r1\nCCTT\n>r2\nAAGG\n").unwrap();
    let output = scratch.run(&[
        "fa2fq",
        "-f",
        fasta.to_str().unwrap(),
        fastq.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@r1\nCCTT\n+\nDCBA\n@r2\nAAGG\n+\nLKJI\n"
    );
    let positive = scratch.run(&[
        "fa2fq",
        "-P",
        "-f",
        fasta.to_str().unwrap(),
        fastq.to_str().unwrap(),
    ]);
    assert!(positive.status.success(), "{positive:?}");
    assert!(positive.stdout.is_empty(), "{positive:?}");
}

#[test]
fn only_positive_strand_flag_is_accepted() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let fastq = scratch.fastq();
    let output = scratch.run(&[
        "fa2fq",
        "-P",
        "-f",
        fasta.to_str().unwrap(),
        fastq.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@r1\nACGT\n+\nIIII\n@r3\nTTTT\n+\nKKKK\n"
    );
}
