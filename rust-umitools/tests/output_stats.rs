//! `dedup --output-stats`: NumPy-compatible null model and table layout.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch() -> PathBuf {
    let path = std::env::temp_dir().join(format!("rust-umitools-stats-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn unique_stats_match_pinned_umitools() {
    let dir = scratch();
    let input = dir.join("input.sam");
    fs::write(
        &input,
        "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:1000\n@SQ\tSN:chr2\tLN:1000\n\
r1\t0\tchr1\t100\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:AAAA\n\
r2\t0\tchr1\t100\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:AAAA\n\
r3\t0\tchr1\t200\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:AAAT\n\
r4\t0\tchr2\t100\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:AAAC\n\
r5\t0\tchr2\t200\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:GGGG\n\
r6\t0\tchr2\t200\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:GGGA\n",
    )
    .unwrap();
    let prefix = dir.join("stats");
    let result = Command::new(env!("CARGO_BIN_EXE_rust-umitools"))
        .args([
            "dedup",
            "-I",
            input.to_str().unwrap(),
            "-S",
            dir.join("out.bam").to_str().unwrap(),
            "--method",
            "unique",
            "--random-seed",
            "1",
            "--extract-umi-method=tag",
            "--umi-tag=RX",
            "--output-stats",
            prefix.to_str().unwrap(),
            "--quiet",
        ])
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");

    let read = |suffix: &str| {
        let path = dir.join(format!("stats_{suffix}"));
        fs::read_to_string(path).unwrap()
    };
    assert_eq!(
        read("per_umi_per_position.tsv"),
        "counts\tinstances_pre\tinstances_post\n1\t4\t4\n2\t1\t1\n"
    );
    assert_eq!(
        read("per_umi.tsv"),
        "UMI\tmedian_counts_pre\ttimes_observed_pre\ttotal_counts_pre\tmedian_counts_post\ttimes_observed_post\ttotal_counts_post\n\
AAAA\t2\t1\t2\t2\t1\t2\n\
AAAC\t1\t1\t1\t1\t1\t1\n\
AAAT\t1\t1\t1\t1\t1\t1\n\
GGGA\t1\t1\t1\t1\t1\t1\n\
GGGG\t1\t1\t1\t1\t1\t1\n"
    );
    assert_eq!(
        read("edit_distance.tsv"),
        "unique\tunique_null\tunique\tunique_null\tedit_distance\n\
3\t3\t3\t3\tSingle_UMI\n\
0\t0\t0\t0\t0\n\
1\t1\t1\t1\t1\n\
0\t0\t0\t0\t2\n"
    );
    let _ = fs::remove_dir_all(dir);
}
