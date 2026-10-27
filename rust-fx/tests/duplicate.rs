//! `duplicate -n` regressions: repeated records, FASTA/FASTQ and zero/one counts.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-duplicate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nAC\n>r2\nGT\n").unwrap();
        path
    }

    fn fastq(&self) -> PathBuf {
        let path = self.0.join("input.fastq");
        fs::write(&path, b"@q1\nAC\n+\nII\n").unwrap();
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
fn default_and_explicit_duplication_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let input = input.to_str().unwrap();

    let once = scratch.run(&["duplicate", input]);
    assert_eq!(String::from_utf8_lossy(&once.stdout), ">r1\nAC\n>r2\nGT\n");

    let twice = scratch.run(&["duplicate", "-n", "2", input]);
    assert_eq!(
        String::from_utf8_lossy(&twice.stdout),
        ">r1\nAC\n>r1\nAC\n>r2\nGT\n>r2\nGT\n"
    );

    let thrice = scratch.run(&["duplicate", "-n", "3", input]);
    assert_eq!(
        String::from_utf8_lossy(&thrice.stdout),
        ">r1\nAC\n>r1\nAC\n>r1\nAC\n>r2\nGT\n>r2\nGT\n>r2\nGT\n"
    );
}

#[test]
fn fastq_duplication_keeps_quality() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let output = scratch.run(&["duplicate", "-n", "2", input.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@q1\nAC\n+\nII\n@q1\nAC\n+\nII\n"
    );
}
