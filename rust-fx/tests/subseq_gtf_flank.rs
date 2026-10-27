//! `subseq --gtf` flank regressions: transcript-aware up/down and only-flank.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-subseq-gtf-flank-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self) -> (PathBuf, PathBuf) {
        let fasta = self.0.join("input.fasta");
        fs::write(&fasta, b">chr1\nACGTACGTACGTACGT\n").unwrap();
        let gtf = self.0.join("input.gtf");
        fs::write(
            &gtf,
            b"chr1\tsrc\tgene\t5\t7\t.\t+\t.\tgene_id \"G1\";\n\
chr1\tsrc\texon\t9\t10\t.\t-\t.\tgene_id \"G1\";\n",
        )
        .unwrap();
        (fasta, gtf)
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
fn gtf_up_and_down_flanks_are_transcript_aware() {
    let scratch = Scratch::new();
    let (fasta, gtf) = scratch.input();
    let fasta = fasta.to_str().unwrap();
    let gtf = gtf.to_str().unwrap();

    let up = scratch.run(&["subseq", "--gtf", gtf, "-u", "2", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&up.stdout),
        ">chr1_5-7:+_us:2 G1\nGTACG\n>chr1_9-10:-_us:2 G1\nACGT\n"
    );

    let down = scratch.run(&["subseq", "--gtf", gtf, "-d", "2", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&down.stdout),
        ">chr1_5-7:+_ds:2 G1\nACGTA\n>chr1_9-10:-_ds:2 G1\nGTAC\n"
    );
}

#[test]
fn gtf_only_flank_and_negative_strand_are_supported() {
    let scratch = Scratch::new();
    let (fasta, gtf) = scratch.input();
    let fasta = fasta.to_str().unwrap();
    let gtf = gtf.to_str().unwrap();

    let only_up = scratch.run(&["subseq", "--gtf", gtf, "-u", "2", "-f", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&only_up.stdout),
        ">chr1_5-7:+_usf:2 G1\nGT\n>chr1_9-10:-_usf:2 G1\nAC\n"
    );

    let only_down = scratch.run(&["subseq", "--gtf", gtf, "-d", "2", "-f", fasta]);
    assert_eq!(
        String::from_utf8_lossy(&only_down.stdout),
        ">chr1_5-7:+_dsf:2 G1\nTA\n>chr1_9-10:-_dsf:2 G1\nAC\n"
    );
}
