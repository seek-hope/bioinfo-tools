//! `subseq --gtf` regressions: 1-based features, tags and negative-strand RC.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-subseq-gtf-{}-{}",
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

    fn gtf(&self) -> PathBuf {
        let path = self.0.join("input.gtf");
        fs::write(
            &path,
            b"chr1\tsrc\tgene\t5\t7\t.\t+\t.\tgene_id \"G1\"; transcript_id \"T1\";\n\
chr1\tsrc\texon\t9\t10\t.\t-\t.\tgene_id \"G1\"; transcript_id \"T1\";\n",
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
fn gtf_features_and_negative_strand_match_seqkit() {
    let scratch = Scratch::new();
    let fasta = scratch.fasta();
    let gtf = scratch.gtf();
    let output = scratch.run(&[
        "subseq",
        "--gtf",
        gtf.to_str().unwrap(),
        fasta.to_str().unwrap(),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">chr1_5-7:+ G1\nACG\n>chr1_9-10:- G1\nGT\n"
    );

    let genes = scratch.run(&[
        "subseq",
        "--gtf",
        gtf.to_str().unwrap(),
        "--feature",
        "gene",
        fasta.to_str().unwrap(),
    ]);
    assert_eq!(
        String::from_utf8_lossy(&genes.stdout),
        ">chr1_5-7:+ G1\nACG\n"
    );
}
