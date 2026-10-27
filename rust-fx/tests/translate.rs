//! `translate` regressions: standard table, frames, unknown and init-M.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rust-fx-translate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn input(&self) -> PathBuf {
        let path = self.0.join("input.fasta");
        fs::write(&path, b">r1\nATGGCCATTGTAATGGGCCGCTGAAAGGGTGCCCGATAG\n").unwrap();
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
fn ambiguous_codons_clean_orfs_and_error_output_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.0.join("ambiguous.fa");
    fs::write(
        &input,
        b">r1\nATGGCCAAATAA\n>r2\nATGTAA\n>r3\nATGNNN\n>acn\nACN\n>ccn\nCCN\n",
    )
    .unwrap();
    let input = input.to_str().unwrap();
    let clean = scratch.run(&["translate", "--clean", input]);
    assert!(clean.status.success(), "{clean:?}");
    assert_eq!(
        String::from_utf8_lossy(&clean.stdout),
        ">r1\nMAKX\n>r2\nMX\n>r3\nMX\n>acn\nT\n>ccn\nP\n"
    );
    let orfs = scratch.run(&["translate", "-s", input]);
    assert!(orfs.status.success(), "{orfs:?}");
    assert_eq!(
        String::from_utf8_lossy(&orfs.stdout),
        ">r1_frame=1_begin=1_end=9 \nMAK\n>r2_frame=1_begin=1_end=3 \nM\n>r3_frame=1_begin=1_end=6 \nMX\n>acn_frame=1_begin=1_end=3 \nT\n>ccn_frame=1_begin=1_end=3 \nP\n"
    );
}

#[test]
fn skip_errors_writes_a_blank_record_with_a_trailing_space() {
    let scratch = Scratch::new();
    let input = scratch.0.join("bad.fa");
    fs::write(&input, b">good\nATG\n>bad\nA?G\n").unwrap();
    let output = scratch.run(&["translate", "-e", input.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        ">good\nM\n>bad \n\n"
    );
}

#[test]
fn fastq_translation_is_fasta_and_appends_frame() {
    let scratch = Scratch::new();
    let input = scratch.0.join("input.fq");
    fs::write(&input, b"@r\nATGCCC\n+\nIIIIII\n").unwrap();
    let output = scratch.run(&["translate", "-F", input.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">r_frame=1 \nMP\n");
}

#[test]
fn genetic_code_tables_use_their_own_codon_mappings() {
    let scratch = Scratch::new();
    let input = scratch.0.join("tables.fa");
    fs::write(&input, b">ata\nATA\n>tga\nTGA\n>taa\nTAA\n>ctg\nCTG\n").unwrap();
    let input = input.to_str().unwrap();
    for (table, expected) in [
        ("2", ">ata\nM\n>tga\nW\n>taa\n*\n>ctg\nL\n"),
        ("6", ">ata\nI\n>tga\n*\n>taa\nQ\n>ctg\nL\n"),
        ("12", ">ata\nI\n>tga\n*\n>taa\n*\n>ctg\nS\n"),
        ("25", ">ata\nI\n>tga\nG\n>taa\n*\n>ctg\nL\n"),
        ("11", ">ata\nI\n>tga\n*\n>taa\n*\n>ctg\nL\n"),
    ] {
        let output = scratch.run(&["translate", "-T", table, input]);
        assert!(output.status.success(), "table {table}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "table {table}"
        );
    }
}

#[test]
fn table_specific_stops_and_listing_metadata_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.0.join("special.fa");
    fs::write(&input, b">aga\nAGA\n>tca\nTCA\n>tta\nTTA\n").unwrap();
    let input = input.to_str().unwrap();
    for (table, expected) in [
        ("2", ">aga\n*\n>tca\nS\n>tta\nL\n"),
        ("22", ">aga\nR\n>tca\n*\n>tta\nL\n"),
        ("23", ">aga\nR\n>tca\nS\n>tta\n*\n"),
    ] {
        let output = scratch.run(&["translate", "-T", table, input]);
        assert!(output.status.success(), "table {table}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }

    let table27 = scratch.run(&["translate", "-l", "27"]);
    assert!(table27.status.success());
    let text = String::from_utf8_lossy(&table27.stdout);
    assert!(text.contains("Stop Codons:\n  TGA\n"));
    assert!(text.contains("TAA: Q, TAC: Y, TAG: Q, TAT: Y"));
    assert!(text.contains("TGA: W, TGC: C, TGG: W, TGT: C"));

    let ambiguous = scratch.run(&["translate", "-L", "1"]);
    assert!(ambiguous.status.success());
    let text = String::from_utf8_lossy(&ambiguous.stdout);
    assert!(text.contains("AAA: K, AAC: N, AAG: K, AAR: K, AAT: N, AAY: N"));
    assert!(text.contains("  MGA: R, MGG: R, MGR: R\n\n"));
    assert!(text.contains("  TRA: *\n"));

    let unknown = scratch.run(&["translate", "-l", "7"]);
    assert!(unknown.status.success());
    assert!(unknown.stdout.is_empty());
}

#[test]
fn standard_frames_match_seqkit() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let first = scratch.run(&["translate", input]);
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        ">r1\nMAIVMGR*KGAR*\n"
    );

    let second = scratch.run(&["translate", "-f", "2", input]);
    assert_eq!(
        String::from_utf8_lossy(&second.stdout),
        ">r1\nWPL*WAAERVPD\n"
    );

    let third = scratch.run(&["translate", "-f", "3", input]);
    assert_eq!(
        String::from_utf8_lossy(&third.stdout),
        ">r1\nGHCNGPLKGCPI\n"
    );
}

#[test]
fn unknown_codon_and_init_m_flags_are_accepted() {
    let scratch = Scratch::new();
    let input = scratch.input();
    let input = input.to_str().unwrap();

    let unknown = scratch.run(&["translate", "-x", input]);
    assert_eq!(
        String::from_utf8_lossy(&unknown.stdout),
        ">r1\nMAIVMGR*KGAR*\n"
    );

    let init = scratch.run(&["translate", "-M", input]);
    assert_eq!(
        String::from_utf8_lossy(&init.stdout),
        ">r1\nMAIVMGR*KGAR*\n"
    );
}
