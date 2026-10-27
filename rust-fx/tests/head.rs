//! `head` regressions: count/length limits, zero-length semantics, FASTA width
//! and output-file behaviour.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-head-{}-{}",
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
            b"@r1\nACGT\n+\nIIII\n@r2\nACGTAC\n+\nIIIIII\n@r3\nACGTACGT\n+\nIIIIIIII\n",
        )
        .unwrap();
        path
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(
            &path,
            b">r1\nACGTACGTACGT\n>r2\nACGTACGTACGTACGT\n>r3\nACGTACGTACGTACGTACGT\n",
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

const FIRST_TWO_FASTQ: &str = "@r1\nACGT\n+\nIIII\n@r2\nACGTAC\n+\nIIIIII\n";

#[test]
fn number_and_length_limits_match_seqkit_shape() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let input = input.to_str().unwrap();

    let two = scratch.run(&["head", "-n", "2", input]);
    assert_eq!(String::from_utf8_lossy(&two.stdout), FIRST_TWO_FASTQ);

    let length_10 = scratch.run(&["head", "-l", "10", input]);
    assert_eq!(String::from_utf8_lossy(&length_10.stdout), FIRST_TWO_FASTQ);

    let length_11 = scratch.run(&["head", "-l", "11", input]);
    assert_eq!(
        String::from_utf8_lossy(&length_11.stdout).lines().count(),
        12
    );

    let length_zero = scratch.run(&["head", "-l", "0", input]);
    assert_eq!(
        String::from_utf8_lossy(&length_zero.stdout).lines().count(),
        12
    );
}

#[test]
fn zero_number_uses_seqkits_exit_code() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let result = scratch.run(&["head", "-n", "0", input.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(255), "{result:?}");
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--number"));
}

#[test]
fn infile_list_and_raw_output_global_flags_are_supported() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let list = scratch.0.join("inputs.txt");
    fs::write(&list, format!("{}\n", input.display())).unwrap();
    let result = scratch.run(&[
        "head",
        "-n",
        "1",
        "-X",
        list.to_str().unwrap(),
        "--id-regexp",
        "^([a-z])",
        "--id-ncbi",
        "--seq-type",
        "dna",
        "--quiet",
        "--skip-file-check",
    ]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "@r1\nACGT\n+\nIIII\n"
    );
}

#[test]
fn fasta_width_zero_keeps_sequences_unwrapped() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let result = scratch.run(&["head", "-n", "1", "-w", "0", input.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        ">r1\nACGTACGTACGT\n"
    );
}

#[test]
fn output_file_contains_the_same_text() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let output = scratch.0.join("head.fastq");
    let result = scratch.run(&[
        "head",
        "-n",
        "2",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read_to_string(output).unwrap(), FIRST_TWO_FASTQ);
}
