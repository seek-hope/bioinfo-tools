//! Error boundaries and frame semantics from direct SeqKit 2.14.0 comparisons.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-fx"))
        .arg("translate")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        // An invalid first record can close stdin before all input is written.
        let _ = stdin.write_all(&input);
    });
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    result
}

#[test]
fn fatal_input_discards_only_pending_output() {
    for (prefix, committed) in [(10, 0), (14_000, 65_536)] {
        let mut input = b">a\nATG\n".repeat(prefix);
        input.extend_from_slice(b">bad\nA?G\n");
        let result = run(&[], &input);
        assert_eq!(result.status.code(), Some(255));
        assert_eq!(result.stdout, b">a\nM\n".repeat(prefix)[..committed]);
        assert_eq!(result.stderr, b"ERROR: seq: invalid DNA base\n");
    }
}

#[test]
fn excess_wrapped_quality_is_rejected_before_committing_the_record() {
    let mut input = b"@a\n".to_vec();
    input.extend_from_slice(&b"ATG".repeat(65_536));
    input.extend_from_slice(b"\n+\n");
    input.extend_from_slice(&b"I".repeat(196_608));
    input.extend_from_slice(b"\nI\n");
    let result = run(&["-w", "0"], &input);
    assert_eq!(result.status.code(), Some(255));
    assert!(
        result.stdout.is_empty(),
        "must not commit an invalid record"
    );
    assert_eq!(
        result.stderr,
        b"ERROR: fastx: unequal sequence and quality\n"
    );
}

#[test]
fn short_sequences_can_all_be_skipped_successfully() {
    let result = run(&["-e", "-F", "-f", "1,2"], b">a desc\nA\n>b\nAC\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a desc\n\n>a desc\n\n>b \n\n>b \n\n");
    assert!(result.stderr.is_empty());
    let fatal = run(&[], b">a\nA\n");
    assert_eq!(fatal.status.code(), Some(255));
    assert_eq!(
        fatal.stderr,
        b"ERROR: seq: sequence too short to translate: 1\n"
    );
}

#[test]
fn first_record_alphabet_check_is_distinct_from_codon_errors() {
    for args in [&[][..], &["-e"][..], &["-x"][..]] {
        let result = run(args, b">bad\nA?G\n");
        assert_eq!(result.status.code(), Some(255));
        assert!(result.stdout.is_empty());
        assert_eq!(
            result.stderr,
            b"ERROR: command 'seqkit translate' only apply to DNA/RNA sequences\n"
        );
    }
    let result = run(&["-x"], b">a\nATG\xffGG\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a\nMX\n");
}

#[test]
fn trim_preserves_internal_residues_and_init_m_uses_table_starts() {
    let input = b">a\nCCCTAATTG\n>b\nATGNNNCCC\n>c\nATGNNNTAA\n";
    let result = run(&["--trim", "--clean"], input);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a\nPXL\n>b\nMXP\n>c\nM\n");
    let invalid = run(&["--trim"], b">a\nATGNNNA?G\n");
    assert_eq!(invalid.status.code(), Some(255));
    assert!(invalid.stdout.is_empty());
    let result = run(&["-M"], b">a\nCCCTAATTG\n>b\nGTG\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a\nP*M\n>b\nV\n");
    let result = run(&["-M", "-T", "11"], b">b\nGTG\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">b\nM\n");
}

#[test]
fn orf_coordinates_include_frame_and_keep_description() {
    let result = run(&["-s", "-f", "2"], b">a desc\nAATGCCCTAA\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a_frame=2_begin=2_end=7 desc\nMP\n");
    let result = run(&["-s", "-f", "-1"], b">a desc\nTTAGGGCAT\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a_frame=-1_begin=4_end=9 desc\nMP\n");
    let result = run(&["-s", "-f", "-1"], b">a desc\nGGGCAT\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">a_frame=-1_begin=1_end=6 desc\nMP\n");
}

#[test]
fn frames_are_ordered_repeatable_and_validated_before_output() {
    let result = run(&["-f", "3,1", "-f", "1", "-F"], b">a desc\nATG\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        result.stdout,
        b">a_frame=3 desc\n\n>a_frame=1 desc\nM\n>a_frame=1 desc\nM\n"
    );
    for frame in ["0", "-4", "7", "garbage"] {
        let result = run(&["-f", frame], b">a\nATG\n");
        assert_eq!(result.status.code(), Some(255), "{result:?}");
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn negative_frames_complement_rna_and_ambiguous_bases() {
    let result = run(&["-f", "-1"], b">ambiguous\nYTC\n>rna\nUUG\n");
    assert!(result.status.success(), "{result:?}");
    // YTC expands to CTC/TTC; their reverse complements GAG/GAA both code E.
    // UUG has reverse complement CAA (Q), matching the DNA equivalent TTG.
    assert_eq!(result.stdout, b">ambiguous\nE\n>rna\nQ\n");
    let dna = run(&["-f", "-1"], b">rna\nTTG\n");
    assert_eq!(dna.stdout, b">rna\nQ\n");
}

#[test]
fn repeated_header_separators_do_not_delete_description_letters() {
    for whitespace in [" ", "  ", "\t \t", "    "] {
        let input = format!(">r{whitespace}description\nATG\n");
        let result = run(&["-F"], input.as_bytes());
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b">r_frame=1 description\nM\n");
    }
}

#[test]
fn wrapped_fastq_and_nonstandard_header_bytes_are_preserved() {
    let result = run(&["-F"], b"@>a \xff\nAT\nGCCC\n+ignored\nII\nIIII\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">>a_frame=1 \xff\nMP\n");
    let result = run(&[], b">>a\nATG\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b">>a\nM\n");
}
