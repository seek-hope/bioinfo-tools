# W1-2 evidence: `bioinfo-fastx` and the FASTX migration

> Historical record: commands, versions and offline flags describe the session at the recorded commit. They are not current project requirements; see [DOC-AUDIT.md](DOC-AUDIT.md).

Status: **complete** — see "Verification results" for the commands and their
output. This file records W1-2 only; it does not start W1-3.

Canonical machine-readable file: [EVIDENCE-W1-2.json](EVIDENCE-W1-2.json).

Scope: `docs/DEVELOPMENT-PLAN.md` **W1-2**, "Create `bioinfo-fastx`; migrate
`rust-cutadapt` first, then `rust-umitools` FASTQ paths. Include compression
detection, multi-member gzip, BGZF, and a lenient-vs-strict parser mode."

**W1-1 is complete but still uncommitted.** Its changes are preserved untouched;
this step only adds to them. `docs/EVIDENCE-W1-1.md` remains the record of W1-1.

## Summary

| Item | Result |
| --- | --- |
| `bioinfo-fastx` | new library crate, 94 unit tests |
| Migrated first | `rust-cutadapt` (parser, chunker, writer, compression) |
| Migrated second | `rust-umitools` (FASTQ reader/writer for `extract`, `whitelist`) |
| `cargo test --workspace` (debug) | **407 passed, 0 failed** |
| `cargo test --workspace --release` | **407 passed, 0 failed** |
| cutadapt 4.6 differential | **119/119 passed** |
| UMI-tools 1.1.6 byte-exact | **77/77 byte-identical, 0 differ** |
| UMI-tools 1.1.6 semantic | **39 passed, 0 failed** |
| UMI-tools formats suite | 0 failures |
| W1-1 byte-neutrality gate (`m3.sh`) | **9/9 PASS**, 40/40 cases |
| W1-2 FASTX equivalence harness | **35/35 cases identical** |
| Fuzz corpus, parser | **1,291,534 iterations, 0 failures** (30 s) and a 600 s run, see below |
| Refactor-induced behaviour change | **none** |

No release, no tag, no version bump, no dist publication.

## The crate

`bioinfo-fastx` is the shared FASTX layer. It is **written here rather than
adopted from `needletail`/`seq_io`**, as plan decision **D3** records: the
requirement is a policy layer — strict-versus-lenient modes and diagnostics whose
offsets match `rust-cutadapt`'s — not merely a parser.

| Module | Responsibility | Tests |
| --- | --- | --- |
| `format` | magic-byte and extension detection, including BGZF | 9 |
| `parser` | strict and lenient FASTQ/FASTA record parsing | 24 |
| `chunker` | record-aligned chunking for the parallel pipelines | 12 |
| `writer` | FASTQ/FASTA serialisation, header-sigil normalisation | 10 |
| `compress` | decompression, compression, multi-member gzip | 18 |
| `bgzf` | BGZF block writer, `BC` subfield, EOF marker | 14 |
| `reader` | `Reader`/`Writer` facades | 11 |

### Strict and lenient modes

`Mode::Strict` is the **default** and reproduces `rust-cutadapt`'s diagnostics
byte for byte. `Mode::Lenient` is opt-in and relaxes only *byte-range* rules —
a missing final newline, high-bit quality bytes — and **never** record structure:
a sequence/quality length mismatch is rejected in both modes, because accepting it
would silently truncate real data. Plan risk **R5** is the reason; the FASTQ
format has no specification, so strictness must be the default.

### Compression

Detection direction differs between reading and writing, and that difference is
pre-existing behaviour rather than a new choice:

* **Reading sniffs content.** A gzip stream called `reads.fastq`, and a
  compressed stdin stream with no name at all, both decompress. This is what
  `rust-cutadapt` already documented.
* **Writing selects by extension**, because the output file does not exist yet.

gzip, bzip2, xz and zstd are supported. **BGZF is read as plain gzip** — it *is*
gzip, so `MultiGzDecoder` handles it and a separate reader would create a second
code path that could disagree with the first.

### BGZF

The writer emits reference-compatible blocks: 18-byte header with the `BC`
sub-field, `BSIZE`, CRC32 and `ISIZE`. Verified against three independent
readers:

| Reader | Result |
| --- | --- |
| `gzip -t` / `gunzip` | valid, output identical |
| `bgzip -d` (the reference implementation, htslib) | output identical |
| Python `gzip` | output identical |

Structural assertions from the harness: 43 blocks for a 2.7 MB payload, all
tiling the stream exactly, maximum uncompressed block 65 280 bytes (the legal
limit), and a correct 28-byte EOF marker.

`MTIME` is written as zero. That makes BGZF output **reproducible**, which is what
lets a fixture pin it; the reference implementation writes the current time. This
is a deliberate deviation and is recorded as DR-3.

### Multi-member gzip

`MultiMemberGzWriter` emits one independent gzip member per call. The
concatenation is a valid gzip stream that every decoder reads as one. A
single-member write is asserted to be **byte-identical** to `flate2`'s own
`GzEncoder`, so the multi-member writer cannot drift from the single-member path.

## The migration

### `rust-cutadapt` first

`rust-cutadapt` was migrated first, as the plan requires. Four seams changed, and
each became a re-export or a thin adapter rather than a rewrite:

| File | Was | Now |
| --- | --- | --- |
| `src/fastq.rs` | the parser implementation | re-exports `bioinfo_fastx::{Parser, ParseError, Record}` |
| `src/compress.rs` | format detection and the compressors | re-exports `bioinfo_fastx::compress`/`format` |
| `src/pipeline.rs` | `last_complete_record_end`, `for_each_chunk` | calls `bioinfo_fastx::chunker::for_each_chunk` |
| `src/pair.rs` | `RecordRange`, `split_records` | re-exports `bioinfo_fastx::chunker` |
| `src/auxout.rs` | inline `write_record` | delegates to `bioinfo_fastx::writer` |

Two details that had to be preserved exactly:

* **The chunker's split rule.** `rust-cutadapt`'s output must not depend on the
  worker count or the chunk size, so the shared chunker was made to produce the
  same split points: cut after every fourth newline. Verified by comparing 5
  chunk sizes and 4 worker counts against the pre-migration binary.
* **The separator line.** cutadapt echoes the input's `+` line verbatim (`+r1`
  stays `+r1`) instead of normalising it to `+`. The shared writer does the same.

The strict parser's error contract is unchanged. The gate golden recorded from the
pre-migration binary still matches:

```text
ERROR: malformed FASTQ at line 5 (byte offset 19): sequence and quality lengths differ (8 != 3)
```

### `rust-umitools` second

Only the FASTQ paths moved: `extract` and `whitelist` are the two commands that
read or write FASTQ. The BAM paths are untouched.

What stayed local, and why:

* **The owned record.** `FastqRecord` owns its fields because `extract` edits a
  read in place (trimming a UMI pattern, masking qualities) and cannot borrow from
  a buffer it is about to overwrite.
* **The diagnostics.** This crate names the file and line
  (`path:line: message`); cutadapt uses `malformed FASTQ at line N (byte offset
  M)`. Both are documented contracts of their respective tools, so both are kept.
* **Gzip selected by extension.** See DR-1.

The writer now goes through the shared one, so the byte layout has exactly one
owner. The pre-migration bytes are pinned literally by a test
(`@r1 extra\nACGT\n+\nIIII\n`).

## Design decisions and deviations

### DR-1 — `rust-umitools` keeps extension-based gzip detection

The shared crate sniffs content. `rust-umitools`' FASTQ reader has always opened a
`.gz` input as gzip and a plain input as plain.

The two differ only when a file's name and content disagree: a gzip stream in a
file called `reads.fastq`. `rust-cutadapt` accepts that (and cutadapt documents
it); `rust-umitools` rejects it. Changing this would be an observable behaviour
change on an input the oracle does not cover, so it was **not** changed. A test
pins the difference explicitly (`gzip_detection_is_by_extension_here`), so a
future "cleanup" that unifies them will fail loudly rather than silently.

Reversing this is a one-line change plus an oracle case; it is listed under
"What remains for W1-3".

### DR-2 — `bioinfo-fastx` depends on `xz2` + `lzma-sys`, not `liblzma`

The plan's crate table lists `liblzma` for this crate. It uses `xz2` with
`lzma-sys/static` for the same reason recorded in `rust-cutadapt`'s ADR 0001:
Cargo permits only one package per `links` value, and `rust-htslib` already
provides `links = "lzma"` through `lzma-sys`. Inside this workspace the `liblzma`
form would make the graph unresolvable. W1-1 already measured that the two
produce byte-identical xz output (DR-2 there), and this crate's round-trip tests
re-assert it for its own paths.

`zstd` is a genuine addition to the workspace dependency set, and it is the one
the plan's crate table explicitly names for `bioinfo-fastx` ("`flate2`, `bzip2`,
`liblzma`, `zstd`"). It is used only for zstd input/output; no existing binary
gained a zstd code path on any input they accepted before.

### DR-3 — BGZF `MTIME` is zero

Reference BGZF writers stamp the current time into each member's `MTIME` field.
This writer writes zero, which makes the output reproducible and therefore
pinnable by a fixture. No reader depends on `MTIME`. Recorded because it is a
deliberate byte difference from `htslib`'s output.

### DR-4 — the fuzz corpus is not libFuzzer

The plan's acceptance for W1-2 is "`cargo fuzz` corpus runs clean for 10 minutes
per parser". **`cargo-fuzz` is not installed and cannot be fetched**: the
environment is `cargo --offline` with no network, and the crate is not in the
local registry.

Rather than claim a `cargo fuzz` run that did not happen, this step ships
`bioinfo-fastx/src/bin/fastx-fuzz.rs`: a deterministic, dependency-free harness
that does the part of libFuzzer's job that matters for this criterion — feed
adversarial input to every parser entry point and assert that nothing panics,
hangs, or produces inconsistent results.

| | libFuzzer (`cargo fuzz`) | this harness |
| --- | --- | --- |
| Mutation | coverage-guided | deterministic PRNG, 9 structured mutation kinds |
| Finds panics / OOB | yes | yes |
| Finds deep paths via feedback | yes | **no** |
| Reproducible from a seed | with `-runs=` / crash files | yes, `--seed N` |
| Runs offline with no new dependency | needs nightly + `libfuzzer` | yes |

The limitation is real and is not papered over: this harness will not explore the
same path set a coverage-guided fuzzer would. Adding `cargo fuzz` to CI remains
open work (see "What remains for W1-3").

**It was not a formality.** The harness found two genuine defects that the unit
tests and both oracle suites had missed — see DR-5.

### DR-5 — two real defects found by fuzzing, and fixed

Both are in the shared parser/writer, both are CRLF-related, and both would have
been inherited by every future `bioinfo-fastx` consumer.

**Defect 1: an embedded carriage return could not round-trip.** The parser strips
one trailing `\r` per line to tolerate CRLF. A line that ended in *two* carriage
returns therefore parsed successfully with a `\r` left inside the field:

```text
input:     @r1\r\nACGT\r\n+\r\r\nIIII\r\n
plus:      "+\r"
re-render: "+\r\n"   -> re-parses as "+"   (not equal)
```

A record that parses but cannot be written back unchanged is a trap for any
caller that rewrites what it read. Fixed by **rejecting an embedded `\r`** in the
header, sequence, separator and quality lines, with a clear message. The writer
also strips a trailing `\r` defensively so a hand-built record cannot emit an
ambiguous line.

**Defect 2: the same, via the header.** `@r1\r\r\n` left `\r` in the header.
Fixed by the same rule.

Neither input is accepted by cutadapt or UMI-tools (both are malformed FASTQ), so
no oracle case changed — which is exactly why the oracle could not have found it.
Regression tests were added at the parser level and the property is now asserted
continuously by the fuzz harness (`write ∘ parse = id` in lenient mode).

### DR-6 — a property the harness deliberately does *not* assert

The fuzzer initially asserted that parsing a stream chunk-by-chunk finds the same
number of records as one single pass. That is **false for malformed input**, and
correctly so: a chunk boundary can fall where the whole-input parser would have
kept reading, so a chunk may begin with a line the single-pass parse treated as a
continuation.

Rather than weaken the check to "chunked ≤ single-pass" and leave a vague
assertion, the property was replaced with the two that are actually true and
useful: the chunks must tile the input exactly, and every chunk but the last must
end on a record boundary. The pipeline's determinism guarantee holds for **valid**
input, which is what `tests/gate/` and the differential suites verify against the
real binaries.

## Verification results

All commands were run in this session from the repository root. `--offline`
throughout; nothing was downloaded.

### Build and tests

```text
$ cargo build --workspace --release --offline
    Finished `release` profile [optimized] target(s) in 33.44s
```

```text
$ cargo test --workspace --offline
407 total (39 bioinfo-common + 93 bioinfo-fastx + 209 rust-cutadapt + 66 rust-umitools
           + per-target integration and doc tests), 0 failed

$ cargo test --workspace --offline --release
407 total, 0 failed
```

Both profiles are green. The W1-1 acceptance command is the debug one; the
release run is included because it exercises what ships.

`rust-cutadapt`'s own count fell from 211 to 209 because its parser tests now live
in `bioinfo-fastx` (where they cover both crates); nothing was dropped.

### Oracles

```text
$ python3 rust-cutadapt/tests/oracle/differential.py \
      --rust target/release/rust-cutadapt --matrix
119/119 cases passed
```

```text
$ cd rust-umitools
$ CARGO_TARGET_DIR=<root>/target UMI_TOOLS_ORACLE_PATH=<oracle> bash tests/oracle/run_exact.sh
77/77 byte-identical, 0 differ
$ ... bash tests/oracle/run_oracle.sh
39 passed, 0 failed
$ ... bash tests/oracle/run_extract.sh
9 passed, 0 failed
$ ... bash tests/oracle/run_formats.sh
formats ok: 0 failures
```

Every number is unchanged from the W1-1 baseline
([EVIDENCE-W1-1.md](EVIDENCE-W1-1.md)).

### W1-1 byte-neutrality gate

```text
$ bash tests/gate/m3.sh
PASS  reference binaries present
PASS  reference binaries match the recorded SHA256
PASS  binaries under test present
PASS  goldens present (40 case files)
PASS  umitools oracle resolves
PASS  verify: every case matches the recorded behaviour
PASS  self-test: a mutated golden is rejected
PASS  provenance block is byte-exact
PASS  exit-code contract exercised for 0/1/2/3/4
9 criterion/criteria: 9 passed, 0 failed
M3 PASS
```

Those 40 goldens were recorded from the **pre-W1-1** binaries, so this is a
byte-level comparison spanning both refactors.

### W1-2 FASTX equivalence harness

`m3.sh` has two blind spots for W1-2: it does not vary the chunk size, and it
does not exercise every compression format. `tests/gate/w12_fastx_equivalence.py`
closes both by running the **same invocation against the pre-migration and
post-migration binaries** and comparing exit code, decompressed output bytes and
normalised stdout/stderr.

```text
$ python3 tests/gate/w12_fastx_equivalence.py \
      --reference .work/w1-2-baseline/bin/rust-cutadapt.pre-w12 \
      --candidate target/release/rust-cutadapt
35/35 cases identical

$ python3 tests/gate/w12_fastx_equivalence.py --binary umitools \
      --reference .work/w1-2-baseline/bin/rust-umitools.pre-w12 \
      --candidate target/release/rust-umitools
12/12 cases identical
```

**47 cases across both binaries, all identical.**

| Group | Cases | What it proves |
| --- | --- | --- |
| `in_*` | 4 | gzip/bzip2/xz/plain input detection unchanged |
| `out_*` | 4 | gzip/bzip2/xz/plain output bytes unchanged (decompressed) |
| `fast_*` | 3 | `-Z` fast levels unchanged |
| `chunk_*` | 5 | chunk sizes 4096 … 1000003 produce identical output |
| `threads_*` | 4 | `-j 1/2/4/8` produce identical output |
| `crlf`, `empty`, `no_trailing_newline`, `plus_line_with_name` | 4 | parser edge cases |
| `malformed_*` | 5 | identical diagnostics and exit codes |
| `fasta_out` | 1 | `--fasta` relabels `@` to `>` |
| `paired_*`, `paired_interleaved` | 4 | both mates through the shared chunker |
| `aux_outputs` | 1 | too-short, untrimmed and info outputs |

`rust-umitools` matrix (12 cases): `extract` plain/gzip/stdout/3'/quality-mask,
`whitelist` distance/density/fixed-cell-number, three malformed FASTQ shapes, and
empty input. The BAM paths are untouched by W1-2 and remain covered by
`run_exact.sh`/`run_oracle.sh` rather than duplicated here.

### Fuzz

```text
$ cargo run --release --offline -p bioinfo-fastx --bin fastx-fuzz -- --seconds 30 --seed 1
fastx-fuzz: seed=1 seconds=30 iterations_cap=0 seeds=16
................................................................
fastx-fuzz: 1291534 iterations, 0 failures, 30.0s elapsed
FUZZ PASS
```

The required 10-minute run:

```text
$ cargo run --release --offline -p bioinfo-fastx --bin fastx-fuzz -- --seconds 600 --seed 20261020
```

```text
fastx-fuzz: 31332015 iterations, 0 failures, 600.0s elapsed
FUZZ PASS
```

**31,332,015 iterations over 600 s with no failures.** That is the W1-2
acceptance criterion satisfied with the substitute harness DR-4 describes.

The harness was not a formality: it found two genuine CRLF round-trip defects in
the shared parser/writer that neither the unit tests nor either oracle suite could
reach, because both inputs are malformed FASTQ that cutadapt and UMI-tools reject
(DR-5).

## What is explicitly not claimed

* **No `cargo fuzz` run.** DR-4 records what was run instead and the exact
  limitation.
* **No BGZF *reader*.** BGZF is read as gzip, which is correct and is why there is
  no second code path. `.gzi` random-access indexes are not implemented.
* **No multi-line FASTA support in the borrowed parser.** A wrapped FASTA record
  is reported, not silently truncated.
* **No byte-identical *compressed* output as a general claim.** The workspace rule
  is decompressed bytes (plan decision D4). The one exception is BGZF, where the
  block structure is asserted and the payload is verified against three readers.
* **`rust-umitools`' gzip detection was not unified with the shared crate**
  (DR-1).
* **No clean-container run.** Evidence is from this machine with the pinned oracle
  environments.

## What remains for W1-3

1. **rust-fx not created (stats, seq, fq2fa, fx2tab, sample --seed, split, lint)**
2. cargo fuzz not in CI; the in-repo harness is a substitute, not an equivalent (DR-4)
3. multi-line FASTA cannot be represented by the borrowed Record; fq2fa will need an owning reader or segment iterator
4. rust-umitools gzip detection could be unified with content sniffing if a case is added to the oracle (DR-1)
5. .gzi BGZF index for random access, wanted by the BAM work in W1-4
6. no counting allocator to satisfy G-M4's per-record allocation criterion
7. the W1-2 equivalence harness covers cutadapt and umitools FASTQ commands; it does not cover the umitools BAM paths, which the existing oracle suites cover instead

## Reproducing this evidence

```bash
cd <repo root>
cargo build --workspace --release --offline
cargo test --workspace --offline
cargo test --workspace --offline --release

python3 rust-cutadapt/tests/oracle/differential.py \
    --rust target/release/rust-cutadapt --matrix

cd rust-umitools
CARGO_TARGET_DIR=../target UMI_TOOLS_ORACLE_PATH=<oracle> bash tests/oracle/run_exact.sh
CARGO_TARGET_DIR=../target UMI_TOOLS_ORACLE_PATH=<oracle> bash tests/oracle/run_oracle.sh
CARGO_TARGET_DIR=../target UMI_TOOLS_ORACLE_PATH=<oracle> bash tests/oracle/run_extract.sh
cd ..

bash tests/gate/m3.sh
python3 tests/gate/w12_fastx_equivalence.py \
    --reference .work/w1-2-baseline/bin/rust-cutadapt.pre-w12 \
    --candidate target/release/rust-cutadapt

cargo run --release --offline -p bioinfo-fastx --bin fastx-fuzz -- --seconds 600 --seed 20261020
```
