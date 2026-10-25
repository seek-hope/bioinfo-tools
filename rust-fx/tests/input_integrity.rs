//! Process-level regressions for P0-01. Fixtures are generated, never oracle files.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use bioinfo_fastx::{Format, Levels, Writer};
use std::io::Write;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-integrity-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self, data: &[u8]) -> PathBuf {
        let path = self.0.join("reads.fastq");
        std::fs::write(&path, data).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str], input: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-fx"))
        .args(args)
        .arg(input)
        .output()
        .unwrap()
}

#[test]
fn shared_reader_commands_reject_an_interior_blank_instead_of_succeeding_with_a_prefix() {
    let dir = Scratch::new();
    let path = dir.input(b"@r1\nACGT\n+\nIIII\n\n@r2\nTGCA\n+\nIIII\n");
    for args in [
        vec!["fq2fa"],
        vec!["seq"],
        vec!["stats", "-T"],
        vec!["fx2tab"],
        vec!["sample", "-p", "1"],
        vec!["split", "-s", "1"],
    ] {
        let output = run(&args, &path);
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(3), "{args:?}: {output:?}");
        assert!(error.contains("line 5"), "{args:?}: {error}");
    }
}

#[test]
fn interior_blanks_are_rejected_with_crlf_gzip_and_more_than_one_blank() {
    let dir = Scratch::new();
    for newline in ["\n", "\r\n"] {
        let data = "@r1\nACGT\n+\nIIII\n\n\n@r2\nTGCA\n+\nIIII\n".replace('\n', newline);
        let path = dir.input(data.as_bytes());
        for compressed in [false, true] {
            if compressed {
                let mut writer = Writer::from_sink(
                    Box::new(std::fs::File::create(&path).unwrap()),
                    Format::Gzip,
                    Levels::DEFAULT,
                )
                .unwrap();
                writer.write_all(data.as_bytes()).unwrap();
                writer.finish().unwrap();
            }
            let output = run(&["fq2fa"], &path);
            assert_eq!(output.status.code(), Some(3), "{output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("line 5"));
        }
    }
}

#[test]
fn empty_input_and_existing_edge_padding_remain_supported() {
    let dir = Scratch::new();
    for (data, expected) in [
        ("", ""),
        ("\n\n", ""),
        ("\n@r1\nACGT\n+\nIIII\n\n\n", ">r1\nACGT\n"),
        ("@r1\r\nACGT\r\n+\r\nIIII\r\n\r\n", ">r1\nACGT\n"),
    ] {
        let path = dir.input(data.as_bytes());
        let output = run(&["fq2fa"], &path);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected.as_bytes());
    }
}

#[test]
fn valid_empty_reads_and_multiple_records_are_not_mistaken_for_blank_headers() {
    let dir = Scratch::new();
    let path = dir.input(b"@empty\n\n+\n\n@r2\nTGCA\n+\nIIII\n");
    let output = run(&["fq2fa"], &path);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b">empty\n\n>r2\nTGCA\n");
}

#[test]
fn truncated_records_and_garbage_after_padding_are_not_clean_eof() {
    let dir = Scratch::new();
    for tail in ["@r2\nTGCA\n+\n", "\n@r2\n", "\nnot-a-record\n"] {
        let data = format!("@r1\nACGT\n+\nIIII\n{tail}");
        let path = dir.input(data.as_bytes());
        let output = run(&["fq2fa"], &path);
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(!output.stderr.is_empty());
    }
}
