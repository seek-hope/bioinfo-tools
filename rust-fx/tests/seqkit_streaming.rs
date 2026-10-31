//! Streaming and output-abort contracts shared by range and tab2fx.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn produces_output_before_eof(args: &[&str], data: Vec<u8>, expected: Vec<u8>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-fx"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let (close, wait_for_close) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let result = input.write_all(&data);
        // The child must emit output before we signal EOF, even after all
        // currently available input has been consumed.
        let _ = wait_for_close.recv();
        drop(input);
        result
    });
    let (ready, observed) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut first = [0; 4096];
        let count = output.read(&mut first).unwrap_or(0);
        let _ = ready.send(count);
        let mut bytes = first[..count].to_vec();
        output.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let observed = observed.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    if !matches!(observed, Ok(n) if n > 0) {
        let _ = child.kill();
    }
    let _ = close.send(());
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().unwrap();
    let input_result = writer.join().unwrap();
    let bytes = reader.join().unwrap();
    let stderr = errors.join().unwrap();
    assert!(!timed_out, "child stalled after producing output: {args:?}");
    assert!(
        matches!(observed, Ok(n) if n > 0),
        "waited for EOF: {args:?}"
    );
    assert!(
        status.success(),
        "{status:?}: {}",
        String::from_utf8_lossy(&stderr)
    );
    input_result.unwrap();
    assert_eq!(bytes, expected);
}

#[test]
fn range_start_to_end_streams_stdin_without_counting_or_spooling() {
    let record = b">a\nACGT\n";
    produces_output_before_eof(
        &["range", "-r", "2:-1"],
        record.repeat(30000),
        record.repeat(29999),
    );
}

#[test]
fn tab2fx_streams_stdin_with_a_fixed_output_commit_buffer() {
    produces_output_before_eof(
        &["tab2fx"],
        b"a\tAC\n".repeat(30000),
        b">a\nAC\n".repeat(30000),
    );
}
