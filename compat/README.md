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

Run `python3 tests/ci/check_compat.py` for the current row counts. Regenerate the
matrix after a behavior change; do not mark a row verified without new evidence.

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
- **FastQC**: `fastqc_data.txt` and Adapter Content have direct oracle evidence.
  The built-in and custom contaminant paths have 22/22 baseline and 7/7 custom
  source comparisons. `--extract/--delete` have 4/4 artifact-layout comparisons.
  `--nogroup`, `--min_length`, `--max_length` and `--dup_length` have 5/5
  option-group comparisons. HTML and stored ZIP artifacts remain experimental
  because plot images and FastQC auxiliary assets are not emitted; Kmer/tile
  modules remain open.
- **SeqKit/fq**: `common`, `head`, `grep`, `sort`, `fa2fq`, `faidx`, `rmdup`,
  `restart`, `range`, `sliding`, `tab2fx`, `rename`, `replace`, `subseq` and
  `translate` have direct per-command differential evidence. The existing 50/50
  FASTX suite also covers `stats`, `seq`, `fq2fa`, `fx2tab`, `sample`, `split`
  and `fq lint`; those are selected-surface verified, not claims of complete
  SeqKit replacement. `translate -l/-L` has 55/55 listing comparisons; its
  invalid-base output policy remains experimental. `tab2fx` has 99/99 direct
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
format and never invents qualities. This is recorded as a migration difference,
not normalized away or counted as a pass. Source inspection of the pinned
`bio v0.15.2` reader shows that its reset/FASTA assignment does not clear the
pooled record's quality field. No upstream pool-reuse behavior is simulated.

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
