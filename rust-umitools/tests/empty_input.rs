//! Regression tests for empty (header-only) BAM input.
//!
//! The old lazy writer never opened its output when zero records were read,
//! so `dedup` exited 0 after committing a zero-byte file. Both the direct and
//! sorted paths must emit a complete empty BAM (header + BGZF EOF).

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use noodles::bam;
use noodles::bgzf;
use noodles::sam;
use noodles::sam::alignment::RecordBuf;

const BGZF_EOF: [u8; 28] = [
    0x1f, 0x8b, 0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x06, 0x00, 0x42, 0x43, 0x02, 0x00,
    0x1b, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn scratch_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rust-umitools-empty-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("create test directory");
    path
}

fn write_empty_bam(path: &Path) {
    let mut writer = bam::io::Writer::new(File::create(path).expect("create input BAM"));
    writer
        .write_header(&sam::Header::default())
        .expect("write empty BAM header");
    writer.try_finish().expect("finish input BAM");
}

fn check_output(path: &Path) {
    let bytes = fs::read(path).expect("read output BAM");
    assert!(bytes.ends_with(&BGZF_EOF), "missing canonical BGZF EOF");

    let file = File::open(path).expect("open output BAM");
    let reader = bgzf::io::Reader::new(BufReader::new(file));
    let mut reader = bam::io::Reader::from(reader);
    let header = reader.read_header().expect("read output BAM header");
    let mut record = RecordBuf::default();
    assert_eq!(
        reader
            .read_record_buf(&header, &mut record)
            .expect("read output BAM record"),
        0,
        "expected no records"
    );
}

#[test]
fn empty_input_writes_valid_bam_on_both_paths() {
    let dir = scratch_dir();
    let input = dir.join("empty.bam");
    write_empty_bam(&input);

    let binary = env!("CARGO_BIN_EXE_rust-umitools");
    for (name, extra) in [
        ("direct.bam", vec!["--no-sort-output"]),
        ("sorted.bam", Vec::new()),
    ] {
        let output = dir.join(name);
        let result = Command::new(binary)
            .args(["dedup", "-I"])
            .arg(&input)
            .args(["-S"])
            .arg(&output)
            .args(["--quiet"])
            .args(extra)
            .output()
            .expect("run rust-umitools");
        assert!(
            result.status.success(),
            "{name} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        check_output(&output);
    }

    fs::remove_dir_all(dir).expect("remove test directory");
}
