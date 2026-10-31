//! Explicit native-runtime exceptions to the upstream JVM launcher contract.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fastqc-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn jvm_options_fail_explicitly_before_input_or_output_access() {
    let scratch = Scratch::new();
    let missing = scratch.0.join("missing.fastq");
    for args in [
        vec!["-j", "/missing/java"],
        vec!["--java", "/missing/java"],
        vec!["--debug"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
            .args(args)
            .arg("--outdir")
            .arg(&scratch.0)
            .arg(&missing)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(result.stdout.is_empty());
        assert_eq!(String::from_utf8_lossy(&result.stderr),
            "ERROR: -j/--java and --debug configure the upstream JVM launcher; rust-fastqc is native Rust and does not launch Java; remove these options\n");
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
    }
}

#[test]
fn native_qc_runs_without_java_or_other_executables_on_path() {
    let scratch = Scratch::new();
    let input = scratch.0.join("reads.fastq");
    fs::write(&input, b"@r\nACGTAC\n+\nIIIIII\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .args(["--stdout", "--quiet"])
        .arg(&input)
        .env("PATH", &scratch.0)
        .env("JAVA_HOME", scratch.0.join("no-jvm"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let text = String::from_utf8_lossy(&result.stdout);
    assert!(text.contains("Total Sequences\t1\n"), "{text}");
    assert!(text.contains("Sequence length\t6\n"), "{text}");
}
