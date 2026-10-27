//! `replace` regressions: name/sequence regex, captures and case folding.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-replace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">seq1 foo\nACGT\n>seq2 bar\nTTTT\n").unwrap();
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
fn name_replacement_and_capture_variables_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let id = scratch.run(&["replace", "-p", "seq", "-r", "id", input]);
    assert_eq!(
        String::from_utf8_lossy(&id.stdout),
        ">id1 foo\nACGT\n>id2 bar\nTTTT\n"
    );

    let comment = scratch.run(&["replace", "-p", "foo", "-r", "baz", input]);
    assert_eq!(
        String::from_utf8_lossy(&comment.stdout),
        ">seq1 baz\nACGT\n>seq2 bar\nTTTT\n"
    );

    let capture = scratch.run(&["replace", "-p", "seq([0-9])", "-r", "s$1", input]);
    assert_eq!(
        String::from_utf8_lossy(&capture.stdout),
        ">s1 foo\nACGT\n>s2 bar\nTTTT\n"
    );
}

#[test]
fn special_symbols_and_key_value_fallbacks_work() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();
    let numbered = scratch.run(&["replace", "-p", "(.*)", "-r", "{nr}", input]);
    assert!(numbered.status.success(), "{numbered:?}");
    assert_eq!(
        String::from_utf8_lossy(&numbered.stdout),
        ">1\nACGT\n>2\nTTTT\n"
    );
    let kv = scratch.0.join("kv.tsv");
    fs::write(&kv, b"1\tVALUE\n").unwrap();
    let value = scratch.run(&[
        "replace",
        "-p",
        "seq([12])",
        "-r",
        "{kv}",
        "-k",
        kv.to_str().unwrap(),
        input,
    ]);
    assert!(value.status.success(), "{value:?}");
    assert_eq!(
        String::from_utf8_lossy(&value.stdout),
        ">VALUE foo\nACGT\n> bar\nTTTT\n"
    );
    let keep = scratch.run(&[
        "replace",
        "-p",
        "seq([12])",
        "-r",
        "{kv}",
        "-K",
        "-k",
        kv.to_str().unwrap(),
        input,
    ]);
    assert_eq!(
        String::from_utf8_lossy(&keep.stdout),
        ">VALUE foo\nACGT\n>2 bar\nTTTT\n"
    );
    let missing = scratch.run(&[
        "replace",
        "-p",
        "seq([12])",
        "-r",
        "{kv}",
        "-m",
        "MISSING",
        "-k",
        kv.to_str().unwrap(),
        input,
    ]);
    assert_eq!(
        String::from_utf8_lossy(&missing.stdout),
        ">VALUE foo\nACGT\n>MISSING bar\nTTTT\n"
    );
}

#[test]
fn filters_are_applied_before_editing_and_sequence_kv_is_rejected() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();
    let filtered = scratch.run(&[
        "replace",
        "-p",
        "seq",
        "-r",
        "id",
        "--f-pattern",
        "seq1",
        input,
    ]);
    assert!(filtered.status.success(), "{filtered:?}");
    assert_eq!(
        String::from_utf8_lossy(&filtered.stdout),
        ">id1 foo\nACGT\n>seq2 bar\nTTTT\n"
    );
    let inverted = scratch.run(&[
        "replace",
        "-p",
        "seq",
        "-r",
        "id",
        "--f-pattern",
        "seq1",
        "--f-invert-match",
        input,
    ]);
    assert_eq!(
        String::from_utf8_lossy(&inverted.stdout),
        ">seq1 foo\nACGT\n>id2 bar\nTTTT\n"
    );
    let rejected = scratch.run(&[
        "replace",
        "-s",
        "-p",
        "A",
        "-r",
        "{kv}",
        "-k",
        "missing.tsv",
        input,
    ]);
    assert_eq!(rejected.status.code(), Some(255), "{rejected:?}");
    assert!(rejected.stdout.is_empty());
}

#[test]
fn sequence_replacement_and_ignore_case_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let sequence = scratch.run(&["replace", "-p", "ACGT", "-r", "NNNN", "-s", input]);
    assert_eq!(
        String::from_utf8_lossy(&sequence.stdout),
        ">seq1 foo\nNNNN\n>seq2 bar\nTTTT\n"
    );

    let case = scratch.run(&["replace", "-p", "acgt", "-r", "NNNN", "-s", "-i", input]);
    assert_eq!(
        String::from_utf8_lossy(&case.stdout),
        ">seq1 foo\nNNNN\n>seq2 bar\nTTTT\n"
    );
}
