use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-bam-filters-{}-{}",
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
            b"@HD\tVN:1.6\tSO:unsorted\n@SQ\tSN:chr1\tLN:1000\n\
r1\t0\tchr1\t1\t60\t4M\t*\t0\t0\tACGT\tIIII\n\
r2\t16\tchr1\t2\t10\t4M\t*\t0\t0\tACGT\tIIII\n\
r3\t4\t*\t0\t0\t*\t*\t0\t0\tACGT\tIIII\n\
r4\t0\tchr1\t3\t60\t4M\t*\t0\t0\tACGT\tIIII\n",
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

fn count(output: &Output) -> u64 {
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn flag_and_mapq_filters_apply_to_counts() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input = input.to_str().unwrap();

    let all = scratch.run(&["view", "-c", input]);
    assert_eq!(count(&all), 4);

    let reverse = scratch.run(&["view", "-c", "-f", "16", input]);
    assert_eq!(count(&reverse), 1);

    let mapped = scratch.run(&["view", "-c", "-F", "4", input]);
    assert_eq!(count(&mapped), 3);

    let mapq = scratch.run(&["view", "-c", "-q", "30", input]);
    assert_eq!(count(&mapq), 2);

    let named = scratch.run(&["view", "-c", "-f", "REVERSE", input]);
    assert_eq!(count(&named), 1);

    let invalid = scratch.run(&["view", "-c", "-f", "NOT_A_FLAG", input]);
    assert_eq!(invalid.status.code(), Some(2), "{invalid:?}");
}

#[test]
fn filters_apply_to_bam_output_too() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input = input.to_str().unwrap();
    let output = scratch.0.join("filtered.bam");
    let write = scratch.run(&[
        "view",
        "-b",
        "-f",
        "16",
        "-o",
        output.to_str().unwrap(),
        input,
    ]);
    assert!(write.status.success(), "{write:?}");
    let count = scratch.run(&["view", "-c", output.to_str().unwrap()]);
    assert_eq!(count_fn(&count), 1);
}

fn count_fn(output: &Output) -> u64 {
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn region_query_uses_bai_and_filters() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input = input.to_str().unwrap();
    let bam = scratch.0.join("input.bam");
    let sorted = scratch.0.join("sorted.bam");
    let write = scratch.run(&["view", "-b", "-o", bam.to_str().unwrap(), input]);
    assert!(write.status.success(), "{write:?}");
    let sort = scratch.run(&[
        "sort",
        "-o",
        sorted.to_str().unwrap(),
        bam.to_str().unwrap(),
    ]);
    assert!(sort.status.success(), "{sort:?}");
    let index = scratch.run(&["index", sorted.to_str().unwrap()]);
    assert!(index.status.success(), "{index:?}");

    let first = scratch.run(&[
        "view",
        "-c",
        "--region",
        "chr1:1-2",
        sorted.to_str().unwrap(),
    ]);
    assert_eq!(count_fn(&first), 2, "{first:?}");
    let second = scratch.run(&[
        "view",
        "-c",
        "--region",
        "chr1:2-2",
        sorted.to_str().unwrap(),
    ]);
    // r1 spans 1-4 and r2 spans 2-5, so both intersect the one-base region.
    assert_eq!(count_fn(&second), 2, "{second:?}");
    let filtered = scratch.run(&[
        "view",
        "-c",
        "--region",
        "chr1:1-2",
        "-f",
        "16",
        sorted.to_str().unwrap(),
    ]);
    assert_eq!(count_fn(&filtered), 1, "{filtered:?}");

    let sam_region = scratch.run(&["view", "-c", "--region", "chr1:1-2", input]);
    assert_eq!(sam_region.status.code(), Some(3), "{sam_region:?}");
}

#[test]
fn csi_index_supports_region_queries() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input = input.to_str().unwrap();
    let bam = scratch.0.join("input.bam");
    let sorted = scratch.0.join("sorted.bam");
    assert!(scratch
        .run(&["view", "-b", "-o", bam.to_str().unwrap(), input])
        .status
        .success());
    assert!(scratch
        .run(&[
            "sort",
            "-o",
            sorted.to_str().unwrap(),
            bam.to_str().unwrap()
        ])
        .status
        .success());
    let csi = scratch.0.join("sorted.bam.csi");
    let indexed = scratch.run(&["index", "--csi", sorted.to_str().unwrap()]);
    assert!(indexed.status.success(), "{indexed:?}");
    assert!(csi.is_file(), "missing {}", csi.display());
    let _ = fs::remove_file(scratch.0.join("sorted.bam.bai"));
    let count = scratch.run(&[
        "view",
        "-c",
        "--region",
        "chr1:1-2",
        sorted.to_str().unwrap(),
    ]);
    assert_eq!(count_fn(&count), 2, "{count:?}");
}

#[test]
fn threaded_bam_output_is_readable() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let input = input.to_str().unwrap();
    let output = scratch.0.join("threaded.bam");
    let write = scratch.run(&[
        "view",
        "-b",
        "-@",
        "2",
        "-o",
        output.to_str().unwrap(),
        input,
    ]);
    assert!(write.status.success(), "{write:?}");
    let count = scratch.run(&["view", "-c", output.to_str().unwrap()]);
    assert_eq!(count_fn(&count), 4, "{count:?}");
}

#[test]
fn sort_rejects_threads_above_one_explicitly() {
    let scratch = Scratch::new();
    let input = scratch.sam();
    let output = scratch.0.join("sorted.bam");
    let result = scratch.run(&[
        "sort",
        "-@",
        "2",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(2), "{result:?}");
}
