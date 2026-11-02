# Statistics and BAM merge selection after 21bc596

The accepted changes replace repeated byte classification with packed integer
contributions and replace a heap of merge records with an index-only selection
tree. On the calibrated inputs, statistics ran faster than SeqKit; BAM sorting
remained slower than samtools. No new dependency or CLI option was introduced.
The parallel temporary-compression prototype was tested and withdrawn.

## Formal upstream/current comparison

The run completed on 2026-11-02, 2026-11-02T03:02:52.762541+00:00 through
2026-11-02T04:35:02.085050+00:00. All 9 conditions, 54 measured observations, 18 warm-ups
and 36 paired output checks passed. The shortest measured invocation was
19.579817 s; none was below 10 s. Ratios below are the
median of three within-block upstream/current ratios, not ratios of the displayed
medians. There is no suite-wide average.

| Workload | Repeated population | Input records | Input GiB | CPU budget | Upstream s, median [min–max] | Current s, median [min–max] | Paired upstream/current |
|---|---|---:|---:|---:|---:|---:|---:|
| SeqKit stats -a -T | real FASTQ subset | 120M | 41.10 | 1 | 60.36 [58.83–60.47] | 21.44 [21.42–21.44] | 2.82× |
| SeqKit stats -a -T | real FASTQ subset | 120M | 41.10 | 2 | 61.40 [61.06–64.68] | 21.47 [21.42–21.60] | 2.86× |
| SeqKit stats -a -T | real FASTQ subset | 120M | 41.10 | 4 | 61.86 [61.63–61.87] | 21.79 [21.70–21.79] | 2.84× |
| SeqKit stats -a -T | synthetic FASTQ | 120M | 36.21 | 1 | 45.52 [44.70–46.08] | 19.59 [19.58–19.60] | 2.32× |
| SeqKit stats -a -T | synthetic FASTQ | 120M | 36.21 | 2 | 46.32 [46.17–47.93] | 19.59 [19.58–19.73] | 2.36× |
| SeqKit stats -a -T | synthetic FASTQ | 120M | 36.21 | 4 | 46.50 [46.43–46.73] | 19.91 [19.90–19.91] | 2.34× |
| samtools sort | synthetic BAM | 86M | 0.39 | 1 | 49.14 [47.62–50.82] | 96.65 [92.94–100.80] | 0.51× |
| samtools sort | synthetic BAM | 86M | 0.39 | 2 | 53.51 [53.48–53.86] | 82.42 [81.59–82.98] | 0.65× |
| samtools sort | synthetic BAM | 86M | 0.39 | 4 | 23.35 [23.32–23.52] | 82.20 [82.00–82.40] | 0.28× |


The source populations are one million real 150 bp reads from ENA SRR2584863,
one million synthetic 150 bp reads and one million synthetic 50 bp BAM records.
Statistics repeat each FASTQ 120 times. Sort reuses and fully rehashes the earlier
86-fold BAM. Repetition adds work, not sequence diversity or biological replication.
All budgets use the same workload input. The previous [scale experiment](SCALE.md)
is separate evidence with different FASTQ sizes and native executables.

## Method and resources

The [scale protocol](SCALE-PROTOCOL.md) was used without measurement-script changes.
Calibration ran both arms in every condition and qualified all 18 invocations;
its minimum was 19.256966 s. The plan was then frozen. Seed 20261103 balances both
arm orders twice across one warm-up and three measured blocks. Builds, generation,
correctness suites and other benchmarks were outside accepted timing.

The host was an i7-13700H, 20 logical CPUs, 31.13 GiB RAM,
Linux 7.3.0-4-cachyos, with inputs/output/temp on NVMe-backed ext4
(`/dev/nvme0n1p2`, `rw,noatime`). Affinities were CPU4, CPU4/6 and CPU0/2/4/6,
representing 1/2/4 physical P cores. CPU4/6 have 5.0 GHz maximum frequency;
CPU0/2 have 4.8 GHz maxima. SMT siblings remained available to background work.
Frequency was unlocked. Sampled temperatures spanned 33–100 °C
and one-minute load 1.00–3.08; these are sampled conditions, not controlled constants.

References were SeqKit 2.14.0 and samtools 1.24; native builds used Rust 1.99.1
with Cargo --locked. Native sort used a 512 MiB estimated chunk budget and final
compression workers 1/2/4. Samtools used `-@ 0/1/3` and `-m 512M/256M/128M`.
These options are not equal total-RSS caps or a pure thread-count scaling test.
Final compression level was 6. Exact commands, executable/runtime/source hashes,
input sizes, wall/user/system time and I/O counters are in the raw report.

| Condition | Peak RSS MiB upstream/current | Effective CPUs upstream/current | Sampled temp GiB upstream/current | Untimed fsync s upstream/current |
|---|---:|---:|---:|---:|
| real-stats-c1 | 31.8/5.2 | 1.00/0.77 | 0.000/0.000 | 0.002/0.002 |
| real-stats-c2 | 31.8/5.5 | 1.00/0.76 | 0.000/0.000 | 0.002/0.002 |
| real-stats-c4 | 31.3/5.5 | 0.99/0.77 | 0.000/0.000 | 0.002/0.002 |
| synthetic-stats-c1 | 31.9/5.3 | 1.00/0.81 | 0.000/0.000 | 0.002/0.002 |
| synthetic-stats-c2 | 31.0/5.4 | 1.00/0.81 | 0.000/0.000 | 0.002/0.002 |
| synthetic-stats-c4 | 31.5/5.4 | 0.99/0.81 | 0.000/0.000 | 0.002/0.002 |
| bam-sort-c1 | 649.2/678.0 | 0.99/0.99 | 0.332/1.966 | 0.001/0.020 |
| bam-sort-c2 | 395.6/678.5 | 0.99/1.12 | 0.334/1.966 | 0.001/0.016 |
| bam-sort-c4 | 576.4/677.9 | 3.04/1.26 | 0.455/1.966 | 0.002/0.023 |


Effective CPU use is (user + system time)/wall time. Statistics remain serial;
-j is accepted but unused. Native sort parallelizes only final BAM compression.
Its four-core observation used a median 1.26 CPUs and remained slower than the
reference. GNU time RSS is not a simultaneous descendant aggregate; approximately
one-second sampling can miss temporary-space or temperature peaks. Sequential
read-conditioning does not guarantee cache residency. CLI timing includes startup
and ordinary I/O; post-exit fsync is untimed and reported separately. Low CPU use
alone does not identify a particular SSD or cache mechanism.

## Complexity, implementation, then concurrency

For statistics, let B include all FASTX input bytes, N be records and D distinct
lengths. The bound remains O(B + N log D), with O(D + largest record/input buffer)
state plus constant tables. These equal-length populations have D=1. The observed
roughly 5 MiB RSS does not establish constant memory for arbitrary input.

`SequenceCounter` precomputes GC, AT, N-or-X and arbitrary gap membership in four
16-bit lanes. `QualityDecoder` packs Q20/Q30 contributions in two lanes. Chunks of
at most 65535 bytes prevent inter-lane carry, including overlapping categories and
all-set lanes. Quality error probabilities still add in byte order into one
accumulator across chunks, then merge in original record order. Floating-point
addition was not replaced with multiplication or histogram reduction.

For external sorting, chunk comparisons remain O(N log C), for C records per
chunk. Each merge level reads/decodes/compresses records and performs O(N log F)
key comparisons for fan-in F; names can add comparison cost. The number of merge
levels is unchanged. The new selection tree builds in O(k), replacing k heap
pushes, and repairs one head in O(log k). It makes at most one key comparison per
level and stores fewer than 4k indices for nonempty k. Record/key/frame handles
stay in fixed slots. The tree height remains based on original slots as runs end.

Full BAM decoding, original global indices, run-index tie fallback and sidecar EOF
semantics remain. Final merging refills before emitting the selected record;
intermediate merging writes its record/frame and index before refilling. The
64 KiB frame-capture threshold limits the added copy only; retained capacities,
reader/decoder buffers, headers and run heads still contribute to memory.

Callgrind on matching 100k inputs, CPU4, showed:

| Workload | Baseline instructions | Current instructions | Reduction |
| --- | ---: | ---: | ---: |
| Real stats -a -T | 563,983,731 | 153,092,893 | 72.86% |
| Synthetic stats -a -T | 560,938,463 | 150,047,438 | 73.25% |
| Sort, 1 MiB chunk, 1 worker | 2,103,324,121 | 1,976,122,580 | 6.05% |
| Flagstat, unchanged path | 164,865,213 | 164,865,740 | approximately unchanged |

The original stats callback accounted for 93.35% of instructions. Heap selection
and movement accounted for about 6.8% of the sort diagnostic instructions;
compression, full decoding, conversion and allocation remain substantial.
Inclusive call-graph percentages overlap. These are instruction counts under
instrumentation, not wall-time speedups. All profiled outputs matched. Final sealed
binaries are byte-identical to the profiled serial candidate; later changes added
tests only. Both candidate source snapshots were verified against their seal hashes.

Only after the serial changes was temporary-run parallel BGZF compression tested.
The prototype used the existing ordered noodles writer at level 1. Its first
build lacked a trait import; that failure is retained. The corrected prototype
passed component tests, clippy and every output comparison, but its 2/4-core 10M
screens were slower. It was reverted. Per-writer pools and queues are real costs;
the experiment did not isolate their individual contribution or establish a
universal conclusion about parallel compression.

## Supplementary candidate screens

Each screen has 32 invocations: four cases, one warm-up and three measured blocks
per arm, balanced orders with seed 20261102, fixed affinities, output hashes and
untimed fsync. Every output matched. These short screening runs are separate from
the calibrated comparison above. Timeout polling affects subsecond resolution;
the 1M stats and 100k sort timings are not formal speedup evidence.

### Baseline versus serial candidate

| Case | First arm median s | Second arm median s |
| --- | ---: | ---: |
| stats-real | 0.565017 | 0.164183 |
| stats-synthetic | 0.565016 | 0.164412 |
| sort-c1 | 10.840645 | 10.283869 |
| sort-c4 | 8.582623 | 7.782582 |
### Serial candidate versus rejected parallel candidate

| Case | First arm median s | Second arm median s |
| --- | ---: | ---: |
| sort-c1 | 9.886029 | 9.985468 |
| sort-c2 | 8.781167 | 9.032945 |
| sort-c4 | 7.733427 | 8.130794 |
| sort-small-runs-c4 | 0.113582 | 0.163787 |

In screen 1 the first arm is 21bc596 and the second is the serial candidate.
In screen 2 the first arm is that serial candidate and the second is the rejected
temporary-compression prototype. Neither table compares against upstream.

## Correctness and evidence

Fresh commands returned:

```text
bash tests/ci/check.sh
QUALITY PASS (base only; external oracles NOT RUN)
bash tests/ci/check.sh --with-fastx-qc-oracles
QUALITY PASS (base + FASTX/QC oracles)
Optimization behavior: 104/104
50/50 cases passed
20/20 cases passed
PASS independent audit: 9 timing medians/ranges, 9 paired ratios, 72 invocations, 36 checks
PASS final identities: 152 files rehashed
```

New tests cover lane boundaries, all-byte gap membership, quality thresholds and
bitwise sums across chunks/records, selection ties and exhaustion/reactivation,
and small/over-capture-limit/small records through actual spill merges. Existing
malformed-field, index EOF, refill-order, long-CIGAR, auxiliary-field and output
failure tests remain in the full gates. The sealed executables also passed the
FX baseline behavior check and pinned SeqKit/fq and samtools oracles.

Formal statistics checks compare the complete declared table. BAM checks include
quickcheck, canonical EOF, all normalized fields/counts and ordering, excluding
only @PG and permitted equal-coordinate ordering. Native byte signatures match
across blocks and CPU budgets. No fixture, comparison harness, compatibility row,
protected UMI source or approved exception was changed. No new upstream correction
or broader compatibility claim is introduced.

The [archive](results/bottleneck-21bc596/README.md) contains raw observations, per-invocation logs,
executed helpers, input manifests, source snapshots/patch, build provenance,
validation, both screens and the rejected prototype. Large reproducible payloads
and executables stay outside Git; their hashes and recipes remain. The audit
checks the complete population and recomputes medians/ranges and paired ratios;
input/runtime/source identities passed, and final route sampling matches the
mid-run sample. SHA-256 coverage binds the archived bytes, not a release signature.

For a new run use [the reproduction guide](SCALE-REPRODUCTION.md), selecting
real-stats, synthetic-stats and bam-sort before preparation. This run started at
multiplicities 120/120/86 and qualified in one calibration round. Review hardware
affinities before calibration, use fresh destinations and retain all reused input
dependencies. Do not reuse the archived frozen plan as qualification for changed
binaries, scripts or hosts.

A follow-up static review identifies full-level temporary merging as a candidate
for reducing intermediate work near the fan-in boundary. Actual run counts and
the I/O-versus-comparison tradeoff still require measurement; it is not an
implemented improvement or an executed probe in this delivery.
