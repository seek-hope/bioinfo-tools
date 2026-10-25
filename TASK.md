# Trial task: rust-cutadapt v0.1.0

> Historical trial brief (October 2026). It describes the original v0.1.0
> single-end subset and is retained for provenance. Current behaviour is in
> [rust-cutadapt/README.md](rust-cutadapt/README.md) and
> [docs/FULL-COMPATIBILITY.md](rust-cutadapt/docs/FULL-COMPATIBILITY.md).
> The no-network, single-end, `clap`+`flate2`-only and 12-hour constraints in this
> file applied only to that trial; they are not current workspace rules.

Build a high-performance Rust implementation of a documented subset of **cutadapt 4.6** for single-end FASTQ trimming. The oracle is cutadapt 4.6; the visible golden outputs are in `rust-cutadapt/tests/fixtures/`.

## Required subset

- CLI: `rust-cutadapt -a ADAPTER -m MIN_LEN -o OUTPUT INPUT`
  - `-a/--adapter`: one 3' adapter sequence.
  - `-m/--minimum-length`: discard reads shorter than this after trimming.
  - `-o/--output`: output FASTQ or FASTQ.gz (format follows the extension).
  - `INPUT`: input FASTQ or FASTQ.gz; `-` for stdin is optional.
- Single-end only. Plain and gzip input/output must both work.
- Match cutadapt 4.6's trimming for this subset: exact and partial 3' adapter matches, one mismatch under the default error rate, internal adapter matches, no-adapter reads, and minimum-length filtering. The visible expected files define the required records exactly.
- Read name, sequence, and quality must be preserved except for the trimmed 3' portion. Output FASTQ must be decompressed-byte-identical to the expected records.
- A read that becomes shorter than `-m` is discarded. An empty input produces an empty output and exit 0.
- Unsupported options (paired-end, 5' adapters, indels, multiple adapters, quality trimming, wildcards) must be rejected before writing output with a clear usage error and exit 2. Do not silently implement a different algorithm.

## Deliverables

- A working crate with a release binary.
- Unit/integration tests for parsing, trimming, gzip, empty input, malformed FASTQ, and minimum length.
- `tests/check.py` passes for both the plain and gzip visible fixtures.
- A benchmark report on `bench/workload.fastq.gz` with wall time and peak RSS; compare with `bench/reference.json` (cutadapt 4.6 baseline). Target at least 5x cutadapt wall-time speedup; if the target is not reached, report the measured result honestly.
- A short `README.md` documenting the supported subset, unsupported behavior, CLI, and benchmark method.
- `rust-umitools/` must remain untouched: `git -C rust-umitools status --short` is empty.

## Constraints

- 12 hour wall clock. No token or model-call budget is imposed by the user; still stop at the deadline.
- No network from the shell; use `--offline`.
- The leader must run the final checks itself; a member's self-report is not acceptance.
