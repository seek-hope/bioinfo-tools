//! Large-input regressions for the streaming `stats`/`split` paths.
//! Inputs are generated here; no oracle or fixture is modified.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rust_fx::table;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-streaming-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fastq(&self, records: usize) -> (PathBuf, Vec<u64>) {
        let path = self.0.join("reads.fastq");
        let mut text = String::new();
        let mut lengths = Vec::with_capacity(records);
        for index in 0..records {
            // A non-trivial length distribution: repeats, a dominant long read
            // and empty reads are all represented.
            let length = if index % 97 == 0 {
                0
            } else if index % 211 == 0 {
                250
            } else {
                1 + (index * 37) % 100
            };
            let sequence: String = "ACGT".chars().cycle().take(length).collect();
            let qualities = "I".repeat(length);
            text.push_str(&format!("@r{index}\n{sequence}\n+\n{qualities}\n"));
            lengths.push(length as u64);
        }
        fs::write(&path, text).unwrap();
        (path, lengths)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-fx"))
        .args(args)
        .output()
        .unwrap()
}

fn part_files(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn stats_all_matches_exact_sorted_statistics_on_a_large_input() {
    let scratch = Scratch::new();
    let (path, lengths) = scratch.fastq(5000);
    let output = run(&["stats", "-a", "-T", path.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(rows.len(), 2, "{text}");
    let fields: Vec<&str> = rows[1].split('\t').collect();
    assert_eq!(fields[3], "5000");

    let mut n50_input = lengths.clone();
    let (n50, l50) = table::n50(&mut n50_input);
    let mut quartile_input = lengths.clone();
    let (q1, q2, q3) = table::quartiles(&mut quartile_input);
    assert_eq!(fields[8], q1.to_string());
    assert_eq!(fields[9], q2.to_string());
    assert_eq!(fields[10], q3.to_string());
    assert_eq!(fields[12], n50.to_string());
    assert_eq!(fields[13], l50.to_string());
    assert_eq!(
        fields[4],
        lengths.iter().sum::<u64>().to_string(),
        "sum_len"
    );
    assert_eq!(
        fields[6],
        format!("{:.1}", lengths.iter().sum::<u64>() as f64 / 5000.0)
    );
}

#[test]
fn split_by_size_streams_every_record_in_order() {
    let scratch = Scratch::new();
    let (path, _) = scratch.fastq(5000);
    let outdir = scratch.0.join("by-size");
    let output = run(&[
        "split",
        "-s",
        "137",
        "-O",
        outdir.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("[INFO] write"));

    let files = part_files(&outdir);
    assert_eq!(files.len(), 5000_usize.div_ceil(137), "{files:?}");
    let mut joined = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let bytes = fs::read(file).unwrap();
        let records = bytes.iter().filter(|&&byte| byte == b'@').count();
        let expected = if index + 1 == files.len() {
            5000 - index * 137
        } else {
            137
        };
        assert_eq!(records, expected, "{file:?}");
        joined.extend(bytes);
    }
    assert_eq!(joined, fs::read(&path).unwrap());
}

#[test]
fn split_by_part_streams_every_record_in_order() {
    let scratch = Scratch::new();
    let (path, _) = scratch.fastq(5000);
    let outdir = scratch.0.join("by-part");
    let output = run(&[
        "split",
        "-p",
        "7",
        "-O",
        outdir.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");

    let files = part_files(&outdir);
    assert_eq!(files.len(), 7, "{files:?}");
    let mut joined = Vec::new();
    for file in &files {
        joined.extend(fs::read(file).unwrap());
    }
    assert_eq!(joined, fs::read(&path).unwrap());

    let counts: Vec<usize> = files
        .iter()
        .map(|file| {
            fs::read(file)
                .unwrap()
                .iter()
                .filter(|&&byte| byte == b'@')
                .count()
        })
        .collect();
    // 5000 = 2 * 715 + 5 * 714; earlier parts receive the remainder.
    assert_eq!(counts, vec![715, 715, 714, 714, 714, 714, 714]);
}

#[test]
fn empty_input_produces_no_parts_and_no_directory() {
    let scratch = Scratch::new();
    let path = scratch.0.join("empty.fastq");
    fs::write(&path, b"").unwrap();
    let outdir = scratch.0.join("empty-out");
    let output = run(&[
        "split",
        "-s",
        "10",
        "-O",
        outdir.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(!outdir.exists());
}
