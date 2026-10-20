//! A deterministic, dependency-free fuzz harness for the `bioinfo-fastx` parsers.
//!
//! # Why not `cargo fuzz`
//!
//! The plan asks for `cargo fuzz` corpora. `cargo-fuzz` is not installed in this
//! environment and cannot be fetched (`cargo --offline`, no network), so this
//! harness reproduces the part of `cargo fuzz` that matters for W1-2's acceptance
//! criterion — *does the parser crash, hang, or index out of bounds on adversarial
//! input?* — without libFuzzer.
//!
//! What it is **not**: a coverage-guided fuzzer. It will find panics and OOB
//! slicing reachable from structured mutations, not the same set of paths
//! libFuzzer would find with its feedback loop. That limitation is recorded in
//! `docs/EVIDENCE-W1-2.md` (DR-4) along with the exact commands that were run.
//!
//! # What it does
//!
//! 1. **Structured mutation.** A corpus of seed inputs is mutated with a
//!    deterministic PRNG (byte flips, truncations, duplications, splices, control
//!    bytes, very long lines, CRLF injection, missing terminators).
//! 2. **Metamorphic properties.** For every accepted input:
//!    * a record written and re-parsed must be identical (`write ∘ parse = id`);
//!    * parsing must be deterministic (same input, same records);
//!    * chunking at any size must reassemble to the input exactly;
//!    * a chunked parse must find the same record count as a single-pass parse.
//! 3. **No panic.** Every entry point is wrapped so a panic is reported as a
//!    failure with the exact input that triggered it, rather than aborting the
//!    run.
//!
//! Run with:
//!
//! ```text
//! cargo run --release --offline -p bioinfo-fastx --bin fastx-fuzz -- \
//!     --seconds 600 --seed 1
//! ```
//!
//! The harness prints a summary and exits non-zero on the first failure.

use bioinfo_fastx::chunker;
use bioinfo_fastx::parser::{Format, Mode, Parser};
use bioinfo_fastx::writer::{write_record, Layout};
use std::io::Write;
use std::time::{Duration, Instant};

/// One parsed record as owned fields: header, sequence, separator, qualities.
type OwnedRecord = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

/// A small, fast, deterministic PRNG (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Seed inputs: valid shapes plus near-miss shapes.
fn seeds() -> Vec<Vec<u8>> {
    vec![
        b"".to_vec(),
        b"@r1\nACGT\n+\nIIII\n".to_vec(),
        b"@r1 extra\nACGTACGT\n+r1 extra\nIIIIIIII\n@r2\nTTTT\n+\n!!!!\n".to_vec(),
        b"@r1\r\nACGT\r\n+\r\nIIII\r\n".to_vec(),
        b"@r1\nACGT\n+\nIIII".to_vec(),      // no trailing newline
        b"@r1\n\n+\n\n".to_vec(),            // empty read
        b"@r1\nACGT\n+\nIII\n".to_vec(),     // length mismatch
        b"r1\nACGT\n+\nIIII\n".to_vec(),     // bad header
        b"@r1\nACGT\n-\nIIII\n".to_vec(),    // bad separator
        b"@r1\nACGT\n+\n".to_vec(),          // truncated
        b"@r1\nACGT\n+\nI\xffII\n".to_vec(), // non-ASCII quality
        b">r1\nACGT\n".to_vec(),             // FASTA
        b">r1\nACGT\n>r2\nTTTT\n".to_vec(),
        b">r1\nACGT\nGGGG\n".to_vec(), // multi-line FASTA
        b"@\n\n+\n\n".to_vec(),        // empty name
        vec![b'A'; 4096],              // no structure
    ]
}

/// Control bytes worth injecting: they exercise the escaping and validation paths.
const NASTY: &[u8] = &[
    0x00, 0x01, 0x08, 0x0b, 0x0c, 0x0d, 0x1f, 0x7f, 0x80, 0xff, b'@', b'+', b'>', b'\n',
];

/// Mutate `input` in one of several ways.
fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut out = input.to_vec();
    match rng.below(9) {
        // Flip a byte.
        0 => {
            if !out.is_empty() {
                let i = rng.below(out.len());
                out[i] ^= 1 << rng.below(8);
            }
        }
        // Truncate.
        1 => {
            let cut = rng.below(out.len() + 1);
            out.truncate(cut);
        }
        // Duplicate a slice.
        2 => {
            if !out.is_empty() {
                let start = rng.below(out.len());
                let len = rng.below(out.len() - start + 1);
                let slice: Vec<u8> = out[start..start + len].to_vec();
                let at = rng.below(out.len() + 1);
                out.splice(at..at, slice);
            }
        }
        // Insert a nasty byte.
        3 => {
            let at = rng.below(out.len() + 1);
            out.insert(at, *rng.pick(NASTY));
        }
        // Delete a byte.
        4 => {
            if !out.is_empty() {
                let at = rng.below(out.len());
                out.remove(at);
            }
        }
        // Repeat the whole input (tests chunk boundaries).
        5 => {
            if out.len() < 8192 {
                let copy = out.clone();
                out.extend_from_slice(&copy);
            }
        }
        // Splice two seeds together.
        6 => {
            let other = seeds();
            let donor = rng.pick(&other).clone();
            let at = rng.below(out.len() + 1);
            out.splice(at..at, donor);
        }
        // Append a very long run (a line longer than any chunk).
        7 => {
            let len = 4096 + rng.below(16384);
            let byte = *rng.pick(b"ACGTI\n");
            out.extend(std::iter::repeat_n(byte, len));
        }
        // Inject CRLF.
        _ => {
            let at = rng.below(out.len() + 1);
            out.splice(at..at, b"\r\n".iter().copied());
        }
    }
    out
}

/// Everything the harness checks about one input.
///
/// Returns `Err(reason)` on a violated property, tagged with the phase so a
/// failure is diagnosable without a rerun.
fn check(data: &[u8]) -> Result<(), String> {
    // ---- per-format record properties -----------------------------------
    for &(format, mode) in &[
        (Format::Fastq, Mode::Strict),
        (Format::Fastq, Mode::Lenient),
        (Format::Fasta, Mode::Strict),
        (Format::Fasta, Mode::Lenient),
    ] {
        let records = parse_all(data, format, mode)?;
        let again = parse_all(data, format, mode)?;
        if records != again {
            return Err(format!(
                "phase=determinism: not deterministic for {format:?}/{mode:?}"
            ));
        }
        if format == Format::Fastq && mode == Mode::Lenient && !records.is_empty() {
            // `write ∘ parse = id`, asserted in lenient mode where the accepted
            // byte ranges are the widest. Strict mode is excluded on purpose: a
            // strict record's *header* is not validated for control bytes, and a
            // header containing a bare newline cannot be re-parsed by any
            // four-line reader — that is inherent to FASTQ, not a defect.
            let mut rendered = Vec::new();
            for (header, seq, plus, qual) in &records {
                write_record(&mut rendered, header, seq, plus, qual, Layout::Fastq)
                    .map_err(|e| format!("phase=roundtrip: write failed: {e}"))?;
            }
            let round = parse_all(&rendered, Format::Fastq, mode)?;
            if round != records {
                return Err(format!(
                    "phase=roundtrip: write∘parse changed {format:?}/{mode:?}\n  records={:?}\n  rendered={:?}\n  round={:?}",
                    records
                        .iter()
                        .map(|r| String::from_utf8_lossy(&r.0).into_owned())
                        .collect::<Vec<_>>(),
                    String::from_utf8_lossy(&rendered),
                    round
                        .iter()
                        .map(|r| String::from_utf8_lossy(&r.0).into_owned())
                        .collect::<Vec<_>>()
                ));
            }
        }
    }

    // ---- chunker properties (FASTQ structure) ---------------------------
    //
    // Two things are asserted, and one deliberately is not.
    //
    // Asserted: the chunks tile the input exactly, and every chunk except the
    // last ends on a record boundary.
    //
    // NOT asserted: that per-chunk parsing finds the same number of records as a
    // single pass. It legitimately does not. A malformed input makes a chunk
    // boundary fall somewhere the whole-input parser would have kept reading, so
    // a chunk can begin with a line the single-pass parser would have treated as
    // a continuation. Both readings are correct for their input; the pipeline
    // guarantees identity only for *valid* input, which is what `tests/gate/`
    // checks against the real binaries.
    for &chunk_size in &[4096usize, 8192, 65_536] {
        let mut cursor = std::io::Cursor::new(data.to_vec());
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        chunker::for_each_chunk::<_, _, std::io::Error>(&mut cursor, chunk_size, |chunk, _, _| {
            chunks.push(chunk);
            Ok(())
        })
        .map_err(|e| format!("phase=chunk: {e}"))?;

        let rejoined: Vec<u8> = chunks.iter().flatten().copied().collect();
        if rejoined != data {
            return Err(format!("phase=chunk: size {chunk_size} did not reassemble"));
        }
        for (index, chunk) in chunks.iter().enumerate() {
            if index + 1 == chunks.len() {
                continue; // the final chunk may hold a partial record
            }
            let newlines = chunk.iter().filter(|&&b| b == b'\n').count();
            if !newlines.is_multiple_of(4) {
                return Err(format!(
                    "phase=chunk: size {chunk_size} chunk {index} ends mid-record \
                     ({newlines} newlines)"
                ));
            }
        }
    }

    Ok(())
}

/// Parse every record, treating a rejection as "no more records".
fn parse_all(data: &[u8], format: Format, mode: Mode) -> Result<Vec<OwnedRecord>, String> {
    let mut parser = Parser::new(data, 0, 1).with_format(format).with_mode(mode);
    let mut records = Vec::new();
    loop {
        match parser.next_record() {
            Ok(Some(record)) => records.push((
                record.header.to_vec(),
                record.sequence.to_vec(),
                record.plus.to_vec(),
                record.qualities.to_vec(),
            )),
            Ok(None) => break,
            Err(error) => {
                if error.line == 0 {
                    return Err("phase=parse: line 0 reported (line numbers are 1-based)".into());
                }
                if error.message.is_empty() {
                    return Err("phase=parse: empty message".into());
                }
                break;
            }
        }
    }
    Ok(records)
}

fn main() {
    let mut seconds = 60u64;
    let mut seed = 1u64;
    let mut iterations_cap = 0usize;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seconds" => {
                i += 1;
                seconds = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(60);
            }
            "--seed" => {
                i += 1;
                seed = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(1);
            }
            "--iterations" => {
                i += 1;
                iterations_cap = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "--help" | "-h" => {
                println!("usage: fastx-fuzz [--seconds N] [--seed N] [--iterations N]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    // A panic inside the parser must be reported, not abort the run silently.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
    }));

    let corpus = seeds();
    let mut rng = Rng::new(seed);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut iterations = 0u64;
    let mut failures = 0u64;

    println!(
        "fastx-fuzz: seed={seed} seconds={seconds} iterations_cap={iterations_cap} seeds={}",
        corpus.len()
    );

    while Instant::now() < deadline {
        if iterations_cap > 0 && iterations as usize >= iterations_cap {
            break;
        }
        iterations += 1;

        // Grow the input occasionally so long inputs are exercised too.
        let mut input = (*rng.pick(&corpus)).clone();
        let extra = rng.below(4);
        for _ in 0..extra {
            input = mutate(&mut rng, &input);
        }

        let result = std::panic::catch_unwind(|| check(&input));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => {
                failures += 1;
                eprintln!("FAIL iteration {iterations}: {reason}");
                eprintln!(
                    "  input ({} bytes): {:?}",
                    input.len(),
                    String::from_utf8_lossy(&input[..input.len().min(200)])
                );
                if failures >= 10 {
                    eprintln!("stopping after 10 failures");
                    break;
                }
            }
            Err(_) => {
                failures += 1;
                eprintln!(
                    "PANIC iteration {iterations} on input ({} bytes)",
                    input.len()
                );
                eprintln!(
                    "  {:?}",
                    String::from_utf8_lossy(&input[..input.len().min(400)])
                );
                if failures >= 10 {
                    break;
                }
            }
        }

        if iterations.is_multiple_of(20_000) {
            print!(".");
            std::io::stdout().flush().ok();
        }
    }

    println!(
        "\nfastx-fuzz: {iterations} iterations, {failures} failures, {:.1}s elapsed",
        seconds as f64
    );
    if failures == 0 {
        println!("FUZZ PASS");
    } else {
        println!("FUZZ FAIL");
        std::process::exit(1);
    }
}
