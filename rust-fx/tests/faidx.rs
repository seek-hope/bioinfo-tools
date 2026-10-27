//! `faidx` regressions: positional regions, full headers, case and region files.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-faidx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">chr1 desc\nACGTACGTACGT\n").unwrap();
        path
    }

    fn regions(&self) -> PathBuf {
        let path = self.0.join("regions.txt");
        fs::write(&path, b"chr1:2-5\nchr1:7-9\n").unwrap();
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
fn positional_regions_and_region_file_match_seqkit() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let fasta = fasta.to_str().unwrap();

    let one = scratch.run(&["faidx", fasta, "chr1:2-5"]);
    assert_eq!(String::from_utf8_lossy(&one.stdout), ">chr1:2-5\nCGTA\n");

    let two = scratch.run(&["faidx", fasta, "chr1:2-5", "chr1:7-9"]);
    assert_eq!(
        String::from_utf8_lossy(&two.stdout),
        ">chr1:2-5\nCGTA\n>chr1:7-9\nGTA\n"
    );

    let regions = scratch.regions();
    let from_file = scratch.run(&["faidx", "-l", regions.to_str().unwrap(), fasta]);
    assert_eq!(
        String::from_utf8_lossy(&from_file.stdout),
        ">chr1:2-5\nCGTA\n>chr1:7-9\nGTA\n"
    );
}

#[test]
fn regexp_ids_extract_full_records_and_create_fai() {
    let scratch = Scratch::new();
    let input = scratch.0.join("records.fa");
    fs::write(&input, b">chr1\nACGTACGT\n>chr2\nTTTTAAAA\n>other\nGGGG\n").unwrap();
    let output = scratch.run(&["faidx", input.to_str().unwrap(), "-r", "chr"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">chr1\nACGTACGT\n>chr2\nTTTTAAAA\n"
    );
    assert!(PathBuf::from(format!("{}.fai", input.display())).is_file());
}

#[test]
fn full_head_uses_seqkit_index_and_update_rebuilds_a_stale_fai() {
    let scratch = Scratch::new();
    let input = scratch.0.join("records.fa");
    fs::write(&input, b">chr1\nACGTACGT\n>chr2\nTTTTAAAA\n").unwrap();
    let output = scratch.run(&["faidx", "-f", input.to_str().unwrap(), "chr1:2-4"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">chr1:2-4\nCGT\n");
    assert!(PathBuf::from(format!("{}.seqkit.fai", input.display())).is_file());

    let default_index = PathBuf::from(format!("{}.fai", input.display()));
    fs::write(&default_index, "chr1\t999\t0\t1\t1\n").unwrap();
    let updated = scratch.run(&["faidx", "-U", input.to_str().unwrap(), "chr1:2-4"]);
    assert!(updated.status.success(), "{updated:?}");
    assert_eq!(String::from_utf8_lossy(&updated.stdout), ">chr1:2-4\nCGT\n");
    assert!(fs::read_to_string(default_index)
        .unwrap()
        .contains("chr1\t8\t"));
}

#[test]
fn full_header_and_case_insensitive_id_match_seqkit() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let fasta = fasta.to_str().unwrap();

    let full = scratch.run(&["faidx", "-f", fasta, "chr1:2-5"]);
    assert_eq!(
        String::from_utf8_lossy(&full.stdout),
        ">chr1 desc:2-5\nCGTA\n"
    );

    let case = scratch.run(&["faidx", "-i", fasta, "Chr1:2-5"]);
    assert_eq!(String::from_utf8_lossy(&case.stdout), ">chr1:2-5\nCGTA\n");
}
