//! `restart -i` regressions: positive/negative circular start positions.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-restart-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nACGTACGT\n").unwrap();
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
fn start_with_searches_both_strands_and_rotates_qualities() {
    let scratch = Scratch::new();
    let input = scratch.0.join("circular.fastq");
    fs::write(&input, b"@seq1\natggcCACTG\n+\nABCDEFGHIJ\n").unwrap();
    let input = input.to_str().unwrap();
    let forward = scratch.run(&["restart", "-I", "-s", "GGCC", input]);
    assert!(forward.status.success(), "{forward:?}");
    assert_eq!(
        String::from_utf8_lossy(&forward.stdout),
        "@seq1\nggcCACTGat\n+\nCDEFGHIJAB\n"
    );
    let reverse = scratch.run(&["restart", "-I", "-s", "AGTG", input]);
    assert!(reverse.status.success(), "{reverse:?}");
    assert_eq!(
        String::from_utf8_lossy(&reverse.stdout),
        "@seq1\nAGTGgccatC\n+\nIHGFEDCBAJ\n"
    );
    let mismatch = scratch.run(&["restart", "-I", "-s", "GGCCT", "-m", "1", input]);
    assert!(mismatch.status.success(), "{mismatch:?}");
    assert_eq!(
        String::from_utf8_lossy(&mismatch.stdout),
        "@seq1\nggcCACTGat\n+\nCDEFGHIJAB\n"
    );
    let missing = scratch.run(&["restart", "-s", "TTTT", input]);
    assert!(missing.status.success(), "{missing:?}");
    assert!(missing.stdout.is_empty(), "{missing:?}");
}

#[test]
fn positive_and_negative_starts_rotate_circularly() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    for (start, expected) in [
        ("1", "ACGTACGT"),
        ("3", "GTACGTAC"),
        ("5", "ACGTACGT"),
        ("-2", "GTACGTAC"),
    ] {
        let output = scratch.run(&["restart", "-i", start, input]);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!(">r1\n{expected}\n"),
            "start {start}"
        );
    }
}
