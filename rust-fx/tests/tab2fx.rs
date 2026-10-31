//! `tab2fx` regressions: 2-column FASTA, 3-column FASTQ and comment skipping.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-tab2fx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
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
fn two_and_three_column_tables_match_seqkit() {
    let scratch = Scratch::new();
    let fasta = scratch.0.join("a.tsv");
    fs::write(&fasta, b"r1\tACGT\nr2\tTTTT\n").unwrap();
    let fasta_out = scratch.run(&["tab2fx", fasta.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&fasta_out.stdout),
        ">r1\nACGT\n>r2\nTTTT\n"
    );

    let fastq = scratch.0.join("q.tsv");
    fs::write(&fastq, b"r1\tACGT\tIIII\nr2\tTTTT\tJJJJ\n").unwrap();
    let fastq_out = scratch.run(&["tab2fx", fastq.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&fastq_out.stdout),
        "@r1\nACGT\n+\nIIII\n@r2\nTTTT\n+\nJJJJ\n"
    );
}

#[test]
fn custom_comment_prefixes_replace_the_defaults() {
    let scratch = Scratch::new();
    let table = scratch.0.join("custom.tsv");
    fs::write(&table, b"; skip\n# keep\tACGT\nr1\tACGT\n").unwrap();
    let output = scratch.run(&["tab2fx", "-p", ";", table.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "># keep\nACGT\n>r1\nACGT\n"
    );
}

#[test]
fn comment_lines_and_unvalidated_quality_lengths_match_seqkit() {
    let scratch = Scratch::new();
    let comments = scratch.0.join("c.tsv");
    fs::write(&comments, b"# comment\n// other\nr1\tACGT\n").unwrap();
    let output = scratch.run(&["tab2fx", comments.to_str().unwrap()]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">r1\nACGT\n");

    let bad = scratch.0.join("bad.tsv");
    fs::write(&bad, b"bad\tACGT\tII\n").unwrap();
    // SeqKit tab2fx copies these fields; validation belongs to `fq lint`.
    // This is also checked against the pinned binary in seqkit_streaming_diff.py.
    let converted = scratch.run(&["tab2fx", bad.to_str().unwrap()]);
    assert!(converted.status.success());
    assert_eq!(converted.stdout, b"@bad\nACGT\n+\nII\n");
}

#[test]
fn byte_fields_extra_columns_and_per_file_fastq_state() {
    let scratch = Scratch::new();
    let first = scratch.0.join("first.tsv");
    let second = scratch.0.join("second.tsv");
    fs::write(&first, b"a\tAC\tII\nb\tAC\t\nc\tAC\tII\textra\nd\t\xffG\n").unwrap();
    fs::write(&second, b"e\tAC\t\nf\t\n").unwrap();
    let output = scratch.run(&["tab2fx", first.to_str().unwrap(), second.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"@a\nAC\n+\nII\n@b\nAC\n+\n\n>c\nAC\n>d\n\xffG\n>e\nAC\n>f\n\n"
    );
}

#[test]
fn csv_comment_prefixes_and_fractional_buffer_sizes() {
    let scratch = Scratch::new();
    let input = scratch.0.join("prefix.tsv");
    fs::write(&input, b"a,b ignored\n# ignored\nr\tAC\n").unwrap();
    let result = scratch.run(&[
        "tab2fx",
        "-p",
        "\"a,b\",#",
        "-b",
        "1.5k",
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">r\nAC\n");
    let all_skipped = scratch.run(&["tab2fx", "-p", ",;", input.to_str().unwrap()]);
    assert!(all_skipped.status.success());
    assert!(all_skipped.stdout.is_empty());
}

#[test]
fn fatal_input_keeps_committed_blocks_but_drops_pending_output() {
    let scratch = Scratch::new();
    for count in [10, 10922, 10923, 30000] {
        let input = scratch.0.join(format!("late-{count}.tsv"));
        let mut bytes = b"a\tAC\n".repeat(count);
        bytes.extend_from_slice(b"bad\n");
        fs::write(&input, bytes).unwrap();
        let output = scratch.run(&["tab2fx", input.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(255), "{output:?}");
        let expected = b">a\nAC\n".repeat(count);
        let committed = expected.len().saturating_sub(1) / 65536 * 65536;
        assert_eq!(output.stdout, expected[..committed]);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("at least two columns needed: bad")
        );
    }
}

#[test]
fn same_file_and_hardlink_outputs_are_rejected_before_truncation() {
    let scratch = Scratch::new();
    let input = scratch.0.join("input.tsv");
    let link = scratch.0.join("link.tsv");
    let data = b"a\tAC\n";
    fs::write(&input, data).unwrap();
    fs::hard_link(&input, &link).unwrap();
    for output in [&input, &link] {
        let result = scratch.run(&[
            "tab2fx",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert_eq!(result.status.code(), Some(255), "{result:?}");
        assert_eq!(fs::read(&input).unwrap(), data);
    }
}

#[test]
fn compression_roundtrips_and_early_failure_does_not_finalize() {
    use bioinfo_fastx::{reader::Writer, Levels};
    use std::io::{Read, Write};
    let scratch = Scratch::new();
    for extension in ["gz", "bz2", "xz", "zst"] {
        let input = scratch.0.join(format!("input.tsv.{extension}"));
        let mut writer = Writer::create(input.to_str().unwrap(), Levels::DEFAULT).unwrap();
        writer.write_all(b"a\tAC\n").unwrap();
        writer.finish().unwrap();
        let output = scratch.0.join(format!("out.fa.{extension}"));
        let result = scratch.run(&[
            "tab2fx",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert!(result.status.success(), "{extension}: {result:?}");
        let mut decoded =
            bioinfo_fastx::decompress(Box::new(fs::File::open(&output).unwrap())).unwrap();
        let mut bytes = Vec::new();
        decoded.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b">a\nAC\n");

        fs::write(&input, b"a\tAC\nbad\n").unwrap();
        let failed = scratch.run(&[
            "tab2fx",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert_eq!(failed.status.code(), Some(255), "{extension}: {failed:?}");
        // No write crossed the 64 KiB boundary. Dropping an encoder must not
        // create a complete (apparently successful) compressed stream.
        assert!(fs::read(&output).unwrap().is_empty());
    }
}

#[test]
fn scanner_limit_discards_uncommitted_output() {
    let scratch = Scratch::new();
    for (length, succeeds) in [(1020, true), (1021, false)] {
        let input = scratch.0.join(format!("line-{length}.tsv"));
        fs::write(&input, format!("r1\t{}\n", "A".repeat(length))).unwrap();
        let output = scratch.0.join(format!("line-{length}.fa"));
        let result = scratch.run(&[
            "tab2fx",
            "-b",
            "1K",
            "-w",
            "0",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert_eq!(result.status.success(), succeeds, "{result:?}");
        if succeeds {
            assert_eq!(
                fs::read_to_string(output).unwrap(),
                format!(">r1\n{}\n", "A".repeat(length))
            );
        } else {
            assert_eq!(result.status.code(), Some(255));
            assert_eq!(fs::read(output).unwrap(), b"");
        }
    }

    let partial = scratch.0.join("partial.tsv");
    fs::write(&partial, format!("r1\tACGT\nr2\t{}\n", "A".repeat(2000))).unwrap();
    let output = scratch.0.join("partial.fa");
    let result = scratch.run(&[
        "tab2fx",
        "-b",
        "1K",
        "-o",
        output.to_str().unwrap(),
        partial.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(fs::read(output).unwrap(), b"");

    let invalid = scratch.0.join("invalid.fa");
    let result = scratch.run(&[
        "tab2fx",
        "-b",
        "1KB",
        "-o",
        invalid.to_str().unwrap(),
        partial.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(255));
    assert!(!invalid.exists());
}
