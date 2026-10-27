//! Process-level regressions for P0-02; no external oracle or new dependencies.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use bioinfo_fastx::{Format, Levels, Parser, Writer, DEFAULT_CHUNK_SIZE};
use rust_fastqc::{stats::QcStats, Report};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
const RECORD: &[u8] = b"@r01\nACGT\n+\nIIII\n";
const FORMATS: [Format; 6] = [
    Format::Plain,
    Format::Gzip,
    Format::Bgzf,
    Format::Bzip2,
    Format::Xz,
    Format::Zstd,
];

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fastqc-integrity-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self, data: &[u8], format: Format) -> PathBuf {
        // Same name for every encoding: report bytes must agree exactly, and
        // compression must be detected from content rather than the suffix.
        let path = self.0.join("reads.fastq");
        let mut writer = Writer::from_sink(
            Box::new(std::fs::File::create(&path).unwrap()),
            format,
            Levels::DEFAULT,
        )
        .unwrap();
        writer.write_all(data).unwrap();
        writer.finish().unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(input: &Path) -> Output {
    run_args(input, &[])
}

fn run_args(input: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .arg("--stdout")
        .args(args)
        .arg(input)
        .output()
        .unwrap()
}

fn expected_report(records: usize) -> Vec<u8> {
    let mut stats = QcStats::new("reads.fastq");
    for _ in 0..records {
        stats.add_read(b"ACGT", b"IIII", 33);
    }
    Report::from_stats(&stats).render().into_bytes()
}

#[test]
fn plain_and_all_shared_compression_formats_have_identical_reports() {
    let dir = Scratch::new();
    let data = RECORD.repeat(101);
    let expected = expected_report(101);
    for format in FORMATS {
        let path = dir.input(&data, format);
        let output = run(&path);
        assert!(output.status.success(), "{format:?}: {output:?}");
        assert_eq!(output.stdout, expected, "{format:?}");
    }
}

#[test]
fn compressed_empty_inputs_match_plain_empty_input() {
    let dir = Scratch::new();
    let expected = expected_report(0);
    for format in FORMATS {
        let path = dir.input(b"", format);
        let output = run(&path);
        assert!(output.status.success(), "{format:?}: {output:?}");
        assert_eq!(output.stdout, expected, "{format:?}");
    }
}

#[test]
fn multi_member_gzip_consumes_every_member() {
    let dir = Scratch::new();
    let path = dir.input(&RECORD.repeat(60), Format::Gzip);
    let first = std::fs::read(&path).unwrap();
    dir.input(&RECORD.repeat(41), Format::Gzip);
    let second = std::fs::read(&path).unwrap();
    std::fs::write(&path, [first, second].concat()).unwrap();
    let output = run(&path);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, expected_report(101));
}

#[test]
fn records_straddling_chunks_are_not_lost_or_double_counted() {
    let dir = Scratch::new();
    let count = DEFAULT_CHUNK_SIZE * 2 / RECORD.len() + 3;
    let data = RECORD.repeat(count);
    // This record size does not divide the chunk size: refills split records.
    assert_ne!(DEFAULT_CHUNK_SIZE % RECORD.len(), 0);
    let expected = expected_report(count);
    for format in [Format::Plain, Format::Gzip] {
        let path = dir.input(&data, format);
        let output = run(&path);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
    }
}

#[test]
fn a_record_larger_than_one_chunk_is_reassembled() {
    let dir = Scratch::new();
    let mut data = b"@".to_vec();
    data.extend(std::iter::repeat_n(b'r', DEFAULT_CHUNK_SIZE + 17));
    data.extend_from_slice(b"\nACGT\n+\nIIII\n");
    for format in [Format::Plain, Format::Gzip] {
        let path = dir.input(&data, format);
        let output = run(&path);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected_report(1));
    }
}

#[test]
fn malformed_tail_after_a_chunk_keeps_global_diagnostic_positions() {
    let dir = Scratch::new();
    let mut data = RECORD.repeat(DEFAULT_CHUNK_SIZE / RECORD.len() + 3);
    data.extend_from_slice(b"@bad\nACGT\n+\nII\n");
    let mut parser = Parser::new(&data, 0, 1);
    let expected = loop {
        match parser.next_record() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("test fixture must be malformed"),
            Err(error) => break error.to_string(),
        }
    };
    for format in [Format::Plain, Format::Gzip] {
        let path = dir.input(&data, format);
        let output = run(&path);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            output.stdout.is_empty(),
            "a failed input must not emit a report"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(&expected),
            "{output:?}"
        );
    }
}

#[test]
fn malformed_or_truncated_fastq_never_emits_a_success_report() {
    let dir = Scratch::new();
    for tail in [
        b"@r2\nACGT\n+\n".as_slice(),
        b"\n@r2\nACGT\n+\nIIII\n",
        b"@r2\nACGT\n+\nIIII", // strict parser requires a final newline
    ] {
        let data = [RECORD, tail].concat();
        for format in [Format::Plain, Format::Gzip] {
            let path = dir.input(&data, format);
            let output = run(&path);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("malformed FASTQ"));
        }
    }
}

#[test]
fn corrupt_or_truncated_gzip_does_not_write_a_report_file() {
    let dir = Scratch::new();
    let path = dir.input(&RECORD.repeat(101), Format::Gzip);
    let valid = std::fs::read(&path).unwrap();
    let mut bad_crc = valid.clone();
    let crc_start = bad_crc.len() - 8;
    bad_crc[crc_start] ^= 1;
    let truncated = valid[..valid.len() - 4].to_vec();
    for bad in [bad_crc, truncated] {
        std::fs::write(&path, bad).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
            .arg("-o")
            .arg(&dir.0)
            .arg(&path)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert!(!output.stderr.is_empty());
        assert!(!dir.0.join("reads_fastqc/fastqc_data.txt").exists());
    }
}

#[test]
fn read_length_limit_fails_before_emitting_a_report() {
    let dir = Scratch::new();
    let path = dir.input(b"@r1\nACGTA\n+\nIIIII\n", Format::Plain);
    let output = run_args(&path, &["--max-read-length", "4"]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "a failed input must not emit a report"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("--max-read-length"));
}

#[test]
fn distinct_sequence_limit_allows_duplicates_and_rejects_new_prefixes() {
    let dir = Scratch::new();
    let duplicate = dir.0.join("duplicate.fastq");
    std::fs::write(&duplicate, b"@a\nACGT\n+\nIIII\n@b\nACGT\n+\nIIII\n").unwrap();
    let allowed = run_args(&duplicate, &["--max-distinct-sequences", "1"]);
    assert!(allowed.status.success(), "{allowed:?}");

    let distinct = dir.0.join("distinct.fastq");
    std::fs::write(&distinct, b"@a\nACGT\n+\nIIII\n@b\nTGCA\n+\nIIII\n").unwrap();
    let rejected = run_args(&distinct, &["--max-distinct-sequences", "1"]);
    assert_eq!(rejected.status.code(), Some(3), "{rejected:?}");
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--max-distinct-sequences"));
}

#[test]
fn compressed_report_uses_the_expected_sample_directory() {
    let dir = Scratch::new();
    let path = dir.input(RECORD, Format::Gzip);
    let gz_path = dir.0.join("reads.fastq.gz");
    std::fs::rename(path, &gz_path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .arg("--extract")
        .arg("-o")
        .arg(&dir.0)
        .arg(&gz_path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
    let report = std::fs::read_to_string(dir.0.join("reads_fastqc/fastqc_data.txt")).unwrap();
    assert!(report.contains("Filename\treads.fastq.gz\n"));
    assert!(report.contains("Total Sequences\t1\n"));
}

#[test]
fn file_output_writes_summary_html_and_zip_bundle() {
    let dir = Scratch::new();
    let path = dir.input(RECORD, Format::Plain);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .arg("--extract")
        .arg("-o")
        .arg(&dir.0)
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let base = dir.0.join("reads_fastqc");
    let summary = std::fs::read_to_string(base.join("summary.txt")).unwrap();
    assert!(
        summary.contains("PASS\tBasic Statistics\treads.fastq\n"),
        "{summary}"
    );
    let html = std::fs::read_to_string(base.join("fastqc_report.html")).unwrap();
    assert!(html.contains("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("plot images are not emitted"), "{html}");
    assert!(dir.0.join("reads_fastqc.html").is_file());
    let zip = std::fs::read(dir.0.join("reads_fastqc.zip")).unwrap();
    assert_eq!(&zip[0..4], b"PK\x03\x04");
    for name in [
        b"reads_fastqc/fastqc_data.txt".as_slice(),
        b"reads_fastqc/summary.txt".as_slice(),
        b"reads_fastqc/fastqc_report.html".as_slice(),
    ] {
        assert!(
            zip.windows(name.len()).any(|window| window == name),
            "missing {name:?}"
        );
    }
}

#[test]
fn default_output_has_html_and_zip_without_an_extracted_directory() {
    let dir = Scratch::new();
    let path = dir.input(RECORD, Format::Plain);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .arg("-o")
        .arg(&dir.0)
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(dir.0.join("reads_fastqc.html").is_file());
    assert!(dir.0.join("reads_fastqc.zip").is_file());
    assert!(!dir.0.join("reads_fastqc").exists());
}

#[test]
fn extract_delete_keeps_the_directory_and_removes_only_the_zip() {
    let dir = Scratch::new();
    let path = dir.input(RECORD, Format::Plain);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .args(["--extract", "--delete", "-o"])
        .arg(&dir.0)
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(dir.0.join("reads_fastqc/fastqc_data.txt").is_file());
    assert!(dir.0.join("reads_fastqc/summary.txt").is_file());
    assert!(dir.0.join("reads_fastqc/fastqc_report.html").is_file());
    assert!(dir.0.join("reads_fastqc.html").is_file());
    assert!(!dir.0.join("reads_fastqc.zip").exists());
}

#[test]
fn delete_without_extract_keeps_the_zip() {
    let dir = Scratch::new();
    let path = dir.input(RECORD, Format::Plain);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .args(["--delete", "-o"])
        .arg(&dir.0)
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(dir.0.join("reads_fastqc.zip").is_file());
    assert!(!dir.0.join("reads_fastqc").exists());
}

#[test]
fn adapter_content_reports_a_known_adapter() {
    let dir = Scratch::new();
    let sequence = format!("{}{}{}", "A".repeat(20), "AGATCGGAAGAG", "A".repeat(68));
    assert_eq!(sequence.len(), 100);
    let mut data = String::new();
    for index in 0..10 {
        data.push_str(&format!("@r{index}\n{sequence}\n+\n{}\n", "I".repeat(100)));
    }
    let path = dir.input(data.as_bytes(), Format::Plain);
    let output = run_args(&path, &[]);
    assert!(output.status.success(), "{output:?}");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains(">>Adapter Content"), "{report}");
    assert!(report.contains("Illumina Universal Adapter"), "{report}");
    assert!(report.contains("PolyA"), "{report}");
    assert!(report.contains("PolyG"), "{report}");
    let adapter = report
        .split(">>Adapter Content")
        .nth(1)
        .and_then(|section| section.split(">>END_MODULE").next())
        .expect("Adapter Content section");
    let line = adapter
        .lines()
        .find(|line| line.starts_with("22-23\t"))
        .expect("adapter group 22-23");
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(fields.get(1), Some(&"100.0"), "universal adapter: {line}");
    assert_eq!(fields.get(5), Some(&"100.0"), "polyA: {line}");
}

#[test]
fn nogroup_disables_long_read_position_grouping() {
    let dir = Scratch::new();
    let sequence = "ACGT".repeat(20);
    let data = format!("@long\n{sequence}\n+\n{}\n", "I".repeat(80));
    let path = dir.input(data.as_bytes(), Format::Plain);
    let output = run_args(&path, &["--nogroup"]);
    assert!(output.status.success(), "{output:?}");
    let report = String::from_utf8_lossy(&output.stdout);
    let quality = report
        .split(">>Per base sequence quality")
        .nth(1)
        .and_then(|section| section.split(">>END_MODULE").next())
        .unwrap();
    assert!(quality.contains("1\t40.0"), "{quality}");
    assert!(quality.contains("80\t40.0"), "{quality}");
    assert!(!quality.contains("10-11\t"), "{quality}");
}

#[test]
fn min_and_max_length_filter_reads_before_qc_statistics() {
    let dir = Scratch::new();
    let short = "ACGT".to_string();
    let long = "A".repeat(100);
    let data = format!(
        "@short\n{short}\n+\nIIII\n@long\n{long}\n+\n{}\n",
        "I".repeat(100)
    );
    let path = dir.input(data.as_bytes(), Format::Plain);
    let only_long = run_args(&path, &["--min_length", "50"]);
    assert!(only_long.status.success(), "{only_long:?}");
    let report = String::from_utf8_lossy(&only_long.stdout);
    assert!(report.contains("Total Sequences\t1\n"), "{report}");
    assert!(report.contains("Sequence length\t100\n"), "{report}");

    let only_short = run_args(&path, &["--max_length", "50"]);
    assert!(only_short.status.success(), "{only_short:?}");
    let report = String::from_utf8_lossy(&only_short.stdout);
    assert!(report.contains("Total Sequences\t1\n"), "{report}");
    assert!(report.contains("Sequence length\t4\n"), "{report}");
}

#[test]
fn dup_length_changes_the_duplicate_prefix_without_changing_read_counts() {
    let dir = Scratch::new();
    let data = b"@a\nAAAACCCC\n+\nIIIIIIII\n@b\nAAAAGGGG\n+\nIIIIIIII\n";
    let path = dir.input(data, Format::Plain);
    let default = run_args(&path, &[]);
    let truncated = run_args(&path, &["--dup_length", "4"]);
    assert!(default.status.success(), "{default:?}");
    assert!(truncated.status.success(), "{truncated:?}");
    let default_report = String::from_utf8_lossy(&default.stdout);
    let truncated_report = String::from_utf8_lossy(&truncated.stdout);
    assert!(default_report.contains("Total Sequences\t2\n"));
    assert!(truncated_report.contains("Total Sequences\t2\n"));
    assert_ne!(
        default_report
            .split(">>Sequence Duplication Levels")
            .nth(1)
            .and_then(|section| section.split(">>END_MODULE").next()),
        truncated_report
            .split(">>Sequence Duplication Levels")
            .nth(1)
            .and_then(|section| section.split(">>END_MODULE").next())
    );
}

#[test]
fn quiet_suppresses_progress_but_keeps_artifacts() {
    let dir = Scratch::new();
    let path = dir.input(RECORD, Format::Plain);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-fastqc"))
        .args(["--quiet", "-o"])
        .arg(&dir.0)
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(dir.0.join("reads_fastqc.zip").is_file());
}

#[test]
fn built_in_contaminants_are_used_when_no_custom_list_is_given() {
    let dir = Scratch::new();
    let sequence = "GATCGGAAGAGCTCGTATGCCGTCTTCTGCTTG";
    let data = (0..200)
        .map(|index| format!("@r{index}\n{sequence}\n+\n{}\n", "I".repeat(sequence.len())))
        .collect::<String>();
    let path = dir.input(data.as_bytes(), Format::Plain);
    let output = run_args(&path, &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Illumina Single End Adapter 1"),
        "{output:?}"
    );
}

#[test]
fn custom_contaminant_source_matches_a_tab_separated_list() {
    let dir = Scratch::new();
    let contaminant = "A".repeat(50);
    let list = dir.0.join("contaminants.tsv");
    std::fs::write(&list, format!("contam\t{contaminant}\n")).unwrap();
    let mut data = String::new();
    for index in 0..100 {
        data.push_str(&format!(
            "@r{index}\n{contaminant}\n+\n{}\n",
            "I".repeat(50)
        ));
    }
    let path = dir.input(data.as_bytes(), Format::Plain);
    let without = run_args(&path, &[]);
    assert!(without.status.success(), "{without:?}");
    assert!(String::from_utf8_lossy(&without.stdout).contains("No Hit"));

    let with = run_args(&path, &["--contaminants", list.to_str().unwrap()]);
    assert!(with.status.success(), "{with:?}");
    let report = String::from_utf8_lossy(&with.stdout);
    assert!(report.contains("contam (100% over 50bp)"), "{report}");
}
