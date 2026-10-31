//! Adapter report semantics that the general QC oracle does not compare.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fastqc-adapters-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, sequence: &str, config: Option<&str>) -> Output {
        let input = self.0.join("reads.fq");
        fs::write(
            &input,
            format!("@r\n{sequence}\n+\n{}\n", "!".repeat(sequence.len())),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"));
        command.args(["--stdout", "--quiet"]);
        if let Some(config) = config {
            let path = self.0.join("adapters.tsv");
            fs::write(&path, config).unwrap();
            command.arg("--adapters").arg(path);
        }
        command.arg(input).output().unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn block(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    let section = text.split_once(">>Adapter Content\t").unwrap().1;
    section.split_once(">>END_MODULE\n").unwrap().0.to_owned()
}

#[test]
fn short_reads_warn_without_an_invented_zero_table() {
    let scratch = Scratch::new();
    assert_eq!(block(&scratch.run("ACGT", None)), "warn\n");
}

#[test]
fn a_complete_adapter_matches_an_equal_length_read() {
    let scratch = Scratch::new();
    for ending in ["\n", "\r", "\r\n"] {
        let config = format!("motif\tACG{ending}");
        let result = scratch.run("ACG", Some(&config));
        assert_eq!(block(&result), "fail\n#Position\tmotif\n1\t100.0\n");
        let result = scratch.run("TTT", Some(&config));
        assert_eq!(block(&result), "pass\n#Position\tmotif\n1\t0.0\n");
    }
}

#[test]
fn equal_length_adapter_hits_do_not_depend_on_read_order() {
    let reads = [b"AGATCGGAAGAG".as_slice(), b"TTTTTTTTTTTTTTTTTTTTTTTTT"];
    let mut forward = rust_fastqc::QcStats::new("forward");
    let mut reverse = rust_fastqc::QcStats::new("reverse");
    for read in reads {
        forward.add_read(read, &vec![b'!'; read.len()], 33);
    }
    for read in reads.into_iter().rev() {
        reverse.add_read(read, &vec![b'!'; read.len()], 33);
    }
    let first = rust_fastqc::modules::adapter_content(&forward);
    let second = rust_fastqc::modules::adapter_content(&reverse);
    assert_eq!(first, second);
    assert_eq!(first.lines[1], "1\t50.0\t0.0\t0.0\t0.0\t0.0\t0.0");
}

#[test]
fn mixed_adapter_lengths_preserve_hits_when_the_reported_range_grows() {
    let reads = [b"TTTTAA".as_slice(), b"TTTTTTTT"];
    let adapters = rust_fastqc::adapters::parse("short\tAA\nlong\tCCCC\n")
        .unwrap()
        .0;
    let mut forward = rust_fastqc::QcStats::new("forward");
    let mut reverse = rust_fastqc::QcStats::new("reverse");
    forward.set_adapters(adapters.clone());
    reverse.set_adapters(adapters);
    for read in reads {
        forward.add_read(read, &vec![b'!'; read.len()], 33);
    }
    for read in reads.into_iter().rev() {
        reverse.add_read(read, &vec![b'!'; read.len()], 33);
    }
    let first = rust_fastqc::modules::adapter_content(&forward);
    let second = rust_fastqc::modules::adapter_content(&reverse);
    assert_eq!(first, second);
    assert_eq!(first.lines.last().unwrap(), "5\t50.0\t0.0");
}

#[test]
fn mixed_length_custom_adapters_use_the_longest_motif_boundary() {
    let scratch = Scratch::new();
    let result = scratch.run("TTTTAA", Some("short\tAA\nlong\tCCCC\n"));
    assert_eq!(
        block(&result),
        "pass\n#Position\tshort\tlong\n1\t0.0\t0.0\n2\t0.0\t0.0\n3\t0.0\t0.0\n"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("different lengths"));
}

#[test]
fn an_empty_adapter_config_fails_before_creating_artifacts() {
    let scratch = Scratch::new();
    let config = scratch.0.join("empty.tsv");
    fs::write(&config, "# no usable entries\n").unwrap();
    let outdir = scratch.0.join("output");
    fs::create_dir(&outdir).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .arg("--adapters")
        .arg(&config)
        .arg("--outdir")
        .arg(&outdir)
        .arg(scratch.0.join("unread-input.fq"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3), "{result:?}");
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no usable name<TAB>sequence entries"));
    assert_eq!(fs::read_dir(outdir).unwrap().count(), 0);
}
