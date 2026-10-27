//! `depth` regressions for the supported samtools-compatible subset.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-bam-depth-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn sam(&self) -> PathBuf {
        let path = self.0.join("input.sam");
        fs::write(
            &path,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10\n\
r1\t0\tchr1\t1\t60\t3M\t*\t0\t0\tACG\tIII\n\
r2\t0\tchr1\t3\t60\t2M\t*\t0\t0\tAC\tII\n\
r3\t0\tchr1\t7\t10\t1M\t*\t0\t0\tA\t!\n\
r4\t0\tchr1\t8\t60\t1M\t*\t0\t0\tA\tI\n\
r5\t0\tchr1\t9\t60\t2M\t*\t0\t0\tAC\tII\n",
        )
        .unwrap();
        path
    }

    fn indel_sam(&self) -> PathBuf {
        let path = self.0.join("indel.sam");
        fs::write(
            &path,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:20\n\
r1\t0\tchr1\t1\t60\t2M1D2M\t*\t0\t0\tACGT\tIIII\n\
r2\t0\tchr1\t5\t60\t2M\t*\t0\t0\tAC\tII\n",
        )
        .unwrap();
        path
    }

    fn unused_reference_sam(&self) -> PathBuf {
        let path = self.0.join("unused.sam");
        fs::write(
            &path,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:3\n@SQ\tSN:chr2\tLN:2\n\
r1\t0\tchr1\t2\t60\t1M\t*\t0\t0\tA\tI\n",
        )
        .unwrap();
        path
    }

    fn overlap_sam(&self) -> PathBuf {
        let path = self.0.join("overlap.sam");
        fs::write(
            &path,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:20\n\
single\t0\tchr1\t1\t30\t20M\t*\t0\t0\tATCGTACTGTGCGCAATTCC\tAAAAAAAAAAAAAAAAAAAA\n\
pair\t67\tchr1\t5\t30\t10M\t=\t5\t10\tTACTGTGCGC\tAAAAAA!!!!\n\
pair\t131\tchr1\t5\t30\t10M\t=\t5\t10\tTACTGTGCGC\tAAAAAAAAAA\n",
        )
        .unwrap();
        path
    }

    fn flag_sam(&self) -> PathBuf {
        let path = self.0.join("flags.sam");
        fs::write(
            &path,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:20\n\
secondary\t256\tchr1\t1\t60\t2M\t*\t0\t0\tAC\tII\n\
duplicate\t1024\tchr1\t3\t60\t2M\t*\t0\t0\tAC\tII\n\
paired_reverse\t17\tchr1\t5\t60\t2M\t*\t0\t0\tAC\tII\n\
paired_read1\t65\tchr1\t7\t60\t2M\t*\t0\t0\tAC\tII\n\
proper_read1\t67\tchr1\t9\t60\t2M\t*\t0\t0\tAC\tII\n\
proper_read2\t131\tchr1\t11\t60\t2M\t*\t0\t0\tAC\tII\n\
plain\t0\tchr1\t13\t60\t2M\t*\t0\t0\tAC\tII\n\
unmapped\t4\t*\t0\t0\t*\t*\t0\t0\tA\tI\n",
        )
        .unwrap();
        path
    }

    fn bed(&self) -> PathBuf {
        let path = self.0.join("regions.bed");
        fs::write(&path, b"chr1\t1\t3\nchr1\t5\t8\n").unwrap();
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rust-bam"))
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

fn expected_default() -> String {
    "chr1\t1\t1\nchr1\t2\t1\nchr1\t3\t2\nchr1\t4\t1\nchr1\t7\t1\nchr1\t8\t1\nchr1\t9\t1\nchr1\t10\t1\n".to_string()
}

#[test]
fn default_depth_matches_samtools_shape() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let result = scratch.run(&["depth", input.to_str().unwrap()]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected_default());
}

#[test]
fn all_positions_and_header_match_samtools_shape() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input_text = input.to_str().unwrap();
    let result = scratch.run(&["depth", "-a", "-H", input_text]);
    assert!(result.status.success(), "{result:?}");
    let text = String::from_utf8_lossy(&result.stdout);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(format!("#CHROM\tPOS\t{input_text}").as_str())
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        vec![
            "chr1\t1\t1",
            "chr1\t2\t1",
            "chr1\t3\t2",
            "chr1\t4\t1",
            "chr1\t5\t0",
            "chr1\t6\t0",
            "chr1\t7\t1",
            "chr1\t8\t1",
            "chr1\t9\t1",
            "chr1\t10\t1",
        ]
    );
}

#[test]
fn quality_mapq_and_read_length_filters() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input_text = input.to_str().unwrap();

    let base_quality = scratch.run(&["depth", "-q", "30", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&base_quality.stdout),
        "chr1\t1\t1\nchr1\t2\t1\nchr1\t3\t2\nchr1\t4\t1\nchr1\t7\t0\nchr1\t8\t1\nchr1\t9\t1\nchr1\t10\t1\n"
    );

    let mapping_quality = scratch.run(&["depth", "-Q", "30", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&mapping_quality.stdout),
        "chr1\t1\t1\nchr1\t2\t1\nchr1\t3\t2\nchr1\t4\t1\nchr1\t8\t1\nchr1\t9\t1\nchr1\t10\t1\n"
    );

    let read_length = scratch.run(&["depth", "-l", "3", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&read_length.stdout),
        "chr1\t1\t1\nchr1\t2\t1\nchr1\t3\t1\n"
    );
}

#[test]
fn deletions_and_regions_match_samtools_shape() {
    let scratch = Scratch::new();
    let input = scratch.indel_sam();
    let input_text = input.to_str().unwrap();

    let default = scratch.run(&["depth", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&default.stdout),
        "chr1\t1\t1\nchr1\t2\t1\nchr1\t3\t0\nchr1\t4\t1\nchr1\t5\t2\nchr1\t6\t1\n"
    );

    let with_deletions = scratch.run(&["depth", "-J", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&with_deletions.stdout),
        "chr1\t1\t1\nchr1\t2\t1\nchr1\t3\t1\nchr1\t4\t1\nchr1\t5\t2\nchr1\t6\t1\n"
    );

    let region = scratch.run(&["depth", "-r", "chr1:2-4", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&region.stdout),
        "chr1\t2\t1\nchr1\t3\t0\nchr1\t4\t1\n"
    );

    let region_deletions = scratch.run(&["depth", "-r", "chr1:2-4", "-J", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&region_deletions.stdout),
        "chr1\t2\t1\nchr1\t3\t1\nchr1\t4\t1\n"
    );
}

#[test]
fn multiple_inputs_have_one_depth_column_each() {
    let scratch = Scratch::new();
    let first = scratch.0.join("first.sam");
    let second = scratch.0.join("second.sam");
    fs::write(
        &first,
        b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10\n\
r1\t0\tchr1\t1\t60\t3M\t*\t0\t0\tACG\tIII\n",
    )
    .unwrap();
    fs::write(
        &second,
        b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10\n\
r2\t0\tchr1\t2\t60\t2M\t*\t0\t0\tAC\tII\n",
    )
    .unwrap();
    let result = scratch.run(&["depth", first.to_str().unwrap(), second.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "chr1\t1\t1\t0\nchr1\t2\t1\t1\nchr1\t3\t1\t1\n"
    );

    let header = scratch.run(&[
        "depth",
        "-H",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    let list = scratch.0.join("inputs.txt");
    fs::write(
        &list,
        format!("{}\n{}\n", first.display(), second.display()),
    )
    .unwrap();
    let listed = scratch.run(&["depth", "-H", "-f", list.to_str().unwrap()]);
    assert!(listed.status.success(), "{listed:?}");
    assert_eq!(listed.stdout, header.stdout);
    let text = String::from_utf8_lossy(&header.stdout);
    let first_line = text.lines().next().unwrap();
    assert_eq!(
        first_line,
        format!("#CHROM\tPOS\t{}\t{}", first.display(), second.display())
    );
}

#[test]
fn bed_intervals_match_samtools_shape() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let bed = scratch.bed();
    let result = scratch.run(&[
        "depth",
        "-b",
        bed.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "chr1\t2\t1\nchr1\t3\t2\nchr1\t7\t1\nchr1\t8\t1\n"
    );

    let conflict = scratch.run(&[
        "depth",
        "-b",
        bed.to_str().unwrap(),
        "-r",
        "chr1:1-2",
        input.to_str().unwrap(),
    ]);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("mutually exclusive"));
}

#[test]
fn all_positions_distinguishes_used_and_unused_references() {
    let scratch = Scratch::new();
    let input = scratch.unused_reference_sam();
    let input = input.to_str().unwrap();

    let all = scratch.run(&["depth", "-a", input]);
    assert!(all.status.success(), "{all:?}");
    assert_eq!(
        String::from_utf8_lossy(&all.stdout),
        "chr1\t1\t0\nchr1\t2\t1\nchr1\t3\t0\n"
    );

    let absolute = scratch.run(&["depth", "-aa", input]);
    let repeated = scratch.run(&["depth", "-a", "-a", input]);
    let expected = "chr1\t1\t0\nchr1\t2\t1\nchr1\t3\t0\nchr2\t1\t0\nchr2\t2\t0\n";
    assert!(absolute.status.success(), "{absolute:?}");
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(String::from_utf8_lossy(&absolute.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&repeated.stdout), expected);

    let empty_region = scratch.run(&["depth", "-a", "-r", "chr2:1-2", input]);
    assert!(empty_region.status.success(), "{empty_region:?}");
    assert_eq!(
        String::from_utf8_lossy(&empty_region.stdout),
        "chr2\t1\t0\nchr2\t2\t0\n"
    );
}

#[test]
fn overlap_removal_counts_one_mate_and_preserves_quality_filters() {
    let scratch = Scratch::new();
    let input = scratch.overlap_sam();
    let input = input.to_str().unwrap();

    let default = scratch.run(&["depth", input]);
    let mut expected = String::new();
    for position in 1..=20 {
        let depth = if (5..=14).contains(&position) { 3 } else { 1 };
        expected.push_str(&format!("chr1\t{position}\t{depth}\n"));
    }
    assert!(default.status.success(), "{default:?}");
    assert_eq!(String::from_utf8_lossy(&default.stdout), expected);

    let deduplicated = scratch.run(&["depth", "-s", input]);
    let mut expected = String::new();
    for position in 1..=20 {
        let depth = if (5..=14).contains(&position) { 2 } else { 1 };
        expected.push_str(&format!("chr1\t{position}\t{depth}\n"));
    }
    assert!(deduplicated.status.success(), "{deduplicated:?}");
    assert_eq!(String::from_utf8_lossy(&deduplicated.stdout), expected);

    let quality = scratch.run(&["depth", "-s", "-q", "30", input]);
    let mut expected = String::new();
    for position in 1..=20 {
        let depth = if (5..=10).contains(&position) { 2 } else { 1 };
        expected.push_str(&format!("chr1\t{position}\t{depth}\n"));
    }
    assert!(quality.status.success(), "{quality:?}");
    assert_eq!(String::from_utf8_lossy(&quality.stdout), expected);
}

#[test]
fn flag_filters_match_samtools() {
    let scratch = Scratch::new();
    let input = scratch.flag_sam();
    let input = input.to_str().unwrap();
    let cases: &[(&[&str], std::ops::RangeInclusive<usize>)] = &[
        (&["-g", "DUP"], 3..=14),
        (&["-G", "PAIRED"], 13..=14),
        (&["--incl-flags", "PAIRED"], 5..=12),
        (&["--require-flags", "PAIRED,PROPER_PAIR"], 9..=12),
        (&["--require-flags", "PAIRED,REVERSE"], 5..=6),
    ];
    for (args, positions) in cases {
        let mut command = vec!["depth"];
        command.extend_from_slice(args);
        command.push(input);
        let output = scratch.run(&command);
        assert!(output.status.success(), "{args:?}: {output:?}");
        let expected = positions
            .clone()
            .map(|position| format!("chr1\t{position}\t1\n"))
            .collect::<String>();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{args:?}"
        );
    }
}

#[test]
fn output_file_contains_the_same_text() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let output = scratch.0.join("depth.tsv");
    let result = scratch.run(&[
        "depth",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read_to_string(output).unwrap(), expected_default());
}
