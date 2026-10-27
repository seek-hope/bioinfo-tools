//! `subseq --bed` regressions: 0-based intervals, flanks and chromosome filter.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-subseq-bed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">chr1\nACGTACGTACGTACGT\n").unwrap();
        path
    }

    fn bed(&self) -> PathBuf {
        let path = self.0.join("input.bed");
        fs::write(&path, b"chr1\t4\t7\n").unwrap();
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
fn bed_interval_and_chromosome_filter() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let bed = scratch.bed();
    let output = scratch.run(&[
        "subseq",
        "--bed",
        bed.to_str().unwrap(),
        "--chr",
        "chr1",
        fasta.to_str().unwrap(),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">chr1_5-7:. \nACG\n"
    );
}

#[test]
fn upstream_and_downstream_flanks_match_seqkit() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let bed = scratch.bed();
    let bed = bed.to_str().unwrap();
    let fasta = fasta.to_str().unwrap();

    let up = scratch.run(&["subseq", "--bed", bed, "-u", "2", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&up.stdout),
        ">chr1_5-7:._us:2 \nGTACG\n"
    );

    let down = scratch.run(&["subseq", "--bed", bed, "-d", "2", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&down.stdout),
        ">chr1_5-7:._ds:2 \nACGTA\n"
    );

    let only_up = scratch.run(&["subseq", "--bed", bed, "-u", "2", "-f", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&only_up.stdout),
        ">chr1_5-7:._usf:2 \nGT\n"
    );

    let only_down = scratch.run(&["subseq", "--bed", bed, "-d", "2", "-f", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&only_down.stdout),
        ">chr1_5-7:._dsf:2 \nTA\n"
    );
}
