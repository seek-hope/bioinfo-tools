//! `sliding -W/-s` regressions: window slices, coordinates and FASTQ qualities.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-sliding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn fasta(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nACGTACGT\n").unwrap();
        path
    }

    fn fastq(&self) -> PathBuf {
        let path = self.0.join("input.fastq");
        fs::write(&path, b"@q1\nACGTACGT\n+\nIIIIIIII\n").unwrap();
        path
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
fn sliding_windows_and_coordinates_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let input = input.to_str().unwrap();

    let windows = scratch.run(&["sliding", "-W", "4", "-s", "2", input]);
    assert_eq!(
        String::from_utf8_lossy(&windows.stdout),
        ">r1_sliding:1-4\nACGT\n>r1_sliding:3-6\nGTAC\n>r1_sliding:5-8\nACGT\n"
    );

    let stepped = scratch.run(&["sliding", "-W", "3", "-s", "1", input]);
    assert_eq!(
        String::from_utf8_lossy(&stepped.stdout),
        ">r1_sliding:1-3\nACG\n>r1_sliding:2-4\nCGT\n>r1_sliding:3-5\nGTA\n\
>r1_sliding:4-6\nTAC\n>r1_sliding:5-7\nACG\n>r1_sliding:6-8\nCGT\n"
    );

    let custom = scratch.run(&["sliding", "-W", "4", "-s", "4", "-S", "_w", input]);
    assert_eq!(
        String::from_utf8_lossy(&custom.stdout),
        ">r1_w:1-4\nACGT\n>r1_w:5-8\nACGT\n"
    );
}

#[test]
fn circular_windows_wrap_coordinates_and_preserve_quality() {
    let scratch = Scratch::new();
    let path = scratch.0.join("circular.fastq");
    fs::write(&path, b"@r\nACGT\n+\nABCD\n").unwrap();
    let input = path.to_str().unwrap();
    let output = scratch.run(&["sliding", "-W", "3", "-s", "1", "-c", input]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@r_sliding:1-3\nACG\n+\nABC\n@r_sliding:2-4\nCGT\n+\nBCD\n@r_sliding:3-1\nGTA\n+\nCDA\n@r_sliding:4-2\nTAC\n+\nDAB\n"
    );
    let stepped = scratch.run(&["sliding", "-W", "3", "-s", "2", "-C", input]);
    assert!(stepped.status.success(), "{stepped:?}");
    assert_eq!(
        String::from_utf8_lossy(&stepped.stdout),
        "@r_sliding:1-3\nACG\n+\nABC\n@r_sliding:3-1\nGTA\n+\nCDA\n"
    );
    let oversized = scratch.run(&["sliding", "-W", "5", "-s", "1", "-c", input]);
    assert!(oversized.status.success(), "{oversized:?}");
    assert_eq!(
        String::from_utf8_lossy(&oversized.stdout),
        "@r_sliding:1-1\nACGTA\n+\nABCDA\n@r_sliding:2-2\nCGTAC\n+\nBCDAB\n@r_sliding:3-3\nGTACG\n+\nCDABC\n@r_sliding:4-4\nTACGT\n+\nDABCD\n"
    );
}

#[test]
fn greedy_emits_all_trailing_short_windows() {
    let scratch = Scratch::new();
    let path = scratch.0.join("greedy.fastq");
    fs::write(&path, b"@r\nACGT\n+\nABCD\n").unwrap();
    let output = scratch.run(&[
        "sliding",
        "-W",
        "5",
        "-s",
        "1",
        "-g",
        path.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@r_sliding:1-4\nACGT\n+\nABCD\n@r_sliding:2-4\nCGT\n+\nBCD\n@r_sliding:3-4\nGT\n+\nCD\n@r_sliding:4-4\nT\n+\nD\n"
    );
}

#[test]
fn infile_list_is_expanded_before_windowing() {
    let scratch = Scratch::new();
    let input = scratch.fasta();
    let list = scratch.0.join("inputs.txt");
    fs::write(&list, format!("{}\n", input.display())).unwrap();
    let output = scratch.run(&[
        "sliding",
        "-W",
        "3",
        "-s",
        "2",
        "-X",
        list.to_str().unwrap(),
        "--skip-file-check",
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">r1_sliding:1-3\nACG\n>r1_sliding:3-5\nGTA\n>r1_sliding:5-7\nACG\n"
    );
}

#[test]
fn sliding_fastq_slices_quality_with_sequence() {
    let scratch = Scratch::new();
    let input = scratch.fastq();
    let output = scratch.run(&["sliding", "-W", "4", "-s", "2", input.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "@q1_sliding:1-4\nACGT\n+\nIIII\n@q1_sliding:3-6\nGTAC\n+\nIIII\n\
@q1_sliding:5-8\nACGT\n+\nIIII\n"
    );
}
