# History: the v0.1.0 README

This is the README as it stood for the original v0.1.0 subset. It is kept for
provenance, not as a description of the current binary. The current surface is
described in [../README.md](../README.md) and
[FULL-COMPATIBILITY.md](FULL-COMPATIBILITY.md). Relative links in the preserved
text point at files relative to this `docs/` directory.

---

# rust-cutadapt

A high-performance Rust implementation of a **documented subset** of
[cutadapt 4.6](https://cutadapt.readthedocs.io/) for single-end FASTQ trimming.

It removes **one 3' adapter** from single-end reads, filters reads that become
too short, and reads/writes plain or gzip-compressed FASTQ. On the trial
workload (1,000,000 reads) it runs **6.3x–6.9x faster than cutadapt 4.6**
(0.64–0.70 s vs 4.44 s wall clock across six measured hyperfine sessions on
this shared machine; the best case was 0.64 s, the slowest 0.70 s) while using
about half of cutadapt's memory — see [BENCHMARK.md](../BENCHMARK.md). Absolute
times vary with machine load, but every measured session stayed well above the
5x target.

## Supported subset

```
rust-cutadapt -a ADAPTER -m MIN_LEN -o OUTPUT INPUT
```

| Option | Meaning |
| --- | --- |
| `-a`, `--adapter` | One 3' adapter sequence. Only `A`, `C`, `G`, `T` (and `U`, converted to `T`) are accepted; upper/lower case both work. |
| `-m`, `--minimum-length` | Discard reads shorter than this **after** trimming. `0` keeps everything. |
| `-o`, `--output` | Output FASTQ. Gzip is used when the name ends in `.gz`. `-` means stdout. |
| `INPUT` | Input FASTQ. Gzip is used when the name ends in `.gz`. `-` means stdin (gzip is auto-detected from the magic bytes). |
| `--quiet` | Suppress the run summary (accepted, no other effect). |
| `-h`, `--help`, `-V`, `--version` | Standard help/version. |

### Matching semantics

The matching engine is a faithful port of cutadapt 4.6's semiglobal aligner
(`src/cutadapt/_align.pyx`, `Aligner` with the `Where.BACK` flags), so it makes
the same decisions cutadapt does for this subset:

- **Error rate 0.1** (`--error-rate` default). Only mismatches are possible in
  the supported subset, and an alignment is accepted when
  `errors <= length * 0.1` for the aligned adapter length.
- **Minimum overlap 3** (`--min-overlap` default): shorter alignments are never
  reported.
- **No indels**: unlike cutadapt's default, this crate implements the
  `--no-indels` model (cutadapt suppresses indels with `indel_cost = 100000`).
  Indel-based alignments are never chosen.
- **Adapter aligned from its 5' end**, so it can match
  - completely in the middle of a read (internal adapter: the read is cut and
    the adapter plus everything after it is removed),
  - partially at the 3' end of a read (only a prefix of the adapter is present),
  - with one mismatch for a 13 bp adapter (13 × 0.1 = 1.3 errors allowed),
  - not at all (the read is written unchanged).
- **At most one adapter removal per read**, as in cutadapt's single-adapter
  mode. A read whose adapter matches at position 0 becomes empty and is then
  discarded by `-m` if `MIN_LEN > 0`.
- Read name (header), `+` line and qualities are preserved byte-for-byte for
  every retained record; only the 3' end of the sequence **and its quality
  string** are cut.
- Matching is case-insensitive (`AGATCGGAAGAGC` also matches
  `agatcggaagagc`), like cutadapt.

Because a k-mer prefilter is used **only** to skip alignments that cannot
possibly succeed, results are identical to running the aligner on every read.
`tests/differential.rs` verifies this on 360,000 randomised reads (including
adapters shorter than the minimum overlap).

## Unsupported behaviour (rejected, never silently ignored)

Anything outside the subset above is refused **before the output file is opened
or written**, with a message on stderr and **exit code 2**:

- paired-end options: `-p/--paired-output`, `-A/--adapter2`, `-G/--front2`,
  `-B/--anywhere2`;
- 5' and anywhere adapters: `-g/--front`, `-b/--anywhere`;
- indels (`--indels`, `--no-indels` is also refused because indels are always
  off in this subset, and any other option that would enable them);
- multiple adapters or multiple rounds: a second `-a`, `-n/--times`;
- quality trimming and quality handling: `-q/--quality-cutoff`,
  `--quality-base`, `--nextseq-trim`, `--zero-cap`, `--double-encode`,
  `--strip-f3`;
- wildcards in the adapter: `-N`, `--no-match-adapter-wildcards`; `N` and other
  IUPAC codes in the `-a` value itself are rejected with exit code 2;
- other filtering/reporting/output options (`--length`, `--max-n`,
  `--discard-*`, `--poly-a`, `--json`, `--info-file`, `-j/--cores`, `-Z`, ...);
- any unknown flag.

The table lives in `src/cli.rs` (`UNSUPPORTED`) and is covered by
`tests/cli.rs` and `tests/cli_e2e.rs`.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success (including empty input → empty output, and `--help`/`--version`). |
| `1` | Runtime failure: unreadable input, corrupt gzip, **malformed FASTQ**. A clear message is printed and no success summary is emitted. |
| `2` | Usage error: missing/invalid arguments, unsupported or unknown options. The output file is not created. |

`--help` and `--version` are successful requests: their text goes to stdout and
the exit code is `0`.

Malformed FASTQ is detected strictly: a header must start with `@`, the
separator with `+`, sequence and quality lengths must match, records must be
complete and terminated by a newline, and sequence/quality bytes must be
printable ASCII. Errors report the line number and byte offset.

## Streaming and memory

The pipeline never loads the input into memory:

1. the input is decompressed and split into 256 KiB **record-aligned** chunks;
2. chunks are trimmed by a small worker pool;
3. chunks are gzip-compressed (one gzip *member* per chunk) or passed through;
4. chunks are written strictly in input order.

Every stage is connected by a bounded channel, so peak RSS stays between about
12 MB and 13 MB on the 1M-read workload (11.6–13.4 MB across measured runs).
Gzip output is a standards-compliant **multi-member** gzip
stream, decoded transparently by `gzip`, `zcat`, `gunzip`, Python's `gzip`
module, `flate2`'s `MultiGzDecoder` and `samtools`. Output is deterministic:
it does not depend on the number of threads.

Output compression uses zlib level 3 by default. Compared with cutadapt's
level-6 output this is about 1.8 % larger but roughly three times faster; see
[BENCHMARK.md](../BENCHMARK.md) for the measurements and the trade-off discussion.
Because a compression-level or thread-count flag is outside the implemented
cutadapt subset, both are controlled by environment variables instead:
`RUST_CUTADAPT_COMPRESSION_LEVEL` (0-9) and `RUST_CUTADAPT_THREADS`
(default: `min(4, available CPUs)`).

## Prebuilt binaries

Prebuilt Linux x86_64 binaries are published in the public distribution
repository [seek-hope/rust-cutadapt-dist](https://github.com/seek-hope/rust-cutadapt-dist).
Installing them needs no Rust toolchain and no clone of this repository:

```bash
curl -fsSL https://raw.githubusercontent.com/seek-hope/rust-cutadapt-dist/main/install.sh | bash
```

The script downloads the latest release, verifies its SHA256 checksum and
installs `rust-cutadapt` into `~/.local/bin` (override with `INSTALL_DIR`).
Pin a version with `RUST_CUTADAPT_VERSION=v0.1.0`, or point at a mirror with
`RUST_CUTADAPT_DIST_REPO`. Only `x86_64-unknown-linux-gnu` is currently
published; other platforms must build from source.

### Release automation

`.github/workflows/release.yml` runs on `v*` tags: it builds, strips and
packages the tarball, uploads it as a workflow artifact, and then publishes to
`seek-hope/rust-cutadapt-dist` using the `DIST_GITHUB_TOKEN` secret. It never
creates a release in this source repository.

The `DIST_GITHUB_TOKEN` secret is configured on this repository with write
access to `seek-hope/rust-cutadapt-dist`, so a `v*` tag publishes there. If the
secret is ever removed, the publish step prints a notice and skips instead of
failing, leaving the tarball as a workflow artifact. The v0.1.0 release in the
dist repository was published manually from a local build of commit `d9c4c52`
before the secret was added.

## Building and testing

```bash
cargo build --release --offline        # target/release/rust-cutadapt
cargo test --release --offline         # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
python3 tests/check.py                 # golden-file conformance (cutadapt 4.6)
```

Dependencies are limited to `clap` and `flate2` (both vendored in the offline
registry). No other crates are used.

## Benchmark method

```bash
mkdir -p /tmp/rc-bench
hyperfine --warmup 1 --runs 3 -N \
  "target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
   -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz"
/usr/bin/time -v target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
  -o /tmp/rc-bench/out.fastq.gz bench/workload.fastq.gz
```

`bench/reference.json` holds the cutadapt 4.6 baseline on the same machine
(`wall_mean_s = 4.4363`, `max_rss_kb = 24412`). Results, including the
compression-level trade-off and how the output was validated, are in
[BENCHMARK.md](../BENCHMARK.md).

## Layout

| Path | Purpose |
| --- | --- |
| `src/align.rs` | Port of cutadapt's semiglobal `Aligner`. |
| `src/trim.rs` | 3' adapter configuration, k-mer prefilter, trim positions. |
| `src/fastq.rs` | Strict streaming FASTQ parser. |
| `src/engine.rs` | Per-chunk trimming and serialization. |
| `src/pipeline.rs` | Streaming, bounded-memory, multi-threaded pipeline. |
| `src/cli.rs` | Argument parsing and unsupported-option rejection. |
| `tests/cli.rs` | CLI parsing and rejection tests (exit code 2). |
| `tests/cli_e2e.rs` | Process-level tests: gzip, empty input, malformed input, `-m`. |
| `tests/differential.rs` | Prefilter-vs-aligner equivalence test. |
