# Optimization experiment after e125519

This follow-up compares the saved pre-change native executables, the optimized
executables, and the same pinned upstream tools on the same local inputs. It
answers a before/after question that the older, unmatched benchmark generations
cannot answer. It does not require every native command to beat upstream.

## Changes and validation requirements

- `rust-fx stats`: cache the 256 possible error probabilities, retaining the
  original per-base and per-read addition order. Exhaustively check quality bytes
  and offsets against the previous formula at the floating-point bit level.
  Replace per-base gap-slice searches and conditional counters with exact byte
  membership and Boolean counters; test all 256 bytes, repeated gap characters
  and gap/base overlaps.
- `rust-fx stats`, `fq2fa`, and `lint`: reuse four-line FASTQ record storage.
  Preserve format detection, malformed-input diagnostics, empty records,
  CRLF, compression failures, output commit behavior and per-file state. Owned
  FASTA/wrapped assembly must release the preceding record before reading the
  next one; otherwise the reuse interface can double live record storage.
- `rust-fx lint`: remove an unread header set; retain all existing validator
  decisions and paired-command name state. Single-file valid-input memory is
  proportional to the longest record plus the read buffer. Paired processing
  still retains read-one names; log-mode failures can also retain the first
  diagnostic. This is not a claim of constant memory for every lint mode.
- `rust-bam sort`: buffer eight-byte sort-index sidecars, retaining order,
  temporary/final compression levels, flush error propagation and diagnostics.
- `rust-bam flagstat`: resolve reference names once per header; keep the full
  alignment decoder, malformed-record checks, and lossy UTF-8 interpretation.
- `rust-fastqc`: cache the existing GC model for just the most recent nonempty
  read length. Keep additions in read order. The cache adds O(L) state, not one
  model per distinct length. Position and distinct-sequence state remain.
  Existing GC/duplication disagreements and missing graphics remain unresolved.

No dependencies or threading settings change. Runtime implementations stay native.
The existing comparison scripts and expected outputs remain unchanged.

The initial syscall trace of a 100k BAM sort with a 32 MiB chunk target recorded
103,216 `write` calls. `perf stat` is available, but `perf record` is blocked by
host policy; no function-level sampling profile is claimed. Instrumented syscall
runs are separate from performance timing.

## Fixed comparison population

`optimization_benchmark.py` selects all of the following 13 conditions:

| Input | Commands |
| --- | --- |
| SRR2584863 real 1M FASTQ, 150 bp | stats, fq2fa, lint, FastQC |
| Synthetic 1M FASTQ, 150 bp | stats, fq2fa, lint, FastQC |
| Synthetic 1M BAM | flagstat, sort |
| Synthetic 10M FASTQ, 150 bp | fq2fa, lint |
| Synthetic 10M BAM | flagstat |

Use the original SHA-256-verified manifests from the README experiment. The same
input data, pinned upstream versions, single logical P-core CPU 4, compression,
32 MiB sort chunks, tmpfs storage and command factories apply. Outputs and
verification remain outside the input files. No build, oracle suite, generation,
or other benchmark runs concurrently with timing.

Each condition has one warm-up and five measured blocks. Every block runs all
three implementations in fresh processes. The six permutations of
before/after/upstream are shuffled with seed 20261101 and used once each across
the warm-up and five measured blocks. Thus each implementation occupies each
measured position either once or twice. Input files are reread and followed by a
two-second idle interval before each command. Temperature, load, frequency, GNU
time CPU/RSS and sampled temporary storage are retained. Shared cores, thermal
variation, page-cache effects and the absence of durability fsync remain limits.

A six-condition 100k synthetic pilot uses one warm-up and one measured block;
its observations are excluded from formal tables. The pilot used the first
optimized artifact. An additional fixed-order 100k exploratory check motivated
the final branchless base counters; those timings are also excluded. The final
artifact is separately hashed and checked against the original behavior before
formal timing. An initial formal candidate run was interrupted after identifying
the FASTA record-lifetime overlap; retain that incomplete report, its reason and
its hashes. Restart all 13 conditions with the corrected final artifact. A
separate two-record, 32 MiB-per-record FASTA check compares original, superseded
and final output bytes and peak RSS; its single observations are memory regression
checks, not speed measurements. The final formal population has
195 measured processes and 39 warm-up processes. Five executions of a dataset
are repeated timings, not biological replicates; min–max ranges are observations,
not confidence intervals. Compute native before/after ratios within each block,
then take the median. Do not divide separately computed medians and label that
a paired ratio. Do not average speedups across commands.

## Space failure and recovery amendment

The corrected-artifact run completed all ten 1M conditions, then exhausted tmpfs
during the first 10M conversion warm-up. Retained outputs from the earlier
interrupted candidate occupied 3.2 GiB, and the new three-arm outputs required
another 4.8 GiB. Output/report writes failed; the parent's status file is empty
because that write also failed. Preserve the raw report, incomplete staged write,
logs and output hashes. Transfer the retained outputs to disk, verify their
SHA-256 hashes, and only then remove their temporary originals.

Keep the ten fully validated core conditions and rerun all three 10M conditions
with the same frozen binaries, inputs, CPU, command factories and comparisons.
Each condition uses intact three-arm blocks from one run. `--streaming-only`
retains the block orders from the full population. A free-space check includes
three complete decoded FASTA outputs and headroom, or one malformed-tail copy
for lint, for these verified uncompressed inputs.

`merge_optimization.py` accepts only the original full plan, ten complete core
conditions, one interrupted warm-up with no measured samples/checks, and all
three complete streaming conditions. It checks binary/source/input identities,
parameters and the hashes of shared command/timing/comparison implementations.
It retains both source-report hashes and execution metadata. All 13 conditions
remain required; the raw incomplete report is still rejected by the summary
program. Eight evidence controls include changed-artifact and discarded-sample
negative cases. This amendment changes storage/retry handling, not native code
or output acceptance.

## Checks and retained artifacts

Every arm must exit zero. Both native arms are checked against upstream using the
unchanged README comparison functions. BAM checks require quickcheck, sorted
coordinates, count and normalized records; the native before/after BAM bytes
must also match. Lint includes an invalid-last-record rejection in each native
arm and upstream, and the native log messages must match after removing only
wall-clock timestamps. Native FastQC must preserve every report, summary, HTML
and ZIP byte. Its upstream comparison remains limited to independently checked
counts and lengths plus visible raw module differences; no FastQC upstream speed
ratio or full mathematical equivalence claim is allowed.

`check_optimization.py` additionally compares the two native FASTX binaries on
26 valid/malformed, boundary, compressed and truncated payloads under four
command forms. This checks preservation, not independent correctness of every
pre-existing behavior. Existing independent oracles and the two full quality
gates provide the separate correctness checks.

Retain raw pilot/formal observations, baseline/optimized hashes, source patch,
executed scripts, input-manifest hashes, build recipes, syscall traces and fresh
validation logs. Successful outputs may be deleted only after comparison. Failed
checks and incomplete populations must remain visible and must not produce a
successful summary. Never overwrite the original README evidence.
