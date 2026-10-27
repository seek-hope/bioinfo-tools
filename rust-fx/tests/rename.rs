//! `rename` regressions: duplicate IDs/names, first-record/counter/separator options.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-rename-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(
            &path,
            b">a comment\nAC\n>a description\nGT\n>a\nTT\n>b comment\nGG\n",
        )
        .unwrap();
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
fn duplicate_ids_are_renamed_and_comments_preserved() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let default = scratch.run(&["rename", input]);
    assert_eq!(
        String::from_utf8_lossy(&default.stdout),
        ">a comment\nAC\n>a_2 description\nGT\n>a_3\nTT\n>b comment\nGG\n"
    );

    let full_name = scratch.run(&["rename", "-n", input]);
    assert_eq!(
        String::from_utf8_lossy(&full_name.stdout),
        ">a comment\nAC\n>a description\nGT\n>a\nTT\n>b comment\nGG\n"
    );
}

#[test]
fn multiple_outfiles_share_duplicate_counters_and_honor_force() {
    let scratch = Scratch::new();
    let first = scratch.0.join("a.fa");
    let second = scratch.0.join("b.fa");
    fs::write(&first, b">a\nAC\n>a\nGT\n").unwrap();
    fs::write(&second, b">a\nTT\n>a\nGG\n").unwrap();
    let out_dir = scratch.0.join("out");
    let output = scratch.run(&[
        "rename",
        "-m",
        "-O",
        out_dir.to_str().unwrap(),
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(out_dir.join("a.fa")).unwrap(),
        ">a\nAC\n>a_2\nGT\n"
    );
    assert_eq!(
        fs::read_to_string(out_dir.join("b.fa")).unwrap(),
        ">a_3\nTT\n>a_4\nGG\n"
    );

    let forced = scratch.run(&[
        "rename",
        "-m",
        "-f",
        "-O",
        out_dir.to_str().unwrap(),
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(forced.status.success(), "{forced:?}");
    assert!(forced.stderr.is_empty(), "{forced:?}");
}

#[test]
fn first_record_separator_and_start_number_are_honored() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let first = scratch.run(&["rename", "-1", input]);
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        ">a_1 comment\nAC\n>a_2 description\nGT\n>a_3\nTT\n>b_1 comment\nGG\n"
    );

    let separator = scratch.run(&["rename", "-s", ".", input]);
    assert_eq!(
        String::from_utf8_lossy(&separator.stdout),
        ">a comment\nAC\n>a.2 description\nGT\n>a.3\nTT\n>b comment\nGG\n"
    );

    let start = scratch.run(&["rename", "-N", "5", input]);
    assert_eq!(
        String::from_utf8_lossy(&start.stdout),
        ">a comment\nAC\n>a_5 description\nGT\n>a_6\nTT\n>b comment\nGG\n"
    );
}
