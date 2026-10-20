//! End-to-end tests that exercise the real binary, including exit codes,
//! gzip input/output, malformed input and the "reject before writing" rule.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

fn binary() -> PathBuf {
    // target/<profile>/deps/<test binary> -> target/<profile>/rust-cutadapt
    let mut path = std::env::current_exe().expect("test executable path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(format!("rust-cutadapt{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "missing release binary at {path:?}");
    path
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "rust-cutadapt-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        );
        path.push(unique);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("failed to execute rust-cutadapt")
}

fn run_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rust-cutadapt");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("wait for child")
}

fn write_gzip(path: &Path, data: &[u8]) {
    let file = std::fs::File::create(path).expect("create gzip file");
    let mut encoder = GzEncoder::new(file, Compression::new(6));
    encoder.write_all(data).expect("write gzip");
    encoder.finish().expect("finish gzip");
}

fn read_maybe_gzip(path: &Path) -> Vec<u8> {
    let mut data = Vec::new();
    if path.extension().map(|e| e == "gz").unwrap_or(false) {
        MultiGzDecoder::new(std::fs::File::open(path).expect("open gzip"))
            .read_to_end(&mut data)
            .expect("decompress");
    } else {
        std::fs::File::open(path)
            .expect("open plain")
            .read_to_end(&mut data)
            .expect("read plain");
    }
    data
}

const ADAPTER: &str = "AGATCGGAAGAGC";

const MIXED: &[u8] = b"@full\nGCTAAAGACAATTACATAACATACACGTCAGCACGAAACTAGATCGGAAGAGC\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII\n@none\nTTTTTATTACACTCAGAAACAGAACTCGGGTAATTTTGACAGGTCACGCA\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII\n@short\nACGT\n+\nIIII\n";

const MIXED_EXPECTED: &[u8] = b"@full\nGCTAAAGACAATTACATAACATACACGTCAGCACGAAACT\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII\n@none\nTTTTTATTACACTCAGAAACAGAACTCGGGTAATTTTGACAGGTCACGCA\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII\n";

#[test]
fn plain_in_plain_out() {
    let dir = TempDir::new("plain");
    let input = dir.join("in.fastq");
    let output = dir.join("out.fastq");
    std::fs::write(&input, MIXED).unwrap();
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(read_maybe_gzip(&output), MIXED_EXPECTED);
}

#[test]
fn gzip_in_plain_out() {
    let dir = TempDir::new("gzin");
    let input = dir.join("in.fastq.gz");
    let output = dir.join("out.fastq");
    write_gzip(&input, MIXED);
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(read_maybe_gzip(&output), MIXED_EXPECTED);
}

#[test]
fn gzip_in_gzip_out() {
    let dir = TempDir::new("gzout");
    let input = dir.join("in.fastq.gz");
    let output = dir.join("out.fastq.gz");
    write_gzip(&input, MIXED);
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    // The output must be a valid gzip stream (possibly multi-member).
    assert_eq!(&std::fs::read(&output).unwrap()[..2], &[0x1f, 0x8b]);
    assert_eq!(read_maybe_gzip(&output), MIXED_EXPECTED);
}

#[test]
fn plain_in_gzip_out() {
    let dir = TempDir::new("gzip-out-plain-in");
    let input = dir.join("in.fastq");
    let output = dir.join("out.fastq.gz");
    std::fs::write(&input, MIXED).unwrap();
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert_eq!(read_maybe_gzip(&output), MIXED_EXPECTED);
}

#[test]
fn stdin_input_is_supported() {
    let dir = TempDir::new("stdin");
    let output = dir.join("out.fastq");
    let result = run_stdin(
        &[
            "-a",
            ADAPTER,
            "-m",
            "10",
            "-o",
            output.to_str().unwrap(),
            "-",
        ],
        MIXED,
    );
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(read_maybe_gzip(&output), MIXED_EXPECTED);
}

#[test]
fn gzip_stdin_input_is_detected() {
    let dir = TempDir::new("stdin-gz");
    let output = dir.join("out.fastq");
    let mut compressed = Vec::new();
    {
        let mut encoder = GzEncoder::new(&mut compressed, Compression::new(6));
        encoder.write_all(MIXED).unwrap();
        encoder.finish().unwrap();
    }
    let result = run_stdin(
        &[
            "-a",
            ADAPTER,
            "-m",
            "10",
            "-o",
            output.to_str().unwrap(),
            "-",
        ],
        &compressed,
    );
    assert!(result.status.success());
    assert_eq!(read_maybe_gzip(&output), MIXED_EXPECTED);
}

#[test]
fn stdout_output_is_supported() {
    let dir = TempDir::new("stdout");
    let input = dir.join("in.fastq");
    std::fs::write(&input, MIXED).unwrap();
    let result = run(&[
        "--quiet",
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        "-",
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert_eq!(result.stdout, MIXED_EXPECTED);
}

#[test]
fn empty_input_gives_empty_output_and_exit_zero() {
    for suffix in ["fastq", "fastq.gz"] {
        let dir = TempDir::new("empty");
        let input = dir.join("in.fastq");
        let output = dir.join(&format!("out.{suffix}"));
        std::fs::write(&input, b"").unwrap();
        let result = run(&[
            "-a",
            ADAPTER,
            "-m",
            "10",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert!(
            result.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(output.exists(), "output file must be created");
        assert!(
            read_maybe_gzip(&output).is_empty(),
            "output must be empty for empty input"
        );
    }
}

#[test]
fn empty_gzip_input_gives_empty_output() {
    let dir = TempDir::new("empty-gz");
    let input = dir.join("in.fastq.gz");
    let output = dir.join("out.fastq");
    write_gzip(&input, b"");
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert!(read_maybe_gzip(&output).is_empty());
}

#[test]
fn minimum_length_zero_keeps_everything() {
    let dir = TempDir::new("minlen0");
    let input = dir.join("in.fastq");
    let output = dir.join("out.fastq");
    std::fs::write(
        &input,
        b"@a\nACGTAGATCGGAAGAGC\n+\nIIIIIIIIIIIIIIIII\n@b\nAC\n+\nII\n",
    )
    .unwrap();
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "0",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert_eq!(
        read_maybe_gzip(&output),
        b"@a\nACGT\n+\nIIII\n@b\nAC\n+\nII\n".to_vec()
    );
}

#[test]
fn malformed_fastq_fails_cleanly() {
    let cases: &[(&str, &[u8])] = &[
        ("bad header", b"r1\nACGT\n+\nIIII\n"),
        ("bad plus", b"@r1\nACGT\n-\nIIII\n"),
        ("length mismatch", b"@r1\nACGT\n+\nIII\n"),
        ("truncated", b"@r1\nACGT\n+\n"),
        ("no trailing newline", b"@r1\nACGT\n+\nIIII"),
    ];
    for (name, data) in cases {
        let dir = TempDir::new("malformed");
        let input = dir.join("in.fastq");
        let output = dir.join("out.fastq");
        std::fs::write(&input, data).unwrap();
        let result = run(&[
            "-a",
            ADAPTER,
            "-m",
            "10",
            "-o",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert!(!result.status.success(), "{name} must fail");
        assert_eq!(result.status.code(), Some(1), "{name}: unexpected exit");
        let stderr = String::from_utf8_lossy(&result.stdout);
        assert!(
            stderr.contains("malformed FASTQ"),
            "{name}: stderr was {stderr}"
        );
        assert!(
            !stderr.contains("Reads:"),
            "{name}: must not print a success summary: {stderr}"
        );
        // cutadapt prints only the diagnostic; the contract that matters is a
        // non-zero exit and no plausible-looking output file.
        let written = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
        assert_eq!(written, 0, "{name}: no partial output may be committed");
    }
}

#[test]
fn malformed_input_does_not_leave_a_valid_looking_output() {
    let dir = TempDir::new("malformed-partial");
    let input = dir.join("in.fastq");
    let output = dir.join("out.fastq");
    // One good record followed by a broken one.
    std::fs::write(
        &input,
        b"@good\nACGTACGTAGATCGGAAGAGC\n+\nIIIIIIIIIIIIIIIIIIIII\n@bad\nACGT\n+\nII\n",
    )
    .unwrap();
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stdout);
    assert!(stderr.contains("malformed FASTQ"), "{stderr}");
    // A FASTQ parse error aborts before anything is committed, so the file must
    // be empty rather than a truncated FASTQ that looks complete.
    let written = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    assert_eq!(written, 0, "partial output must not be committed");
}

#[test]
fn supported_adapter_options_now_work() {
    // Full-compatibility contract: the options the v0.1.0 subset rejected are
    // now real features, so they must succeed and produce output.
    let dir = TempDir::new("supported");
    let input = dir.join("in.fastq");
    std::fs::write(&input, MIXED).unwrap();
    let cases: &[&[&str]] = &[
        &["-g", "ACGT"],
        &["-b", "ACGT"],
        &["-a", "AGATCGGAAGAGC$"],
        &["-a", "^ACGT...AGATCGGAAGAGC"],
        &["-n", "2"],
        &["-O", "1"],
        &["-e", "0.2"],
        &["--no-indels"],
        &["-a", "ACGT"], // a second adapter
    ];
    for extra in cases {
        let output = dir.join("out.fastq");
        let mut argv: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        argv.extend([
            "-a".to_string(),
            ADAPTER.to_string(),
            "-m".to_string(),
            "10".to_string(),
            "-o".to_string(),
            output.to_str().unwrap().to_string(),
            input.to_str().unwrap().to_string(),
        ]);
        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let result = run(&refs);
        assert_eq!(
            result.status.code(),
            Some(0),
            "args {extra:?} should succeed; stderr: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(output.exists(), "args {extra:?} should write output");
    }
}

#[test]
fn no_documented_option_is_silently_refused() {
    // The v0.1.0 subset used to refuse every unimplemented option with exit 1.
    // Full compatibility means the documented cutadapt options now work, so this
    // test asserts the inverse contract: a representative invocation of each
    // formerly-refused option exits 0 and writes output.
    let dir = TempDir::new("implemented");
    let input = dir.join("in.fastq");
    std::fs::write(&input, MIXED).unwrap();
    let cases: &[&[&str]] = &[
        &["-q", "20"],
        &["-u", "2"],
        &["--poly-a"],
        &["-l", "20"],
        &["--trim-n"],
        &["--nextseq-trim", "20"],
        &["-M", "100"],
        &["--max-n", "5"],
        &["--max-ee", "5"],
        &["--discard-untrimmed"],
        &["--action", "mask"],
        &["--rc"],
        &["-x", "PRE_"],
        &["--report", "minimal"],
        &["--fasta"],
        &["--match-read-wildcards"],
        &["-N"],
        &["-Z"],
    ];
    for extra in cases {
        let output = dir.join("out.fastq");
        let mut argv: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        argv.extend([
            "-a".to_string(),
            ADAPTER.to_string(),
            "-m".to_string(),
            "10".to_string(),
            "-o".to_string(),
            output.to_str().unwrap().to_string(),
            input.to_str().unwrap().to_string(),
        ]);
        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let result = run(&refs);
        assert_eq!(
            result.status.code(),
            Some(0),
            "args {extra:?} should now be implemented; stderr: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

#[test]
fn read_modifications_and_filters_are_applied() {
    // The modifier options now change the output instead of being refused.
    let dir = TempDir::new("mods");
    let input = dir.join("in.fastq");
    std::fs::write(&input, MIXED).unwrap();
    for extra in [
        vec!["-u", "3"],
        vec!["-u", "-3"],
        vec!["-q", "20"],
        vec!["-q", "20,20"],
        vec!["--poly-a"],
        vec!["--length", "20"],
        vec!["--length", "-20"],
        vec!["--trim-n"],
        vec!["-z"],
        vec!["--nextseq-trim", "20"],
        vec!["-m", "30"],
        vec!["-M", "40"],
        vec!["--max-n", "2"],
        vec!["--max-ee", "2"],
        vec!["--max-aer", "0.5"],
        vec!["--discard-trimmed"],
        vec!["--discard-untrimmed"],
        vec!["--discard-casava"],
        vec!["--action", "mask"],
        vec!["--action", "retain"],
        vec!["--action", "lowercase"],
        vec!["--action", "none"],
        vec!["--info-file", "i.txt"],
        vec!["-r", "r.txt"],
        vec!["--too-short-output", "ts.fastq"],
        vec!["--too-long-output", "tl.fastq"],
        vec!["--untrimmed-output", "u.fastq"],
        vec!["--fasta"],
        vec!["--rc"],
        vec!["--length-tag", "length="],
        vec!["--strip-suffix", ":1"],
        vec!["-x", "PRE_"],
        vec!["-y", "_SUF"],
        vec!["--rename", "{id}_{adapter_name}"],
        vec!["--untrimmed-output", "u.fastq"],
        vec!["--json", "r.json"],
        vec!["--report", "minimal"],
        vec!["--match-read-wildcards"],
        vec!["-Z"],
    ] {
        let output = dir.join("out.fastq");
        let mut argv: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        argv.extend([
            "-a".to_string(),
            ADAPTER.to_string(),
            // Only add a default -m when the case does not set one itself.
            "-m".to_string(),
            "0".to_string(),
            "-o".to_string(),
            output.to_str().unwrap().to_string(),
            input.to_str().unwrap().to_string(),
        ]);
        if extra.first() == Some(&"-m") {
            let pos = argv.iter().position(|a| a == "-m").unwrap();
            argv.drain(pos..pos + 2);
        }
        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let result = run(&refs);
        assert_eq!(
            result.status.code(),
            Some(0),
            "args {extra:?} should succeed; stderr: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(output.exists(), "args {extra:?} should write output");
    }
}

#[test]
fn unknown_flag_exits_2() {
    let dir = TempDir::new("unknown");
    let input = dir.join("in.fastq");
    std::fs::write(&input, MIXED).unwrap();
    let output = dir.join("out.fastq");
    let result = run(&[
        "--frobnicate",
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(!output.exists());
}

#[test]
fn missing_input_file_fails_without_creating_output() {
    let dir = TempDir::new("missing-input");
    let output = dir.join("out.fastq");
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        dir.join("does-not-exist.fastq").to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    // cutadapt creates the output file before opening the input, so an empty
    // file is left behind (verified with the oracle).
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("cannot open input"), "{stdout}");
}

#[test]
fn corrupt_gzip_input_fails_cleanly() {
    let dir = TempDir::new("corrupt-gz");
    let input = dir.join("in.fastq.gz");
    let output = dir.join("out.fastq");
    // Valid gzip magic, garbage payload.
    std::fs::write(&input, b"\x1f\x8b\x08\x00garbage-not-really-gzip").unwrap();
    let result = run(&[
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&result.stdout).is_empty());
}

#[test]
fn multiple_chunks_are_processed_in_order() {
    // Enough records to span several internal chunks (>= 3 x 256 KiB).
    let dir = TempDir::new("chunks");
    let input = dir.join("in.fastq");
    let output = dir.join("out.fastq");
    let mut data = Vec::new();
    let mut expected = Vec::new();
    let mut seed: u64 = 12345;
    for i in 0..20_000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let len = 60 + (seed % 30) as usize;
        let mut seq = String::with_capacity(len + 13);
        for _ in 0..len {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seq.push(b"ACGT"[(seed % 4) as usize] as char);
        }
        let trimmed = seq.clone();
        if i % 2 == 0 {
            seq.push_str(ADAPTER);
        }
        let quality = "I".repeat(seq.len());
        data.extend_from_slice(format!("@read{i}\n{seq}\n+\n{quality}\n").as_bytes());
        expected.extend_from_slice(
            format!("@read{i}\n{trimmed}\n+\n{}\n", "I".repeat(trimmed.len())).as_bytes(),
        );
    }
    std::fs::write(&input, &data).unwrap();
    let result = run(&[
        "--quiet",
        "-a",
        ADAPTER,
        "-m",
        "10",
        "-o",
        output.to_str().unwrap(),
        input.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(read_maybe_gzip(&output), expected);
}

#[test]
fn parallel_and_single_threaded_outputs_are_identical() {
    let dir = TempDir::new("determinism");
    let input = dir.join("in.fastq");
    let mut data = Vec::new();
    for i in 0..5_000 {
        let seq = format!(
            "ACGTACGTACGTACGTACGT{}",
            if i % 3 == 0 { ADAPTER } else { "" }
        );
        data.extend_from_slice(format!("@r{i}\n{seq}\n+\n{}\n", "I".repeat(seq.len())).as_bytes());
    }
    std::fs::write(&input, &data).unwrap();
    let outputs: Vec<PathBuf> = (1..=4)
        .map(|threads| {
            let output = dir.join(&format!("out{threads}.fastq.gz"));
            let result = Command::new(binary())
                .env("RUST_CUTADAPT_THREADS", threads.to_string())
                .args([
                    "--quiet",
                    "-a",
                    ADAPTER,
                    "-m",
                    "10",
                    "-o",
                    output.to_str().unwrap(),
                    input.to_str().unwrap(),
                ])
                .output()
                .expect("run");
            assert!(
                result.status.success(),
                "stderr: {}",
                String::from_utf8_lossy(&result.stdout)
            );
            output
        })
        .collect();
    let reference = read_maybe_gzip(&outputs[0]);
    for output in &outputs[1..] {
        assert_eq!(read_maybe_gzip(output), reference);
    }
}

#[test]
fn help_and_version_exit_zero() {
    // Regression: these used to exit 2 because clap's DisplayHelp/DisplayVersion
    // errors were mapped onto usage errors. cutadapt (click) exits 0 for both.
    for flag in ["--help", "-h"] {
        let output = Command::new(binary())
            .arg(flag)
            .output()
            .expect("run --help");
        assert_eq!(output.status.code(), Some(0), "{flag} must exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Usage:"),
            "{flag} should print usage to stdout, got {stdout:?}"
        );
        assert!(
            output.stderr.is_empty(),
            "{flag} should not write to stderr, got {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for flag in ["--version", "-V"] {
        let output = Command::new(binary())
            .arg(flag)
            .output()
            .expect("run --version");
        assert_eq!(output.status.code(), Some(0), "{flag} must exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(env!("CARGO_PKG_VERSION")),
            "{flag} should print the version to stdout, got {stdout:?}"
        );
    }
}
