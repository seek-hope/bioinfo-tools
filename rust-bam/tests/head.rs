use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const SAM: &str = concat!(
    "@HD\tVN:1.6\tSO:coordinate\n",
    "@SQ\tSN:chr1\tLN:1000\n",
    "@RG\tID:rg1\tSM:sample\n",
    "@PG\tID:p1\tPN:test\tVN:1\n",
    "@CO\tcomment with spaces\n",
    "r1\t0\tchr1\t100\t60\t4M\t*\t0\t0\tACGT\tIIII\n",
    "r2\t16\tchr1\t200\t60\t4M\t*\t0\t0\tTGCA\tIIII\n",
);
const HEADER: &str = concat!(
    "@HD\tVN:1.6\tSO:coordinate\n",
    "@SQ\tSN:chr1\tLN:1000\n",
    "@RG\tID:rg1\tSM:sample\n",
    "@PG\tID:p1\tPN:test\tVN:1\n",
    "@CO\tcomment with spaces\n",
);
const R1: &str = "r1\t0\tchr1\t100\t60\t4M\t*\t0\t0\tACGT\tIIII\n";
const R2: &str = "r2\t16\tchr1\t200\t60\t4M\t*\t0\t0\tTGCA\tIIII\n";

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-bam-head-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let sam = path.join("input.sam");
        fs::write(&sam, SAM).unwrap();
        let bam = path.join("input.bam");
        let output = Command::new(env!("CARGO_BIN_EXE_rust-bam"))
            .args(["view", "-b", "-o"])
            .arg(&bam)
            .arg(&sam)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self(path)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-bam"))
        .arg("head")
        .args(args)
        .output()
        .unwrap()
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-bam"))
        .arg("head")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

#[test]
fn default_prints_all_sam_and_bam_headers_without_records() {
    let scratch = Scratch::new();
    for name in ["input.sam", "input.bam"] {
        let output = run(&[&scratch.path(name)]);
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(stdout(&output), HEADER, "{name}");
    }
}

#[test]
fn header_and_record_limits_are_independent() {
    let scratch = Scratch::new();
    let input = scratch.path("input.sam");
    let cases: &[(&[&str], String)] = &[
        (&["-n", "1"], format!("{HEADER}{R1}")),
        (
            &["-h", "1", "-n", "1"],
            format!("@HD\tVN:1.6\tSO:coordinate\n{R1}"),
        ),
        (&["-h", "0", "-n", "2"], format!("{R1}{R2}")),
        (
            &["--headers", "2", "--records", "1"],
            format!("@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:1000\n{R1}"),
        ),
    ];
    for (args, expected) in cases {
        let mut command = args.to_vec();
        command.push(&input);
        let output = run(&command);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(stdout(&output), expected, "{args:?}");
    }
}

#[test]
fn limits_larger_than_the_input_emit_the_complete_sam() {
    let scratch = Scratch::new();
    let output = run(&["-h", "99", "-n", "99", &scratch.path("input.sam")]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), SAM);
}

#[test]
fn sam_and_bam_standard_input_are_streamed_without_reopening() {
    let scratch = Scratch::new();
    for name in ["input.sam", "input.bam"] {
        let input = fs::read(scratch.0.join(name)).unwrap();
        let output = run_with_stdin(&["-n", "1"], &input);
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(stdout(&output), format!("{HEADER}{R1}"), "{name}");
    }
}

#[test]
fn explicit_stdin_path_supports_truncated_headers() {
    let scratch = Scratch::new();
    let input = fs::read(scratch.0.join("input.sam")).unwrap();
    let output = run_with_stdin(&["-h", "1", "-n", "1", "-"], &input);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), format!("@HD\tVN:1.6\tSO:coordinate\n{R1}"));
}
