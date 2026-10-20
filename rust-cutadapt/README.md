# rust-cutadapt

A Rust implementation of the documented **cutadapt 4.6 command-line surface**
for adapter trimming of FASTQ and FASTA reads.

This README describes the current binary. The original v0.1.0 subset text is
preserved in [docs/HISTORY.md](docs/HISTORY.md); it does not describe the
current tree. The exact option table, comparison rules and known deviations
live in [docs/FULL-COMPATIBILITY.md](docs/FULL-COMPATIBILITY.md).

## Status

At the current tree (code commit `c8b0249`; this README is a later documentation-only change):

| Check | Result |
| --- | --- |
| Documented cutadapt 4.6 options | 60, all implemented |
| Differential cases against real cutadapt 4.6 | 119 / 119 pass |
| Rust tests | 208 pass (171 lib + 14 CLI + 21 CLI end-to-end + 1 differential + 1 doc-test) |
| `cargo fmt` / `cargo clippy --all-targets -- -D warnings` | clean |
| Source tree | no tags, no release published for this tree |

The differential harness compares exit status, decompressed read data,
auxiliary outputs, the normalised text report and the JSON report. It uses a
real cutadapt 4.6 binary as the oracle; it is not a reimplementation-only
self-check.

## What it implements

### Adapters and matching

- `-a` / `--adapter` (3'), `-g` / `--front` (5'), `-b` / `--anywhere`
- `-A`, `-G`, `-B` for R2, including `--pair-adapters`
- repeatable adapters, `file:FILE` adapter lists, named adapters and
  `{name}`/`{id}`/`{adapter_name}` templates
- `^` and `$` anchors, linked adapters, IUPAC wildcards and
  `--match-read-wildcards` / `-N`
- `-e` / `--error-rate` (rate or absolute count), indels by default and
  `--no-indels`, `-O` / `--overlap`, `-n` / `--times`
- `--action {trim,retain,mask,lowercase,none}`, `--revcomp` / `--rc`

### Read modifications

`-u`/`-U`, `-q`/`-Q`, `--nextseq-trim`, `--quality-base`, `--poly-a`, `-l` /
`--length`, `--trim-n`, `--zero-cap`, `--length-tag`, `--strip-suffix`,
`-x`/`--prefix`, `-y`/`--suffix`, `--rename`.

### Filtering

`-m`/`--minimum-length` (including `LEN:LEN2`), `-M`/`--maximum-length`,
`--max-n`, `--max-expected-errors` / `--max-ee`,
`--max-average-error-rate` / `--max-aer`, `--discard-trimmed` /
`--discard`, `--discard-untrimmed` / `--trimmed-only`,
`--discard-casava`, and `--pair-filter {any,both,first}`.

### Input, output and reports

- FASTQ and FASTA input; plain, gzip, bzip2 and xz input detected by magic
  bytes
- output to stdout or `-o`/`--output`; output compression selected by
  extension, with `-Z` selecting the fast level
- paired-end and interleaved input/output (`-p`, `--interleaved`)
- auxiliary outputs: `--info-file`, `-r`/`--rest-file`,
  `--wildcard-file`, `--too-short-output`, `--too-long-output`,
  `--untrimmed-output`, and the paired equivalents
- demultiplexing through `{name}` in the output name
- text report (`--report full|minimal`) and `--json`
- `-j`/`--cores`; output is deterministic across core counts

`--help` and `--version` exit 0. Usage errors exit 2 and do not leave a
plausible-looking output file. Runtime failures (I/O, malformed input, corrupt
compression, template errors) exit 1.

## Compatibility and known deviations

The full compatibility contract is in
[docs/FULL-COMPATIBILITY.md](docs/FULL-COMPATIBILITY.md). The current
differences from cutadapt 4.6 are limited and deliberate:

| Area | Current behaviour |
| --- | --- |
| Compressed bytes | Decompressed bytes are identical. gzip output defaults to level 3 (cutadapt uses 6), so compressed sizes differ; `-Z` selects level 1 (and the fast bzip2/xz preset). |
| Partial-output diagnostic | If an error happens after some records were written, rust-cutadapt adds a "Note: the output file is incomplete" line. cutadapt does not. |
| `--report` "expect" column | Derived from a pseudo-random stream in cutadapt, so the harness compares that one column with a tolerance; all other report fields are compared exactly. |
| `--max-average-error-rate` on an empty read | cutadapt 4.6 raises `ZeroDivisionError`; rust-cutadapt matches the observable exit code, stdout and diagnostic without a traceback. |
| Oracle quirks | Where cutadapt 4.6 has a documented quirk, the differential suite records the observable behaviour rather than silently "fixing" it. |

No uncompressed read-data or report deviation is currently known.

## Performance

### Current full surface (default 4 workers; measured 2026-10-20, code `c8b0249`)

- workload: 1,000,000 reads, `bench/workload.fastq.gz`
- command: `rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 -o <out> bench/workload.fastq.gz`
- five independent `/usr/bin/time -v` runs: **0.62–0.72 s**, mean **0.672 s**
- peak RSS: **37.5–40.4 MB**
- decompressed output: **byte-identical to cutadapt 4.6**; 999,988 records
- speed-up vs the recorded 4.4363 s baseline: **6.60x**
- speed-up vs a same-session oracle run (9.65 s under load): **14.36x**

The 5x target is met with the default four-worker configuration. cutadapt
defaults to one core; `-j 1` keeps exact single-thread parity but is slower:
2.352 s mean (1.89x baseline, 4.16x same-session), recorded at commit
`281a8dd`. `hyperfine` was not installed, so the raw evidence is five
`/usr/bin/time -v` runs per mode. Raw values and checksums:
[docs/bench/cutadapt-c8b0249.json](docs/bench/cutadapt-c8b0249.json) for the
default mode and [docs/bench/cutadapt-281a8dd.json](docs/bench/cutadapt-281a8dd.json)
for `-j 1`. The default-worker decision is recorded in
[docs/adr/0002-default-worker-count.md](docs/adr/0002-default-worker-count.md).

### Historical v0.1.0 subset

The 6.3x–6.9x figure belongs to the **v0.1.0 subset**, not to the current
tree: cutadapt 4.6 4.4363 s / 24,412 kB; v0.1.0 rust-cutadapt 0.64–0.70 s /
11.6–13.4 MB across six hyperfine sessions. See [BENCHMARK.md](BENCHMARK.md)
and [bench/reference.json](bench/reference.json) for that original method and
baseline. Do not quote the old headline as a property of the current binary.

## Build and test

Requirements: Rust 1.88 or newer. The crate is tested offline.

```bash
cargo build --release --offline
cargo test --release --offline
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
```

The differential suite needs a real cutadapt 4.6 binary:

```bash
# Oracle discovery: --oracle, then CUTADAPT_ORACLE, then well-known
# paths, then PATH.
python3 tests/oracle/differential.py --matrix
python3 tests/oracle/differential.py --self-check
```

`tests/oracle/differential.py --json out.json` writes the per-case matrix.
The oracle used for the recorded 119/119 run was a pinned cutadapt 4.6
binary; the portable declaration is [tests/ci/env.yml](tests/ci/env.yml).

Dependencies are `clap`, `flate2`, `bzip2`, `xz2` and `lzma-sys` (plus the
workspace-local `bioinfo-common`). The bzip2/xz dependencies were added when
those formats were implemented; the older `clap` + `flate2`-only rule is
recorded as superseded in
[docs/adr/0001-dependency-policy.md](docs/adr/0001-dependency-policy.md), which
also records why the xz binding moved from `liblzma` to `xz2` when this crate
joined the `bioinfo-tools` workspace (`liblzma-sys` and the `lzma-sys` that
`rust-htslib` needs cannot both be linked).

## Installation

The public distribution repository
[seek-hope/rust-cutadapt-dist](https://github.com/seek-hope/rust-cutadapt-dist)
currently contains only **v0.1.0**, built from commit `d9c4c52`. That artifact
predates the full-surface work and does not represent this tree.

For the current tree, build from source:

```bash
cargo install --path . --locked
# or
cargo build --release --offline
./target/release/rust-cutadapt --version
```

The distribution install script is still available for the historical
v0.1.0 artifact, but reproducing the current surface requires a source build
until a new artifact is approved and published.

## Release status

No tag or release is created by this tree. There is no source tag; the v0.1.0
distribution artifact was built from commit `d9c4c52` and is historical. See
[docs/RELEASE-FREEZE.md](docs/RELEASE-FREEZE.md) for the active freeze and its
exit criteria. `.github/workflows/` contains the existing release automation,
but it is not part of the compatibility claim and must not be triggered
without an explicit release decision.

## Repository layout

| Path | Purpose |
| --- | --- |
| `src/cli.rs` | Full option grammar, aliases, value parsers and validation. |
| `src/adapter/` | Adapter tree, anchors, linked adapters, wildcard matching and best-match selection. |
| `src/align.rs` | Semiglobal aligner port with mismatch and indel support. |
| `src/modify.rs` | Read/quality modifications, renaming and `--action`. |
| `src/filter.rs` | Length, N, expected-error and discard filters. |
| `src/pair.rs` | Paired-end and interleaved coordination. |
| `src/fastq.rs` | Strict streaming FASTQ/FASTA parser. |
| `src/compress.rs` | Magic-byte input detection, gzip/bzip2/xz compression. |
| `src/auxout.rs` | Auxiliary output writers. |
| `src/pipeline.rs` | Streaming, bounded-memory pipeline and worker orchestration. |
| `src/report/` | Text and JSON report assembly. |
| `src/runreport.rs` | Run-level counters and report handoff. |
| `tests/cli.rs`, `tests/cli_e2e.rs` | CLI contract and process-level tests. |
| `tests/differential.rs` | Prefilter/aligner equivalence tests. |
| `tests/oracle/differential.py` | Differential harness against cutadapt 4.6. |
| `docs/FULL-COMPATIBILITY.md` | Current compatibility contract and option table. |
| `docs/HISTORY.md` | Historical v0.1.0 README. |
| `docs/reports/` | Recorded differential and baseline matrices. |
| `docs/bench/` | Current full-surface benchmark evidence. |

## License

MIT. See [LICENSE](LICENSE).
