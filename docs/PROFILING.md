# T11 profiling and profile-driven optimization

Historical snapshot: targeted profiling on the recorded `49953ec` dirty
worktree and host. The measurements below are not results for the current build.

The later [optimization after e125519](../bench/OPTIMIZATION.md) covers stats,
conversion, lint, flagstat, sort and QC with matched native before/after results,
output-preservation checks and current source/binary provenance. It includes
aggregate instruction and syscall counts; the original T11 evidence below stays
historical.

Status: **completed for the targeted profiling pass**. This is not a universal
performance claim; the numbers belong to the host, binary and synthetic input
recorded here.

## Environment and input

- Host: 20 logical / 14 physical CPUs, Linux x86_64, shared development machine.
- Rust: 1.99.1; workspace commit `49953ec` plus the recorded dirty worktree.
- Input: `bench/run.py --sizes 1000` (1,000,000 FASTQ reads / 1,000,000 SAM
  records), generated deterministically by the benchmark runner.
- Counters: `perf stat -e task-clock:u,cycles:u,instructions:u,branches:u,branch-misses:u,cache-misses:u -r 2`.
  `perf record` is blocked by `perf_event_paranoid=2`; callgrind was also tried but
  the release binary's symbols were insufficient for a useful function-level
  report. No kernel-stack or hardware-precision claim is made.

## perf stat (1000x, representative run)

| Case | task-clock | wall | cpu_core cycles | cpu_core instructions |
| --- | ---: | ---: | ---: | ---: |
| `cutadapt` (`-j 4`) | ~0.56 s | 0.559 s | 17.30 B | 53.29 B |
| `fastqc` | 4.349 s | 4.361 s | 19.55 B | 64.45 B |
| `fx stats -a -T` | 1.405 s | 1.407 s | 6.41 B | 20.07 B |
| `bam sort --max-memory 1MiB --max-open-files 8` (before) | 7.585 s | 7.604 s | 32.16 B | 94.53 B |
| `bam sort` same (after) | 5.252 s | 5.269 s | 20.75 B | 64.62 B |
| `umi dedup` (16 threads) | 2.194 s | 1.030 s | 6.71 B | 13.36 B |

The high `bam_sort` low-memory cost is dominated by writing and re-reading many
temporary BGZF runs. The temporary runs used the default BGZF compression level
(6); the final output does not need that level.

## Change

`rust-bam/src/io_util.rs` adds `BamWriter::new_fast`, which builds a BGZF writer
with `CompressionLevel::FAST` (level 1). Only `spill_run` and `merge_to_run` use
it. The final output still uses `BamWriter::new` (default level 6).

## Before/after

| Metric | Before | After | Change |
| --- | ---: | ---: | ---: |
| wall (1000x, low-memory sort) | 7.604 s | 5.269 s | **-30.7%** |
| task-clock | 7.585 s | 5.252 s | -30.8% |
| cpu_core cycles | 32.16 B | 20.75 B | -35.5% |
| cpu_core instructions | 94.53 B | 64.62 B | -31.6% |
| decompressed record stream | baseline | identical | `cmp` of `rust-bam view` output |
| final output compression | default | default | unchanged |

The trade-off is larger temporary BAM files; final output bytes and decompressed
records are unchanged. `cargo test --release -p rust-bam` (including the
external-sort byte-equality, malformed-input and temp-cleanup tests) passes.

## What was not changed

- No change to cutadapt, FASTX, QC or UMI hot paths: their profiles did not show a
  single low-risk optimization with a clear end-to-end win under the same
  correctness contract.
- No kernel profiling, SIMD rewrite, GPU path or threaded BAM sort was attempted.
- The benchmark runner remains the source of reproducible numbers; this document
  does not replace `bench/results/current.json`.

## Remaining performance work

T10 (BAM region/filter/CSI/thread behavior and QC adapter/contaminant detection)
is now implemented; any future optimization must use the same-input correctness
and benchmark evidence.