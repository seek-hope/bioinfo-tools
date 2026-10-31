# Compatibility matrix

This directory tracks five CLIs against pinned upstream versions. A command is
`verified` only for the inputs and comparison rules named in its evidence;
command-level verification does not imply that every upstream option is present.

## Targets

| CLI | Upstream | Tracked surface |
| --- | --- | --- |
| `rust-cutadapt` | cutadapt 4.6 | Help options, reports, compression and paired/interleaved processing |
| `rust-umitools` | UMI-tools 1.1.6 | Six commands and the component feature matrix |
| `rust-bam` | samtools 1.24 | Every top-level command plus selected feature rows |
| `rust-fastqc` | FastQC 0.13.0 | CLI options, report artifacts and modules |
| `rust-fx` | SeqKit 2.14.0 / fq 0.12.0 | Every SeqKit command and `fq lint` |

Pinned help/version snapshots are in [`upstream/`](upstream/); snapshots of the
current CLIs are in [`current/`](current/). `matrix.json` records the upstream
snapshot hashes.

## Status definitions

| Status | Meaning |
| --- | --- |
| `verified` | Executable upstream comparison or an explicit fixture supports the stated behavior. |
| `experimental` | Some behavior is implemented, but coverage or a documented difference remains. |
| `unsupported` | The capability is absent or rejected before output. |
| `exception` | An explicitly approved behavior difference, with a registered rule and executable proof; it is not an upstream-equivalence pass. |

Run `python3 tests/ci/check_compat.py` for the current row counts. Regenerate the
matrix after a behavior change; do not mark a row verified without new evidence.
Matrix schema 3 adds the `unachievable` text column; schema 2 introduced `exception`.
The column contains a reason only where an approved constraint prevents an
upstream-equivalent capability. Empty means no such determination, including
unfinished work. Currently only `-j/--java` and `--debug` have values, referring
to the approved pure-Rust runtime. They remain exception rows; this column is
not an additional status or a way to remove unsupported/experimental work.
The finite `approved-exceptions.json` registry
requires explicit approval, a behavior rule, proof and a verifier for each such
row. Native-only JVM exclusions and proven upstream defects are recorded separately
from implemented equivalence and unfinished functionality. No missing scientific
command can become an exception merely to reduce the unsupported count.

## Coverage notes

- **cutadapt**: all 66 rows are verified by the 119-case cutadapt 4.6 differential baseline.
- **UMI-tools**: the component matrix supplies 56 verified and two experimental rows.
  `dedup --gene-transcript-map` representative ties depend on upstream Python `set`
  iteration order and are accepted as a limitation; `--representative=stable` is a
  project extension. `--output-stats` is verified for unique/directional/cluster/
  adjacency/percentile, including multi-bundle inputs and NumPy's `RandomState`
  weighted-choice null model. The unique method's duplicate-dict-key overwrite is preserved.
- **samtools**: nine commands are verified. `flags` has 29 differential cases;
  `head -h/-n` has 36 exact SAM/BAM file/stdin cases. `depth` has 32 cases for
  `-a/-aa/-s` and 35 cases for flag filters. The coverage basic surface is
  verified for merged inputs, `--min-depth`, flags and histogram/depth output
  with 57 cases plus 35 flag cases. The full `coverage` command remains
  experimental because CRAM is unsupported and `-r` is sequential.
- **FastQC**: the 22-fixture `fastqc_data.txt` baseline excludes Adapter Content.
  The dedicated adapter suite now compares 35 cases exactly and independently
  proves six counting corrections and three empty-config rejections (44 cases).
  `-a/--adapters` is implemented natively; the adapter option/module use an
  explicit correctness exception. See [adapter evidence](../docs/EVIDENCE-FASTQC-ADAPTERS.md).
  The built-in and custom contaminant paths have 22/22 baseline and 7/7 custom
  source comparisons. `--extract/--delete` have 4/4 artifact-layout comparisons.
  `--nogroup`, `--min_length`, `--max_length` and `--dup_length` have 5/5
  option-group comparisons. HTML and stored ZIP artifacts remain experimental
  because plot images and FastQC auxiliary assets are not emitted. Native tile
  statistics now have 53 cases (48 exact, four independently proven missing-data
  corrections, one invalid Phred64 rejection). Missing observations remain `NaN`
  and are excluded from the reference mean; observed Q0 is retained. See
  [tile evidence and grouping fixes](../docs/EVIDENCE-FASTQC-TILES.md).
  Native Kmer and `--limits` now have independent probability/counting proofs,
  full module switches and thresholds; see [configuration and Kmer evidence](../docs/EVIDENCE-FASTQC-KMER-LIMITS.md).
  Corrected counts, inclusive tails, full opportunity denominators, verdict selection,
  and early rejection of partial configuration remain explicit exceptions.
  The JVM launcher options `-j/--java` and `--debug` are
  explicit native-runtime exceptions: they fail before input/output access with
  exit 2, while ordinary QC requires no JVM or upstream executable.
- **SeqKit/fq**: `common`, `head`, `grep`, `sort`, `fa2fq`, `faidx`, `rmdup`,
  `restart`, `range`, `sliding`, `tab2fx`, `rename`, `replace`, `subseq` and
  `translate` have direct per-command differential evidence. The existing 50/50
  FASTX suite also covers `stats`, `seq`, `fq2fa`, `fx2tab`, `sample`, `split`
  and `fq lint`; those are selected-surface verified, not claims of complete
  SeqKit replacement. `translate` has 710/756 exact direct comparisons and 46
  reviewed, hash-checked differences after correcting proven upstream defects.
  Together with `range`, it has 434 independent correctness checks (see below).
  `tab2fx` has 99/99 direct
  comparisons for conversion, byte fields, CSV comment prefixes, scanner limits,
  compressed input/output and fatal plain-output commit boundaries. `range`
  has 203/206 comparisons: interval selection and stdin streaming align, but
  three mixed-format cases remain different (see below). The shared
  `--compress-level` path has gzip/zstd/bzip2/xz differential coverage;
  compressed bytes are backend-dependent and compared after decompression.

## SeqKit streaming evidence and remaining boundaries

```bash
python3 tests/oracle/seqkit_streaming_diff.py --command tab2fx
python3 tests/oracle/seqkit_streaming_diff.py --command range
cargo test -p rust-fx --test range --test tab2fx --test seqkit_streaming --locked
```

The new differential creates inputs only in a temporary directory. It compares
exit codes and exact stdout/plain output files, stripping only ANSI color and
`ERROR:`/`[ERRO]` diagnostic prefixes. gzip/bzip2/xz success output is compared
after decoding; Rust process tests also round-trip zstd. Compressed bytes and
the physical prefix of an aborted compressed stream depend on the backend.
Invalid CSV flag syntax uses the Rust CLI diagnostic format; NaN, infinity and
overflowing buffer sizes are rejected rather than attempting an allocation.
These are not claims about all SeqKit global options or platform I/O diagnostics.

`tab2fx` now reads raw byte lines, including compressed input. As upstream does,
it accepts unequal sequence/quality lengths, treats more than three columns as
FASTA, and remembers FASTQ mode within each file. It is a converter, not a FASTQ
validator. The scanner needs O(longest line) memory (bounded by `-b`), with
wrapping/header scratch space of the same order, codec buffers and a 64 KiB
output buffer. A fatal input error discards that pending output but retains
already committed blocks. The previous whole-file pre-scan matched small errors
only and has been removed. Tests verify progress before stdin reaches EOF.

`range START:-1` is single-pass for files and stdin; other mixed-sign ranges are
rejected like SeqKit. Positive ranges retain O(longest record) bytes, while
negative ranges retain at most `abs(START)` records plus the current record and
formatting/codec buffers. Short-input negative ranges reproduce the upstream
circular-tail selection, not a clamped mathematical interval. Wrapped FASTQ,
explicit FASTQ output width and discarded `+` descriptions are also covered.

The `range` differential intentionally still exits nonzero: three cases append
FASTA after FASTQ. SeqKit 2.14 can force the FASTA records to FASTQ and reuse the
previous file's quality bytes from a pooled reader; Rust keeps each file's own
format and never invents qualities. This is an approved, independently proven
exception, not an exact-equivalence pass. The complete source analysis, malformed
output and independent fq S005 rejection are in
[the defect evidence](../docs/EVIDENCE-UPSTREAM-DEFECTS.md). The 96 provenance and
96 composition checks derive expected records from typed inputs. No upstream
pool-reuse behavior is simulated.

## SeqKit translate evidence

```bash
python3 tests/oracle/check_seqkit_exceptions.py --rust target/release/rust-fx
python3 tests/oracle/seqkit_translate_diff.py --rust target/release/rust-fx
cargo test -p rust-fx --test translate --test translate_errors --locked
```

The raw differential still reports 710/756 exact cases and exits 1. Its 46
mismatches are **not** suppressed or counted as byte-identical results. The separate
exception gate first proves corrected semantics, then requires the exact reviewed
case set and both binaries' observation hashes from
`seqkit-translate-approved-differences.json`. Missing/new mismatches or changed
output hashes fail. The gate also runs the original range/tab2fx differential.

The 434 independent checks cover typed-record/quality provenance, IUPAC/RNA strand
symmetry, ORF source-coordinate projection, right-end-only trimming and lossless
descriptions. The strand checks include all 3375 IUPAC codons for every one of 24
tables, three negative frames and both initiation modes. Six minimal reproductions,
source references, a third-party FASTQ validator and negative controls are in
[the defect evidence](../docs/EVIDENCE-UPSTREAM-DEFECTS.md).

Translation remains single-pass with O(longest record) working memory, codec
buffers and a 64 KiB commit buffer. Fatal errors exit 255, discard pending bytes
and retain committed plain output. `-e` emits a blank record for each failed frame;
all skipped translations still succeed. Initial alphabet detection is not bypassed
by `-e` or `-x`. The complete record is validated, including quality continuation
lines and codons following an internal X/STOP. There is no whole-file pre-scan.

This surface does not claim support for every SeqKit global option or identical
clap/pflag syntax diagnostics. Missing-file and same-file checks happen before
output creation to preserve input data. Rust-specific flag type errors and platform
I/O diagnostics retain the CLI's existing conventions. Compressed success output
compares after decoding; aborted compressed physical bytes are backend-dependent.

## Regeneration

```bash
python3 compat/build_matrix.py
python3 tests/ci/check_compat.py --write
python3 tests/ci/check_compat.py
```

The check runs as part of `tests/ci/check.sh`. It validates TSV/JSON
synchronization, snapshot hashes, pinned versions, unique surfaces and coverage
of upstream commands/options. It also rejects a current CLI command that is still
marked `unsupported`.
