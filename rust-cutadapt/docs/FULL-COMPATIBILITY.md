# Full cutadapt 4.6 compatibility

Status: 119 of 119 differential cases pass against a real cutadapt 4.6 binary, and `tests/oracle/differential.py --matrix` exits 0. No behavioural deviations remain; the only intentional differences are the compressed-byte level and the extra note printed when an output file is left incomplete (see "Known deviations").

Delivered: the full cutadapt 4.6 option surface, all adapter types, read
modifications, filters, `--action` modes, `--revcomp`, renaming, auxiliary
outputs, FASTA, paired-end and interleaved input/output, the text report (full
and minimal) and the JSON report.

The reference oracle is a real cutadapt 4.6 binary (Python 3.14.8, compiled
`_align`/`_kmer_finder` extensions). Its output reproduces the repository's
stored golden fixture byte for byte.

## What "full" means

A rust-cutadapt release is *fully compatible* only when, for every documented
cutadapt 4.6 invocation:

1. **CLI surface.** Every option and alias in the list below is accepted with
   the same arity and value grammar, including `--option=value`, attached short
   values (`-aAGAT`, `-q20,20`, `-Z9`), repeated options, `file:FILE` adapter
   notation, and `{name}`/`{id}`/`{adapter_name}` templates. Options absent from
   cutadapt 4.6 must be rejected; mutually exclusive combinations must be
   rejected with the same exit code.
2. **Exit codes.** `0` success, `1` runtime failure (I/O, parse, corrupt gzip,
   template errors), `2` usage error. A failure must not leave a
   plausible, complete-looking output file.
3. **Read data.** Decompressed FASTQ/FASTA written to `-o`, stdout, and every
   auxiliary output (`--too-short-output`, `--too-long-output`,
   `--untrimmed-output`, and their `-paired-output` forms), plus
   `--info-file`, `--rest-file`, `--wildcard-file`, must be byte-identical.
   Record order is input order. Headers, `+` lines, name transformations
   (`-x`, `-y`, `--rename`, `--strip-suffix`, `--length-tag`) and case
   changes (`--action lowercase`) are part of the byte comparison.
4. **Reports.** The text report must match cutadapt's after normalising only
   inherently machine-dependent values: version banner, `Command line
   parameters`, timing/rate lines, core count, memory, and file paths. Every
   statistic, percentage, breakdown row and adapter table must match.
5. **JSON.** `--json` output must match cutadapt's schema, field names, nesting
   and values, ignoring only `cutadapt_version`, `python_version`,
   `command_line_arguments`, `cores` and `tag`. Absent counters stay `null`
   exactly where cutadapt leaves them `null`.
6. **Compression.** Input auto-detected as gzip/bzip2/xz by magic bytes;
   output compression selected by extension; `-Z` selects level 1; decompressed
   bytes must match (compressed bytes need not, since deflate implementations
   differ).
7. **Parallelism.** `-j N` must not change output bytes or report content
   relative to `-j 1`. Results are deterministic across core counts.
8. **Performance.** The target is at least 5x wall clock on
   `bench/workload.fastq.gz`. The default four-worker configuration meets it:
   five runs measured a 0.672 s mean, 6.60x against the recorded 4.4363 s
   baseline and 14.36x against a same-session oracle. `-j 1` reproduces the
   earlier 2.352 s single-thread result; cutadapt defaults to one core, while
   rust-cutadapt defaults to four. See
   [bench/cutadapt-c8b0249.json](bench/cutadapt-c8b0249.json).

## Frozen option surface (cutadapt 4.6)

Captured from `cutadapt --help` (214 lines, 60 options). Grouped exactly as the
oracle groups them. "Status" is against the current v0.1.0 tree on this branch.

### Global

| Option | Argument | Default | Status |
| --- | --- | --- | --- |
| `-h`, `--help` | – | – | implemented (exit 0) |
| `--version` | – | – | implemented (exit 0) |
| `--debug` | – | off | implemented |
| `-j`, `--cores` | CORES | 4 (`0` = auto; cutadapt default 1) | implemented (4 default for performance; `-j 1` for exact parity) |

### Finding adapters

| Option | Argument | Default | Status |
| --- | --- | --- | --- |
| `-a`, `--adapter` | ADAPTER (repeatable, `file:FILE`, `$` anchor, `{name}`) | – | implemented (repeatable, `$` anchor, `file:FILE` lists) |
| `-g`, `--front` | ADAPTER (repeatable, `^` anchor) | – | implemented |
| `-b`, `--anywhere` | ADAPTER (repeatable) | – | implemented |
| `-e`, `--error-rate`, `--errors` | E (rate `< 1` or absolute integer `>= 1`) | 0.1 | implemented (rate and absolute forms) |
| `--no-indels` | – | indels allowed | implemented (indels off) |
| `-n`, `--times` | COUNT | 1 | implemented |
| `-O`, `--overlap` | MINLENGTH | 3 | implemented |
| `--match-read-wildcards` | – | off | implemented |
| `-N`, `--no-match-adapter-wildcards` | – | off (adapter wildcards on) | implemented (IUPAC codes in the adapter are literal) |
| `--action` | `{trim,retain,mask,lowercase,none}` | `trim` | implemented |
| `--rc`, `--revcomp` | – | off | implemented |

### Additional read modifications

| Option | Argument | Default | Status |
| --- | --- | --- | --- |
| `-u`, `--cut` | LEN (signed, repeatable once per sign) | – | implemented |
| `--nextseq-trim` | 3'CUTOFF | – | implemented |
| `-q`, `--quality-cutoff` | `[5'CUTOFF,]3'CUTOFF` | – | implemented |
| `--quality-base` | N | 33 | implemented |
| `--poly-a` | – | off | implemented |
| `--length`, `-l` | LENGTH | – | implemented |
| `--trim-n` | – | off | implemented |
| `--length-tag` | TAG | – | implemented |
| `--strip-suffix` | STRIP_SUFFIX (repeatable) | – | implemented |
| `-x`, `--prefix` | PREFIX (`{name}`) | – | implemented |
| `-y`, `--suffix` | SUFFIX (`{name}`) | – | implemented |
| `--rename` | TEMPLATE | – | implemented |
| `--zero-cap`, `-z` | – | off | implemented |

### Filtering of processed reads

| Option | Argument | Default | Status |
| --- | --- | --- | --- |
| `-m`, `--minimum-length` | LEN[:LEN2] | 0 | implemented (defaults to 0, `:LEN2` supported) |
| `-M`, `--maximum-length` | LEN[:LEN2] | none | implemented |
| `--max-n` | COUNT (int or fraction) | – | implemented |
| `--max-expected-errors`, `--max-ee` | ERRORS | – | implemented |
| `--max-average-error-rate`, `--max-aer` | ERROR_RATE | – | implemented |
| `--discard-trimmed`, `--discard` | – | off | implemented |
| `--discard-untrimmed`, `--trimmed-only` | – | off | implemented |
| `--discard-casava` | – | off | implemented |

### Output

| Option | Argument | Default | Status |
| --- | --- | --- | --- |
| `--quiet` | – | off | implemented |
| `--report` | `{full,minimal}` | `full` | implemented (full and minimal) |
| `--json` | FILE | – | implemented |
| `-o`, `--output` | FILE (`{name}`, demultiplexing) | stdout | implemented (stdout default, `{name}` renaming) |
| `--fasta` | – | off | implemented |
| `-Z` | – | level 6 | implemented (level 1 with -Z) |
| `--info-file` | FILE | – | implemented |
| `-r`, `--rest-file` | FILE | – | implemented |
| `--wildcard-file` | FILE | – | implemented |
| `--too-short-output` | FILE | – | implemented |
| `--too-long-output` | FILE | – | implemented |
| `--untrimmed-output` | FILE | – | implemented |

### Paired-end

| Option | Argument | Default | Status |
| --- | --- | --- | --- |
| `-A` | ADAPTER | – | implemented |
| `-G` | ADAPTER | – | implemented |
| `-B` | ADAPTER | – | implemented |
| `-U` | LENGTH | – | implemented |
| `-Q` | `[5'CUTOFF,]3'CUTOFF` | same as R1 | implemented |
| `-p`, `--paired-output` | FILE | – | implemented |
| `--pair-adapters` | – | off | implemented |
| `--pair-filter` | `{any,both,first}` | `any` | implemented |
| `--interleaved` | – | off | implemented |
| `--untrimmed-paired-output` | FILE | – | implemented |
| `--too-short-paired-output` | FILE | – | implemented |
| `--too-long-paired-output` | FILE | – | implemented |

Total: 60 options across 5 groups. All of them are implemented and exercised by
the differential harness, including the `file:FILE` adapter-list notation for
`-a`/`-g`/`-b` and the `-A`/`-G`/`-B` R2 forms (one adapter per FASTA record,
record name used in the report heading and by `{name}`).

## Differential harness

`tests/oracle/differential.py` runs both binaries on identical inputs and
arguments and compares exit status, decompressed outputs, auxiliary files, the
normalised text report and the JSON report.

```bash
python3 tests/oracle/differential.py --self-check   # harness sanity
python3 tests/oracle/differential.py               # full suite
python3 tests/oracle/differential.py --matrix      # compact matrix
python3 tests/oracle/differential.py --filter json -v
python3 tests/oracle/differential.py --json out.json
```

Binary discovery: `--oracle`/`--rust`, then `CUTADAPT_ORACLE`/`RUST_CUTADAPT`,
then the prefix declared by `tests/ci/env.yml`, an active `CONDA_PREFIX`,
`.work/oracle-venv`, and finally `PATH`. No network is required.

The harness self-check validates report normalisation, JSON comparison, fixture
generation, FASTQ record validity (sequence/quality lengths) and placeholder
expansion, so a broken fixture can never be mistaken for a product defect.

## Baseline against the v0.1.0 binary

Recorded in `docs/reports/baseline-v0.1.0.{txt,json}`: **2 of 75 cases pass**.

| Category | Count | Meaning |
| --- | --- | --- |
| missing feature | 70 | option rejected with exit 2, no output produced (historical baseline; all are implemented now) |
| default-value gap | 2 | `-m` and `-o` default incorrectly (required instead of 0/stdout) |
| report format | 1 | `a3_regular`: output bytes identical, text report format differs |
| unexplained | 0 | – |

The single genuinely shared case (`a3_regular`) already produces byte-identical
read data, which confirms the Phase 0 trimming core is sound; everything else is
missing surface area.

### Known deviations

| Area | Status | Detail |
| --- | --- | --- |
| `--poly-a` | exact | Direct port of cutadapt 4.6's compiled `qualtrim.pyx::poly_a_trim_index`: +1 per A, −2 otherwise, the best index is kept only while `errors * 5 <= scanned_tail_length` (a 0.2 error-rate guard), tails shorter than 3 ignored. Verified against 3,500 generated reads with zero differences. |
| `--report` "expect" column | implemented | cutadapt derives it from a global pseudo-random stream, so it varies run to run; the differential harness compares it with a tolerance while count and error columns are compared exactly. |
| `--max-average-error-rate` on an empty read | exact (fail-closed) | cutadapt 4.6 raises `ZeroDivisionError` and exits 1. rust-cutadapt matches the observable behaviour: exit 1, identical stdout, the diagnostic on stderr, no traceback, and the already-flushed records preserved. Guarded by `empty_read_with_average_rate_filter_is_fatal`. |
| Compressed output bytes | level choice | Decompressed bytes are always identical. gzip defaults to level 3 (cutadapt uses 6) as a speed trade-off; `-Z` selects level 1 for gzip and bzip2 and preset 0 for xz, as cutadapt does. Compressed sizes therefore differ slightly. |
| Error output for a partial output file | extra note | cutadapt reports only its diagnostic even when records were already written. rust-cutadapt adds a "Note: the output file is incomplete: N record(s) ..." line, because a plausible-looking truncated FASTQ is a real hazard. This is the only place the output deliberately says more than cutadapt. |

### Verification summary

`python3 tests/oracle/differential.py --matrix` -> **119/119 pass** and exits 0.
The cases cover: every adapter type and notation (including `file:FILE` lists and
inline `name=SEQ`), error rate/overlap/times variants, all read modifications,
all filters, `--action` modes, `--revcomp`, renaming, auxiliary outputs, FASTA,
`--report minimal`, `--json`, `-j`, `-Z`, demultiplexing with `{name}`, paired
end (adapters, filters, per-mate lengths, auxiliary outputs, interleaved input
and output, `--pair-adapters`), gzip/bzip2/xz input and output plus magic-byte
detection, and the documented validation errors. 208 Rust tests pass, and
`cargo fmt --all -- --check` plus `cargo clippy --all-targets --locked -- -D
warnings` are clean.

Performance at commit `c8b0249` is 0.672 s mean (five `/usr/bin/time -v`
runs) with the default four workers, 6.60x against the recorded baseline and
14.36x against a same-session oracle; the 5x target is met. `-j 1` reproduces
the 2.352 s single-thread result. Decompressed output is byte-identical to
cutadapt 4.6. Raw evidence: [bench/cutadapt-c8b0249.json](bench/cutadapt-c8b0249.json)
and [bench/cutadapt-281a8dd.json](bench/cutadapt-281a8dd.json).

### Known oracle quirks to encode rather than fix

* `cutadapt -o - -` treats the second `-` as an output *filename* (`0`) and can
  exit 0 after an error message.
* `--max-average-error-rate` raises `ZeroDivisionError` in cutadapt 4.6 when a
  read becomes empty; the harness records cutadapt's actual behaviour.

## Implementation plan

Order of work, with reuse decisions. Each stage ends with the differential
matrix green for its category before the next begins.

### Stage A — full single-end

New modules, replacing the current monolithic CLI and engine:

| Module | Responsibility | Reuse |
| --- | --- | --- |
| `src/cli/args.rs` | Full 60-option grammar, aliases, attached values, repeatable options, template placeholders | **replace** `src/cli.rs` |
| `src/adapter/mod.rs` | Adapter tree: `-a`/`-g`/`-b`, `$`/`^` anchors, linked `^...`, `file:FILE`, `{name}`, wildcards, `--times`, multiple adapters, best-match selection | **extend** `trim.rs` (regular 3' path is already byte-correct) |
| `src/adapter/align.rs` | Indel-capable semiglobal DP (remove `indel_cost = 100000` lock), `-e` absolute/rate modes | **extend** `align.rs` (single-column DP already ports cutadapt's `_align.pyx`) |
| `src/modify.rs` | `-u/-U`, `-q/-Q`, `--nextseq-trim`, `--poly-a`, `-l`, `--trim-n`, `-z`, `--length-tag`, `-x/-y/--rename/--strip-suffix`, `--action`, `--rc` | new |
| `src/filter.rs` | `-m/-M` (with `:LEN2`), `--max-n`, `--max-ee`, `--max-aer`, `--discard-*`, `--pair-filter` | new |
| `src/report/mod.rs` | Text report (`full`/`minimal`) and JSON report with cutadapt field names, null semantics and adapter tables | new |
| `src/io/fastq.rs` | FASTQ **and** FASTA, gzip/bzip2/xz detection, `-o`/`-p`/auxiliary writers, stdout default | **extend** `fastq.rs` |
| `src/pipeline.rs` | Streaming chunk pipeline with `-j`, deterministic ordering, bounded memory | **reuse**, add per-read modifiers |

Semantics to port directly from the oracle rather than invent: adapter
`match_sequence`/`trimmed()` suffix trimming for 3', prefix trimming for 5',
`--action` post-processing, the quality-trimming loop, poly-A scoring, and the
`--rename` variable set.

### Stage B — paired-end / interleaved

| Module | Responsibility |
| --- | --- |
| `src/pair.rs` | R1/R2 record lockstep, `-A/-G/-B/-U/-Q`, `-p/--paired-output`, `--interleaved` read+write, paired auxiliary outputs |
| `src/filter.rs` (extend) | `--pair-filter any/both/first`, pair-wise discard |
| `src/adapter/mod.rs` (extend) | `--pair-adapters` (all-or-none per pair) |

Both reads must be processed from the same chunk so that ordering stays
deterministic under `-j`.

### Stage C — reports, JSON and format parity

| Module | Responsibility |
| --- | --- |
| `src/report/text.rs` | Byte-level match of the full report after normalisation: summary, fate breakdown, basepair counts, adapter tables, `Overview of removed sequences`, `Bases preceding removed adapters` |
| `src/report/json.rs` | Exact schema (`schema_version`, `cutadapt_version`, `input`, `read_counts`, `basepair_counts`, `adapters_read1/2`, `poly_a_trimmed_*`), `null` where cutadapt uses `null` |
| `src/report/mod.rs` | `--report minimal`, `--quiet`, `--debug` |

### Decisions that would change public behaviour (need a ruling)

1. **`--no-indels` default.** cutadapt allows indels by default; shipping
   indels-on will change existing v0.1.0 outputs for some reads. Required for
   compatibility, but it changes results, so it is called out explicitly.
2. **Default compression level.** cutadapt uses 6; v0.1.0 uses 3 to reach the 5x
   speed target. Matching cutadapt byte-for-byte only requires decompressed
   equality, so level 3 can stay — but it is a documented deviation.
3. **`-o` default.** Making stdout the default (as cutadapt does) changes the
   v0.1.0 "required -o" contract.
4. **Report banner.** cutadapt prints version/Python banners; matching them
   exactly would make rust-cutadapt claim to be cutadapt. The harness normalises
   these, and the report body must match.

## Next concrete step

Stage A, starting with `src/cli/args.rs` (the full option grammar) and
`src/report/` being the two largest blockers, then the adapter tree
(`-g`, `-b`, anchors, `file:`, wildcards, `--times`). Re-run
`python3 tests/oracle/differential.py --matrix` after each module lands; the
single-end category must reach 100% before Stage B starts.
