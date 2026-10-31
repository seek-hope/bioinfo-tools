//! Native tile report wiring, filtering and per-file isolation.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fastqc-tile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn input(&self, name: &str, data: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, data).unwrap();
        path
    }
    fn run(&self, paths: &[PathBuf], options: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
            .args(["--stdout", "--quiet"])
            .args(options)
            .args(paths)
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
fn optional_tile_state_is_reset_for_each_input() {
    let scratch = Scratch::new();
    let first = scratch.input("first.fq", "@ordinary\nACGT\n+\n5555\n");
    let second = scratch.input(
        "second.fq",
        "@I:1:F:1:1:0:0\nACGT\n+\n5555\n@I:1:F:1:2:0:0\nACGTACGT\n+\n55555555\n",
    );
    let output = scratch.run(&[first, second], &[]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let reports: Vec<_> = text.split("##FastQC\t0.13.0\n").skip(1).collect();
    assert_eq!(reports.len(), 2);
    assert!(!reports[0].contains(">>Per tile sequence quality"));
    assert!(reports[1].contains(">>Per tile sequence quality\tpass\n#Tile\tBase\tMean\n"));
    assert!(reports[1].contains("1\t5\tNaN\n"));
    assert!(reports[1].contains("2\t5\t0.0\n"));
    assert!(
        reports[1].find(">>Per base sequence quality").unwrap()
            < reports[1].find(">>Per tile sequence quality").unwrap()
    );
    assert!(
        reports[1].find(">>Per tile sequence quality").unwrap()
            < reports[1].find(">>Per sequence quality scores").unwrap()
    );
}

#[test]
fn length_filtering_precedes_header_detection() {
    let scratch = Scratch::new();
    let input = scratch.input(
        "reads.fq",
        "@ordinary\nA\n+\n!\n@I:1:F:1:1:0:0\nACGT\n+\n5555\n",
    );
    let output = scratch.run(&[input], &["--min_length", "4"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Total Sequences\t1\n"));
    assert!(text.contains(">>Per tile sequence quality\tpass\n"));
}

#[test]
fn invalid_phred64_is_rejected_before_report_creation() {
    let scratch = Scratch::new();
    let input = scratch.input("reads.fq", "@I:1:F:1:1:0:0\nACGT\n+\n!!!!\n");
    let output = scratch.run(&[input], &["--phred64"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Phred64"));
}

#[test]
fn malformed_tail_does_not_emit_accumulated_tile_results() {
    let scratch = Scratch::new();
    let input = scratch.input(
        "reads.fq",
        "@I:1:F:1:1:0:0\nACGT\n+\n5555\n@bad\nACGT\n+\n5\n",
    );
    let output = scratch.run(&[input], &[]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
