//! `coverage` regressions for the supported samtools-compatible subset.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-bam-coverage-{}-{}",
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
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:30\n\
r1\t0\tchr1\t1\t60\t3M\t*\t0\t0\tACG\tIII\n\
r2\t0\tchr1\t10\t20\t3M\t*\t0\t0\tACG\t!!!\n",
        )
        .unwrap();
        path
    }

    fn multiple_inputs(&self) -> (PathBuf, PathBuf, PathBuf) {
        let first = self.0.join("first.sam");
        let second = self.0.join("second.sam");
        let list = self.0.join("inputs.txt");
        fs::write(
            &first,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10\n@SQ\tSN:chr2\tLN:5\n\
r1\t0\tchr1\t1\t60\t3M\t*\t0\t0\tACG\tIII\n\
r2\t0\tchr1\t3\t60\t2M\t*\t0\t0\tAC\tII\n\
r3\t0\tchr2\t2\t10\t1M\t*\t0\t0\tA\t!\n",
        )
        .unwrap();
        fs::write(
            &second,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10\n@SQ\tSN:chr2\tLN:5\n\
r4\t0\tchr1\t2\t60\t2M\t*\t0\t0\tAC\t!!\n",
        )
        .unwrap();
        fs::write(
            &list,
            format!("{}\n{}\n", first.display(), second.display()),
        )
        .unwrap();
        (first, second, list)
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

    fn histogram_sam(&self) -> PathBuf {
        let path = self.0.join("histogram.sam");
        fs::write(
            &path,
            b"@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:20\n\
r1\t0\tchr1\t1\t60\t5M\t*\t0\t0\tACGTA\tIIIII\n\
r2\t0\tchr1\t3\t60\t5M\t*\t0\t0\tACGTA\tIIIII\n\
r3\t0\tchr1\t5\t60\t5M\t*\t0\t0\tACGTA\tIIIII\n",
        )
        .unwrap();
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

const HEADER: &str =
    "#rname\tstartpos\tendpos\tnumreads\tcovbases\tcoverage\tmeandepth\tmeanbaseq\tmeanmapq\n";

fn expected_default() -> String {
    format!("{HEADER}chr1\t1\t30\t2\t6\t20\t0.2\t20\t40\n")
}

#[test]
fn default_coverage_matches_samtools_shape() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let result = scratch.run(&["coverage", input.to_str().unwrap()]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected_default());
}

#[test]
fn no_header_and_filters_match_samtools_shape() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input_text = input.to_str().unwrap();

    let no_header = scratch.run(&["coverage", "-H", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&no_header.stdout),
        "chr1\t1\t30\t2\t6\t20\t0.2\t20\t40\n"
    );

    let mapq = scratch.run(&["coverage", "-q", "30", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&mapq.stdout),
        format!("{HEADER}chr1\t1\t30\t1\t3\t10\t0.1\t40\t60\n")
    );

    let baseq = scratch.run(&["coverage", "-Q", "30", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&baseq.stdout),
        format!("{HEADER}chr1\t1\t30\t2\t3\t10\t0.1\t40\t40\n")
    );

    let min_len = scratch.run(&["coverage", "-l", "4", input_text]);
    assert_eq!(
        String::from_utf8_lossy(&min_len.stdout),
        format!("{HEADER}chr1\t1\t30\t0\t0\t0\t0\t0\t0\n")
    );
}

#[test]
fn region_is_restricted_and_formatted_like_samtools() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let result = scratch.run(&["coverage", "-r", "chr1:1-10", input.to_str().unwrap()]);
    assert!(result.status.success(), "{result:?}");
    // r1 covers 1-3, r2 covers 10-12; inside 1-10 there are 4 covered bases,
    // mean depth 4/10=0.4, mean baseQ (3*40+0)/4=30, mean mapQ (60+20)/2=40.
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        format!("{HEADER}chr1\t1\t10\t2\t4\t40\t0.4\t30\t40\n")
    );
}

#[test]
fn multiple_inputs_and_bam_list_merge_by_position() {
    let scratch = Scratch::new();
    let (first, second, list) = scratch.multiple_inputs();
    let expected =
        format!("{HEADER}chr1\t1\t10\t3\t4\t40\t0.7\t28.6\t60\nchr2\t1\t5\t1\t1\t20\t0.2\t0\t10\n");
    let positional = scratch.run(&[
        "coverage",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(positional.status.success(), "{positional:?}");
    assert_eq!(String::from_utf8_lossy(&positional.stdout), expected);

    let from_list = scratch.run(&["coverage", "-b", list.to_str().unwrap()]);
    assert!(from_list.status.success(), "{from_list:?}");
    assert_eq!(String::from_utf8_lossy(&from_list.stdout), expected);
}

#[test]
fn minimum_depth_is_applied_after_merging_inputs() {
    let scratch = Scratch::new();
    let (first, second, _) = scratch.multiple_inputs();
    let args = [first.to_str().unwrap(), second.to_str().unwrap()];

    let minimum_two = scratch.run(&["coverage", "--min-depth", "2", args[0], args[1]]);
    assert!(minimum_two.status.success(), "{minimum_two:?}");
    assert_eq!(
        String::from_utf8_lossy(&minimum_two.stdout),
        format!("{HEADER}chr1\t1\t10\t3\t2\t20\t0.5\t24\t60\nchr2\t1\t5\t1\t0\t0\t0\t0\t10\n")
    );

    let minimum_three = scratch.run(&["coverage", "--min-depth", "3", args[0], args[1]]);
    assert!(minimum_three.status.success(), "{minimum_three:?}");
    assert_eq!(
        String::from_utf8_lossy(&minimum_three.stdout),
        format!("{HEADER}chr1\t1\t10\t3\t1\t10\t0.3\t26.7\t60\nchr2\t1\t5\t1\t0\t0\t0\t0\t10\n")
    );
}

#[test]
fn required_and_filter_flags_follow_coverage_semantics() {
    let scratch = Scratch::new();
    let input = scratch.flag_sam();
    let input = input.to_str().unwrap();
    let cases = [
        ("--ff", "DUP", 6, 12),
        ("--ff", "0", 7, 14),
        ("--ff", "PAIRED", 3, 6),
        ("--rf", "PAIRED", 4, 8),
        ("--rf", "PAIRED,PROPER_PAIR", 4, 8),
        ("--rf", "PAIRED,REVERSE", 4, 8),
        ("--rf", "PROPER_PAIR,READ1", 3, 6),
    ];
    for (option, value, reads, bases) in cases {
        let output = scratch.run(&["coverage", option, value, input]);
        assert!(output.status.success(), "{option} {value}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!(
                "{HEADER}chr1\t1\t20\t{reads}\t{bases}\t{}\t{}\t40\t60\n",
                bases * 5,
                bases as f64 / 20.0
            ),
            "{option} {value}"
        );
    }
}

#[test]
fn histogram_and_ascii_depth_plot_match_samtools() {
    let scratch = Scratch::new();
    let input = scratch.histogram_sam();
    let input = input.to_str().unwrap();
    let breadth = scratch.run(&["coverage", "-m", "-w", "5", input]);
    assert!(breadth.status.success(), "{breadth:?}");
    assert_eq!(
        String::from_utf8_lossy(&breadth.stdout),
        concat!(
            "chr1 (20bp)\n",
            ">  90.00% │██   │ Number of reads: 3\n",
            ">  80.00% │██   │ \n",
            ">  70.00% │██   │ Covered bases:   9bp\n",
            ">  60.00% │██   │ Percent covered: 45%\n",
            ">  50.00% │██   │ Mean coverage:   0.75x\n",
            ">  40.00% │██   │ Mean baseQ:      40\n",
            ">  30.00% │██   │ Mean mapQ:       60\n",
            ">  20.00% │██▄  │ \n",
            ">  10.00% │███  │ Histo bin width: 4bp\n",
            ">   0.00% │███  │ Histo max bin:   100%\n",
            "          1              20   \n"
        )
    );

    let depth = scratch.run(&["coverage", "-D", "-w", "5", "-A", input]);
    assert!(depth.status.success(), "{depth:?}");
    assert_eq!(
        String::from_utf8_lossy(&depth.stdout),
        concat!(
            "chr1 (20bp)\n",
            ">     1.8 | :   | Number of reads: 3\n",
            ">     1.6 | :   | \n",
            ">     1.4 |.:   | Covered bases:   9bp\n",
            ">     1.2 |::   | Percent covered: 45%\n",
            ">     1.0 |::   | Mean coverage:   0.75x\n",
            ">     0.8 |::   | Mean baseQ:      40\n",
            ">     0.6 |::   | Mean mapQ:       60\n",
            ">     0.4 |::   | \n",
            ">     0.2 |::   | Histo bin width: 4bp\n",
            ">     0.0 |:::  | Histo max cov:   2\n",
            "          1              20   \n"
        )
    );
}

#[test]
fn depth_cap_is_per_alignment_start_not_final_depth() {
    let scratch = Scratch::new();
    let input = scratch.0.join("depth-cap.sam");
    let mut text = String::from("@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:20\n");
    for (index, quality) in [40, 40, 40, 40, 0, 0].into_iter().enumerate() {
        text.push_str(&format!(
            "r{}\t0\tchr1\t1\t60\t20M\t*\t0\t0\t{}\t{}\n",
            index + 1,
            "A".repeat(20),
            char::from(33 + quality).to_string().repeat(20)
        ));
    }
    fs::write(&input, text).unwrap();
    let input = input.to_str().unwrap();
    let capped = scratch.run(&["coverage", "-d", "1", input]);
    assert!(capped.status.success(), "{capped:?}");
    assert_eq!(
        String::from_utf8_lossy(&capped.stdout),
        format!("{HEADER}chr1\t1\t20\t6\t20\t100\t1\t40\t60\n")
    );
    let five = scratch.run(&["coverage", "-d", "5", input]);
    assert!(five.status.success(), "{five:?}");
    assert_eq!(
        String::from_utf8_lossy(&five.stdout),
        format!("{HEADER}chr1\t1\t20\t6\t20\t100\t5\t32\t60\n")
    );
}

#[test]
fn histogram_keeps_region_tail_in_summary() {
    let scratch = Scratch::new();
    let input = scratch.histogram_sam();
    let input = input.to_str().unwrap();
    let output = scratch.run(&["coverage", "-r", "chr1:1-10", "-m", "-w", "4", input]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Covered bases:   9bp"), "{text}");
    assert!(text.contains("Percent covered: 90%"), "{text}");
    assert!(text.contains("Mean coverage:   1.5x"), "{text}");
}

#[test]
fn output_file_contains_the_same_text() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let output = scratch.0.join("coverage.tsv");
    let result = scratch.run(&[
        "coverage",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read_to_string(output).unwrap(), expected_default());
}
