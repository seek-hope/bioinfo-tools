//! SeqKit's global `--compress-level` across primary and auxiliary outputs.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("rust-fx-compression-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn input(dir: &Path) -> PathBuf {
    let path = dir.join("input.fastq");
    let text = (0..12)
        .map(|i| format!("@r{i}\nACGTACGTACGT\n+\nIIIIIIIIIIII\n"))
        .collect::<String>();
    fs::write(&path, text).unwrap();
    path
}

fn decode(path: &Path) -> Vec<u8> {
    let file = File::open(path).unwrap();
    let mut reader = bioinfo_fastx::decompress(Box::new(file)).unwrap();
    let mut data = Vec::new();
    reader.read_to_end(&mut data).unwrap();
    data
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-fx"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn numeric_levels_cover_all_compression_formats() {
    let dir = scratch("formats");
    let input = input(&dir);
    let expected = b"@r0\nACGTACGTACGT\n+\nIIIIIIIIIIII\n@r1\nACGTACGTACGT\n+\nIIIIIIIIIIII\n@r2\nACGTACGTACGT\n+\nIIIIIIIIIIII\n";
    for (extension, level) in [
        ("gz", "1"),
        ("gz", "9"),
        ("zst", "4"),
        ("bz2", "1"),
        ("xz", "0"),
    ] {
        let output = dir.join(format!("out-{extension}-{level}.{extension}"));
        let result = run(&[
            "head",
            "-n",
            "3",
            "--compress-level",
            level,
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert!(result.status.success(), "{}: {result:?}", output.display());
        assert_eq!(decode(&output), expected, "{}", output.display());
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn invalid_levels_use_seqkit_exit_code_and_leave_no_file() {
    let dir = scratch("reject");
    let input = input(&dir);
    for (extension, level, format, range) in [
        ("gz", "0", "gzip", "1-9"),
        ("gz", "10", "gzip", "1-9"),
        ("zst", "5", "zstd", "1-4"),
        ("bz2", "10", "bzip2", "1-9"),
    ] {
        let output = dir.join(format!("bad-{extension}-{level}.{extension}"));
        let result = run(&[
            "head",
            "--compress-level",
            level,
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert_eq!(result.status.code(), Some(255));
        assert!(!output.exists());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains(format), "{error}");
        assert!(error.contains(range), "{error}");
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn split_and_duplicate_reports_use_the_same_level() {
    let dir = scratch("auxiliary");
    let input = input(&dir);
    let split_dir = dir.join("parts");
    let result = run(&[
        "split",
        "-s",
        "2",
        "-O",
        split_dir.to_str().unwrap(),
        "-e",
        ".fastq.gz",
        "--compress-level",
        "1",
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    let mut parts = fs::read_dir(&split_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    parts.sort();
    assert_eq!(parts.len(), 6);
    for part in parts {
        assert!(!decode(&part).is_empty());
    }

    let deduped = dir.join("dedup.fastq.gz");
    let report = dir.join("duplicates.tsv.gz");
    let result = run(&[
        "rmdup",
        "-s",
        "-D",
        report.to_str().unwrap(),
        "-o",
        deduped.to_str().unwrap(),
        "--compress-level",
        "1",
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    assert!(!decode(&deduped).is_empty());
    assert!(!decode(&report).is_empty());
    let _ = fs::remove_dir_all(dir);
}
