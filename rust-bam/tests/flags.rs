use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-bam"))
        .arg("flags")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

#[test]
fn named_flags_are_case_insensitive_deduplicated_and_bit_ordered() {
    let output = run(&["READ1,REVERSE,read1", "paired,proper_pair"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout(&output),
        "0x50\t80\tREVERSE,READ1\n0x3\t3\tPAIRED,PROPER_PAIR\n"
    );
}

#[test]
fn numeric_flags_support_decimal_hex_octal_and_unknown_bits() {
    let output = run(&["0", "077", "0x50", "4096", "65535"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout(&output),
        concat!(
            "0x0\t0\t\n",
            "0x3f\t63\tPAIRED,PROPER_PAIR,UNMAP,MUNMAP,REVERSE,MREVERSE\n",
            "0x50\t80\tREVERSE,READ1\n",
            "0x1000\t4096\t\n",
            "0xffff\t65535\tPAIRED,PROPER_PAIR,UNMAP,MUNMAP,REVERSE,MREVERSE,READ1,READ2,SECONDARY,QCFAIL,DUP,SUPPLEMENTARY\n",
        )
    );
}

#[test]
fn numeric_prefixes_and_an_empty_argument_match_samtools() {
    let output = run(&["08", "0b11", "1,2", ""]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout(&output),
        "0x0\t0\t\n0x0\t0\t\n0x1\t1\tPAIRED\n0x0\t0\t\n"
    );
}

#[test]
fn no_arguments_prints_the_samtools_flags_usage() {
    let output = run(&[]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(stdout(&output).starts_with(
        "About: Convert between textual and numeric flag representation\nUsage: samtools flags FLAGS...\n"
    ));
    assert!(stdout(&output).ends_with(" 0x800  2048  SUPPLEMENTARY  supplementary alignment\n"));
}

#[test]
fn malformed_names_numbers_and_help_are_rejected() {
    for value in [
        "UNKNOWN",
        "PAIRED,",
        "PAIRED,0x4",
        "2147483648",
        "-1",
        "--help",
    ] {
        let output = run(&[value]);
        assert_eq!(output.status.code(), Some(1), "{value}: {output:?}");
        assert!(output.stdout.is_empty(), "{value}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Could not parse"),
            "{value}: {output:?}"
        );
    }
}
