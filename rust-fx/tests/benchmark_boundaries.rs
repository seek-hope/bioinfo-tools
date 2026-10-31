//! Boundaries exposed by the real-data README benchmark pilot.
use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], input: &[u8]) -> Vec<u8> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-fx"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    output.stdout
}

#[test]
fn quality_thresholds_include_q20_and_q30() {
    for score in [19_u8, 20, 21, 29, 30, 31] {
        let mut input = b"@read\nACGT\n+\n".to_vec();
        input.extend([score + 33; 4]);
        input.push(b'\n');
        let text = String::from_utf8(run(&["stats", "-a", "-T", "-"], &input)).unwrap();
        let mut rows = text.lines();
        let header: Vec<_> = rows.next().unwrap().split('\t').collect();
        let values: Vec<_> = rows.next().unwrap().split('\t').collect();
        for (name, threshold) in [("Q20(%)", 20), ("Q30(%)", 30)] {
            let index = header.iter().position(|field| *field == name).unwrap();
            assert_eq!(values[index], if score >= threshold { "100" } else { "0" });
        }
    }
}

#[test]
fn fq2fa_default_is_unwrapped_and_explicit_width_is_respected() {
    let sequence = "ACGT".repeat(38);
    let input = format!(
        "@read description\n{sequence}\n+\n{}\n",
        "I".repeat(sequence.len())
    );
    let unwrapped = format!(">read description\n{sequence}\n");
    assert_eq!(run(&["fq2fa", "-"], input.as_bytes()), unwrapped.as_bytes());
    assert_eq!(
        run(&["fq2fa", "-w", "0", "-"], input.as_bytes()),
        unwrapped.as_bytes()
    );
    for width in [1, 60, 80] {
        let mut expected = b">read description\n".to_vec();
        for chunk in sequence.as_bytes().chunks(width) {
            expected.extend_from_slice(chunk);
            expected.push(b'\n');
        }
        assert_eq!(
            run(&["fq2fa", "-w", &width.to_string(), "-"], input.as_bytes()),
            expected
        );
    }
}
