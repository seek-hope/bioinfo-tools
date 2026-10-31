//! Limits validation precedence, module visibility and per-file k-mer state.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fastqc-limits-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, data: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, data).unwrap();
        path
    }
    fn run(&self, config: &str, files: &[PathBuf]) -> Output {
        let limits = self.write("limits.txt", config);
        Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
            .args(["--stdout", "--quiet", "--limits"])
            .arg(limits)
            .args(files)
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
fn malformed_config_precedes_input_access_and_output() {
    let scratch = Scratch::new();
    let result = scratch.run("kmer ignore\n", &[scratch.0.join("missing.fq")]);
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("three fields"));
}

#[test]
fn missing_config_fails_without_a_report() {
    let scratch = Scratch::new();
    let input = scratch.write("input.fq", "@r\nACGT\n+\n5555\n");
    let result = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .arg("--limits")
        .arg(scratch.0.join("missing.txt"))
        .arg("--outdir")
        .arg(&scratch.0)
        .arg(input)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[test]
fn ignoring_overrepresentation_keeps_duplication_available() {
    let scratch = Scratch::new();
    let input = scratch.write("input.fq", &"@r\nACGT\n+\n5555\n".repeat(20));
    let result = scratch.run("overrepresented ignore 1\nkmer ignore 1\n", &[input]);
    assert!(result.status.success(), "{result:?}");
    let report = String::from_utf8(result.stdout).unwrap();
    assert!(!report.contains(">>Overrepresented sequences"));
    assert!(report.contains(">>Sequence Duplication Levels\tfail"));
}

#[test]
fn kmer_sampling_state_is_per_file() {
    let scratch = Scratch::new();
    let sequence = format!("{}{}", "A".repeat(7), "C".repeat(99));
    let record = format!("@r\n{sequence}\n+\n{}\n", "5".repeat(106));
    let first = scratch.write("first.fq", &record.repeat(49));
    let second = scratch.write("second.fq", &record.repeat(200));
    let output = scratch.run("kmer ignore 0\n", &[first, second]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let reports: Vec<_> = text.split("##FastQC\t0.13.0\n").skip(1).collect();
    assert_eq!(reports.len(), 2);
    assert!(reports[0].contains(">>Kmer Content\tpass\n>>END_MODULE"));
    assert!(reports[1].contains("AAAAAAA\t200\t1.6384E-4\t100.0\t1\n"));
}

#[test]
fn zero_length_warning_and_error_switches_reach_the_library() {
    use rust_fastqc::{
        config::FastqcConfig,
        modules::{sequence_length_distribution, Status},
        QcStats,
    };
    let mut stats = QcStats::new("empty.fq");
    stats.add_read(b"", b"", 33);
    stats.add_read(b"A", b"5", 33);
    assert_eq!(sequence_length_distribution(&stats).status, Status::Fail);
    stats.set_config(FastqcConfig::parse("sequence_length error 0\n").unwrap());
    assert_eq!(sequence_length_distribution(&stats).status, Status::Warn);
    stats.set_config(
        FastqcConfig::parse("sequence_length error 0\nsequence_length warn 0\n").unwrap(),
    );
    assert_eq!(sequence_length_distribution(&stats).status, Status::Pass);
}
