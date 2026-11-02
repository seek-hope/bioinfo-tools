//! External-sort regressions: bounded chunks, multi-pass merge, sidecar index,
//! temp cleanup and malformed-input failure. Inputs are generated here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-bam-sort-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("tmp")).unwrap();
        Self(path)
    }

    fn input(&self, records: usize, tail: Option<&str>) -> PathBuf {
        let path = self.0.join("input.sam");
        let mut text = String::from(
            "@HD\tVN:1.6\tSO:unsorted\n@SQ\tSN:chr1\tLN:1000\n@SQ\tSN:chr2\tLN:1000\n",
        );
        for index in 0..records {
            let unmapped = index % 5 == 0;
            let name = format!("r{index:04}");
            if unmapped {
                text.push_str(&format!("{name}\t4\t*\t0\t0\t*\t*\t0\t0\tACGT\tIIII\n"));
            } else {
                let reference = if index % 2 == 0 { "chr1" } else { "chr2" };
                let flag = if index % 3 == 0 { 16 } else { 0 };
                let position = index % 20 + 1;
                text.push_str(&format!(
                    "{name}\t{flag}\t{reference}\t{position}\t60\t4M\t*\t0\t0\tACGT\tIIII\n"
                ));
            }
        }
        if let Some(tail) = tail {
            text.push_str(tail);
        }
        fs::write(&path, text).unwrap();
        path
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rust-bam"))
            .args(args)
            .env("TMPDIR", self.0.join("tmp"))
            .output()
            .unwrap()
    }

    fn leftover_workdirs(&self) -> Vec<PathBuf> {
        fs::read_dir(self.0.join("tmp"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("rust-bam-sort-"))
            })
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_no_output(path: &Path) {
    assert!(!path.exists(), "unexpected output file: {}", path.display());
}

#[test]
fn external_sort_is_byte_identical_to_in_memory_sort_and_cleans_temp() {
    let scratch = Scratch::new();
    let input = scratch.input(200, None);
    let memory = scratch.0.join("memory.sam");
    let external = scratch.0.join("external.sam");

    let normal = scratch.command(&[
        "sort",
        "-O",
        "-o",
        memory.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(normal.status.success(), "{normal:?}");
    let isolated = scratch.command(&[
        "sort",
        "--max-memory",
        "1KiB",
        "--max-open-files",
        "2",
        "-O",
        "-o",
        external.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(isolated.status.success(), "{isolated:?}");
    assert!(String::from_utf8_lossy(&isolated.stderr).contains("sorted 200 records"));
    assert_eq!(fs::read(&memory).unwrap(), fs::read(&external).unwrap());
    assert_eq!(scratch.leftover_workdirs(), Vec::<PathBuf>::new());
}

#[test]
fn parallel_compression_is_byte_identical_across_spill_and_worker_counts() {
    let scratch = Scratch::new();
    let input = scratch.input(4000, None);
    let expected = scratch.0.join("serial.bam");
    let result = scratch.command(&[
        "sort",
        "-o",
        expected.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    let expected = fs::read(expected).unwrap();
    for budget in ["8KiB", "32MiB"] {
        for workers in ["0", "1", "2", "4"] {
            let output = scratch.0.join(format!("out-{budget}-{workers}.bam"));
            let result = scratch.command(&[
                "sort",
                "--max-memory",
                budget,
                "--max-open-files",
                "2",
                "-@",
                workers,
                "-o",
                output.to_str().unwrap(),
                input.to_str().unwrap(),
            ]);
            assert!(result.status.success(), "{result:?}");
            assert_eq!(
                fs::read(output).unwrap(),
                expected,
                "budget={budget} workers={workers}"
            );
            assert!(scratch.leftover_workdirs().is_empty());
        }
    }
}

#[test]
fn encoded_runs_preserve_auxiliary_fields_long_cigar_and_buffer_clearing() {
    let scratch = Scratch::new();
    let input = scratch.0.join("fields.sam");
    let mut sam = String::from("@HD\tVN:1.6\tSO:unsorted\n@SQ\tSN:c\tLN:1000000\n");
    for i in 0..200 {
        if i % 5 == 0 {
            sam.push_str("*\t4\t*\t0\t0\t*\t*\t0\t0\t*\t*\n");
        } else {
            let len = i % 75 + 1;
            let name = if i % 3 == 0 {
                "name".repeat(60)
            } else {
                "same".into()
            };
            let tags = if i % 2 == 0 {
                format!("\tAA:A:x\tAC:B:c,-1,2\tAS:B:s,-300,400\tAI:B:i,-100000,200000\tAU:B:C,0,255\tAV:B:S,0,65535\tAW:B:I,0,4294967295\tAF:B:f,1.5,-2.25\tZZ:Z:{i}\tHH:H:00FF\tNM:i:{i}\tFF:f:NaN")
            } else {
                String::new()
            };
            sam.push_str(&format!(
                "{name}\t0\tc\t{}\t60\t{len}M\t*\t0\t0\t{}\t{}{tags}\n",
                i % 10 + 1,
                "A".repeat(len),
                "I".repeat(len)
            ));
        }
    }
    // This requires BAM's CG representation and crosses BGZF block boundaries.
    sam.push_str(&format!(
        "long\t0\tc\t7\t60\t{}\t*\t0\t0\t{}\t{}\tZZ:Z:end\n",
        "1M".repeat(65536),
        "C".repeat(65536),
        "I".repeat(65536)
    ));
    fs::write(&input, sam).unwrap();
    let memory = scratch.0.join("memory.bam");
    let result = scratch.command(&[
        "sort",
        "-o",
        memory.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{result:?}");
    let expected = fs::read(&memory).unwrap();
    for source in [&input, &memory] {
        for threads in ["1", "4"] {
            let output = scratch.0.join("external.bam");
            let result = scratch.command(&[
                "sort",
                "--max-memory",
                "8KiB",
                "--max-open-files",
                "2",
                "-@",
                threads,
                "-o",
                output.to_str().unwrap(),
                source.to_str().unwrap(),
            ]);
            assert!(result.status.success(), "{result:?}");
            assert_eq!(fs::read(output).unwrap(), expected);
            assert!(scratch.leftover_workdirs().is_empty());
        }
    }
}

#[test]
fn merge_reuses_heads_across_cached_large_and_cached_records() {
    let scratch = Scratch::new();
    let input = scratch.0.join("transitions.sam");
    let mut sam = String::from("@HD\tVN:1.6\tSO:unsorted\n@SQ\tSN:c\tLN:1000000\n");
    // Runs have interleaved coordinates. The middle records exceed the capture
    // threshold, while both neighbours use captured frames. Small budgets and
    // fan-in 2 also exercise these transitions in intermediate merge passes.
    for i in (0..18).rev() {
        for (part, length) in [1, 70000, 2].into_iter().enumerate() {
            let quality = char::from(b'!' + i as u8);
            sam.push_str(&format!(
                "r{i}-{part}\t0\tc\t{}\t60\t{length}M\t*\t0\t0\t{}\t{}\tZZ:Z:{i}-{part}\n",
                part * 18 + i + 1,
                "ACGT".chars().cycle().take(length).collect::<String>(),
                quality.to_string().repeat(length),
            ));
        }
    }
    fs::write(&input, sam).unwrap();
    for sam_output in [false, true] {
        let memory = scratch.0.join("memory-output");
        let mut args = vec![
            "sort",
            "-o",
            memory.to_str().unwrap(),
            input.to_str().unwrap(),
        ];
        if sam_output {
            args.push("-O");
        }
        let result = scratch.command(&args);
        assert!(result.status.success(), "{result:?}");
        let expected = fs::read(memory).unwrap();
        for (budget, fan_in) in [("128KiB", "2"), ("384KiB", "64")] {
            for threads in ["1", "4"] {
                let output = scratch.0.join("merged-output");
                let mut args = vec![
                    "sort",
                    "--max-memory",
                    budget,
                    "--max-open-files",
                    fan_in,
                    "--threads",
                    threads,
                    "-o",
                    output.to_str().unwrap(),
                    input.to_str().unwrap(),
                ];
                if sam_output {
                    args.push("-O");
                }
                let result = scratch.command(&args);
                assert!(result.status.success(), "{result:?}");
                assert_eq!(
                    fs::read(output).unwrap(),
                    expected,
                    "sam={sam_output} budget={budget} threads={threads}"
                );
                assert!(scratch.leftover_workdirs().is_empty());
            }
        }
    }
}

#[test]
fn malformed_input_fails_without_output_or_temp_runs() {
    let scratch = Scratch::new();
    let input = scratch.input(100, Some("broken\t0\tchr1\n"));
    let output = scratch.0.join("out.sam");
    let result = scratch.command(&[
        "sort",
        "--max-memory",
        "1KiB",
        "--max-open-files",
        "2",
        "-O",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(!result.status.success(), "{result:?}");
    assert_no_output(&output);
    assert_eq!(scratch.leftover_workdirs(), Vec::<PathBuf>::new());
}

#[test]
fn invalid_memory_size_is_rejected_before_opening_output() {
    let scratch = Scratch::new();
    let input = scratch.input(2, None);
    let output = scratch.0.join("out.sam");
    let result = scratch.command(&[
        "sort",
        "--max-memory",
        "10XiB",
        "-O",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(2), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr).contains("unknown size suffix"));
    assert_no_output(&output);
}
