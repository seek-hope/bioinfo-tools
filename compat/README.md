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
  SeqKit replacement. `translate -l/-L` has 55/55 listing comparisons and
  `tab2fx -b` has line-boundary/partial-output comparisons. Their remaining
  malformed-input policy differences stay experimental. The shared
  `--compress-level` path has gzip/zstd/bzip2/xz differential coverage;
  compressed bytes are backend-dependent and compared after decompression.

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
