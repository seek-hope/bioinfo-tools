# Larger-input upstream/current comparison after 7a7ba28

The formal run completed on 2026-11-01 from 12:19:10 to 18:05:08 UTC. All
28 conditions, 168 measured observations, 56 warm-ups and 112 paired checks
passed their specified validation rules. The final input, executable, runtime
and measurement-source checks also passed. The shortest measured invocation
was 14.759773495985428 s; none was below 10 s.

See the [evidence archive](results/scale-7a7ba28/README.md),
[protocol](SCALE-PROTOCOL.md) and [reproduction guide](SCALE-REPRODUCTION.md).
The archive contains every calibration, formal observation and executed source
snapshot, including failed development attempts. The native baseline was
`7a7ba282acaa96f453200164cae14ab8c12b436c`; the sealed source patch and
build provenance identify this batch. The intervening `dacff08` commit changed
only the bilingual README and its writing evidence.

The host was an Intel i7-13700H laptop with 31.1 GiB RAM, using the local
UMIS RPEYJ1T24MKN2QWY NVMe drive, `/dev/nvme0n1p2`, ext4 with `rw,noatime`.
References were cutadapt 4.6, UMI-tools 1.1.6, samtools 1.24, FastQC 0.13.0,
SeqKit 2.14.0 and fq 0.12.0. Java was 25.0.2; the locked native build used
Rust 1.99.1. MultiQC 1.35 was used in the local oracle gate.

## What changed

The native build extends the preceding optimization batch in three areas:

- cutadapt uses a Myers bit-vector prefilter for eligible canonical 3′ adapters
  of 1–64 bases. It rejects only provable nonmatches. Every possible match, score,
  tie and unsupported filter configuration still goes through the scalar aligner.
- FX stats, fq2fa and lint can borrow complete four-line FASTQ records from the
  input buffer. Boundary-spanning records, FASTA, wrapped input and strict errors
  retain the owned parser. Per-file gap/quality/validation setup is reused;
  floating-point additions retain their original order. The direct memchr 2.8.3
  edge reuses an already locked dependency with default features disabled.
- BAM sort moves records into chunks, sorts small indices using total keys,
  reuses merge storage and forwards normalized internal
  frames after the ordinary full decoder validates them. `-@/--threads` above 1
  parallelizes final BGZF compression. Sorting, intermediate runs and SAM output
  remain serial. Flagstat still performs full decoding.

All changes retain strict decoding and output/order/error contracts in their
specified scopes. The 64 KiB capture threshold applies only to added frame data;
vector capacity can be larger and noodles retains its largest body buffer for
each open run. Sort's 512 MiB chunk option is not a total RSS cap.

The sealed binaries passed FX 104/104 behavior comparisons and cutadapt 119/119
pinned differential cases. Both full local gates passed again after the new
benchmark controls were integrated. Native sort tests cover worker counts,
spills/multipass merges, long CIGARs/auxiliary fields, corrupt fields and ordering
of errors/refills. Benchmark controls cover complete populations, exact commands
and affinities, finite metrics, calibration exclusion, within-block ratios,
output evidence, source reuse, process failures/timeouts and failed-output
retention. The archived logs contain:

```text
QUALITY PASS (base only; external oracles NOT RUN)
QUALITY PASS (base + FASTX/QC oracles)
Optimization behavior: 104/104
119/119 cases passed
SCALE SUMMARY PASS: 28 conditions, 168 measured observations, 56 warmups
```

The two gate commands were `bash tests/ci/check.sh` and
`bash tests/ci/check.sh --with-fastx-qc-oracles`. They were run before the
accepted timing, after the native and control changes; both exited 0.
See `accept-base.log`, `accept-oracles.log`, the sealed component logs and
`formal-summary.log` in the archive.

## Measurement design

Upstream and current runtimes are the primary columns. Before/after evidence is
secondary; the earlier accepted optimization archive and the initial README
archive remain unchanged. This batch's subsecond exploratory trials are not
substitutes for its formal results. Trial 5's `phase4` label accidentally selected
four CPUs through suffix matching; its actual command is retained and that
observation must never be labeled single-core.

The starting populations are the frozen 1M order-preserving SRR2584863 real-read
subset, 1M synthetic 150 bp FASTQ, and1M synthetic 50 bp BAM. Integer repetition makes
longer processing workloads, not more biological samples or distinct sequences.
Real and synthetic FASTQ remain separate. BAM repetition preserves original
names/fields; UMI input is globally sorted and indexed afterward.

UMI's preservation argument is specific to this frozen four-UMI population:
counts 6s/2s/s/s, pair distances 1, 2, 10 / 1, 9 / 8. For integer multiplicity m≥1, the
eligible forward inequalities remain 6ms≥4ms−1 and 2ms≥2ms−1, their reverse
inequalities remain false, and the distant UMI remains isolated. Identical copies
of each unique highest-MAPQ source representative preserve the expected output
set. Uniform count multiplication is not a general directional-clustering
invariant. The benchmark uses UB tags and random seed 1.

All applicable arm/core combinations are calibrated. Proposals target 15 s for
the fastest observation and never shrink a population; every final calibration
observation must reach 10 s before freezing. The same workload size is used for
all its core budgets. Calibration 1 passed its output checks but had three
observations below the minimum. Calibration 2 increased only UMI from 37M to
71M and QC from 5M to 9M, reusing the other nine populations. All 56 final
calibration observations were at least 14.700760072999401 s. Formal
measurements use one warm-up and three measured
blocks; each arm order occurs twice across those four blocks. Report medians,
min–max and the median of three within-block upstream/current ratios. Retain and
flag any measured observation below 10 s. No suite-wide average or confidence
interval is reported.

Affinities `4`; `4,6`; `0,2,4,6` select distinct P cores. CPU 4/6 have 5.0 GHz maximum turbo;
CPU 0/2 have 4.8 GHz. Shared SMT siblings and unlocked frequency remain limitations.
An allocated core budget does not mean the command uses that many cores: FX `-j`
is unused, native flagstat is serial, native FastQC handles one input serially,
and native BAM sort workers compress only final BAM output. Upstream UMI-tools
also receives no invented concurrency option.

Native sort uses 512 MiB of chunk budget independent of core count. Samtools uses
`-m 512M/256M/128M` with `-@ 0/1/3` for the 1/2/4-core budgets. Both final
compression levels are 6. These settings do not impose equal total RSS limits;
the observed CPU use is reported below.

Inputs, outputs and temporary files use the local NVMe/ext4 filesystem. Every
input is sequentially read-conditioned before each invocation, followed by 2 s of
cooldown; large workloads are not claimed to reside fully in RAM. CLI time
includes startup and ordinary I/O. Post-exit fsync is untimed and separately
reported. Builds, input generation, bulk file transfers, correctness suites and
other benchmarks were kept outside CLI timing. Routine status reads continued.
The user-requested README rewrite paused the orchestration after both timed
processes exited for synthetic-cutadapt-c2's warm-up block, while output
validation was pending. The stop was at 2026-11-01T13:41:34.253966Z with no child process remaining;
the group resumed at 2026-11-01T14:10:26.228181Z. `readme-pause.json` records both timestamps and the
README commit. The experiment's start-to-finish interval includes this pause;
the recorded CLI observations do not. Sources, inputs and measured artifacts
stayed frozen throughout it. GNU time RSS is not the simultaneous sum across
descendant processes. Temporary storage is sampled, and 1 s telemetry
can miss shorter peaks. Low observed CPU utilization limits conclusions about
parallel compute scaling.

## Acceptance and interpretation

Each pair must pass full validation. FASTQ/FASTA/stats/flagstat compare complete
bytes, decoding gzip where applicable. Lint adds source validation and an invalid
tail after the entire expanded population. Sort compares every SAM field and
record, ignores only @PG and canonicalizes coordinate ties, checks coordinates
and BGZF finalization, and checks native encoded-byte determinism. UMI checks
full normalized records and the independent representative set. Native outputs
are checked across blocks and core budgets before deleting payloads; mismatch
anchors and the failing pair are retained.

FastQC has unresolved GC/duplication differences and incomplete graphics/report
resources. Its times are descriptive, with no speed ratio. Independent count,
displayed-base and length checks do not establish whole-report equivalence.
Each of the eight FastQC paired checks found 9 of 11 data modules equal; the
two differing modules were Per sequence GC content and Sequence Duplication
Levels. The static follow-up note identifies hypotheses for those differences;
it does not constitute a behavioral probe or an approved new exception.

Final calibration/formal runs must match inputs, executables, upstream runtime
payloads, measurement scripts and timing-tool identities. Complete reports,
executed sources, raw logs, failed attempts and source/build metadata are retained.
Large reproducible inputs/outputs and executable files are not Git artifacts;
record their hashes, sizes and generation/build recipes. Hash manifests provide
integrity checks, not a signature or release approval.

## Formal results

The paired ratio is computed within each measured block, then summarized by
its median. It can differ from dividing the two displayed time medians.
M denotes one million records; GiB is the actual on-disk input size. BAM
compression makes those byte counts much smaller than decoded working data.

| Workload | Repeated population | Input records | Input GiB | CPU budget | Upstream s, median [min–max] | Current s, median [min–max] | Paired upstream/current |
|---|---|---:|---:|---:|---:|---:|---:|
| cutadapt plain | real FASTQ subset | 54M | 18.50 | 1 | 127.21 [120.21–141.27] | 145.22 [142.22–157.21] | 0.89× |
| cutadapt plain | real FASTQ subset | 54M | 18.50 | 2 | 99.02 [95.21–108.38] | 147.06 [143.26–175.30] | 0.66× |
| cutadapt plain | real FASTQ subset | 54M | 18.50 | 4 | 106.41 [78.99–106.69] | 138.99 [113.83–156.92] | 0.69× |
| cutadapt plain | synthetic FASTQ | 46M | 13.88 | 1 | 125.39 [113.53–132.81] | 112.06 [94.95–119.51] | 1.05× |
| cutadapt plain | synthetic FASTQ | 46M | 13.88 | 2 | 84.83 [66.85–92.55] | 121.61 [105.32–149.98] | 0.63× |
| cutadapt plain | synthetic FASTQ | 46M | 13.88 | 4 | 65.63 [61.38–72.03] | 72.77 [65.97–130.40] | 0.84× |
| cutadapt gzip6 output | real FASTQ subset | 8M | 2.74 | 1 | 65.34 [65.16–65.40] | 58.41 [58.32–58.62] | 1.12× |
| cutadapt gzip6 output | real FASTQ subset | 8M | 2.74 | 2 | 53.84 [53.60–53.85] | 29.47 [29.38–29.53] | 1.83× |
| cutadapt gzip6 output | real FASTQ subset | 8M | 2.74 | 4 | 21.63 [21.52–21.75] | 16.83 [16.76–16.87] | 1.28× |
| SeqKit stats -a -T | real FASTQ subset | 30M | 10.28 | 1 | 15.61 [15.51–15.69] | 15.47 [15.47–15.54] | 1.00× |
| SeqKit stats -a -T | real FASTQ subset | 30M | 10.28 | 2 | 15.36 [15.21–15.37] | 15.68 [15.55–15.68] | 0.98× |
| SeqKit stats -a -T | real FASTQ subset | 30M | 10.28 | 4 | 15.37 [15.36–15.44] | 15.92 [15.92–15.99] | 0.96× |
| SeqKit stats -a -T | synthetic FASTQ | 41M | 12.37 | 1 | 16.13 [15.91–16.15] | 21.21 [21.18–21.22] | 0.76× |
| SeqKit stats -a -T | synthetic FASTQ | 41M | 12.37 | 2 | 15.99 [15.72–16.11] | 21.47 [21.37–21.58] | 0.74× |
| SeqKit stats -a -T | synthetic FASTQ | 41M | 12.37 | 4 | 15.85 [15.82–16.15] | 21.93 [21.86–22.44] | 0.72× |
| SeqKit fq2fa | synthetic FASTQ | 146M | 44.06 | 1 | 155.21 [138.19–179.92] | 134.25 [119.64–138.81] | 1.16× |
| SeqKit fq2fa | synthetic FASTQ | 146M | 44.06 | 4 | 106.11 [103.83–132.82] | 135.81 [110.02–139.59] | 0.95× |
| fq lint | synthetic FASTQ | 102M | 30.78 | 1 | 28.35 [26.81–29.14] | 23.06 [22.60–23.28] | 1.23× |
| fq lint | synthetic FASTQ | 102M | 30.78 | 4 | 29.53 [29.41–30.40] | 27.34 [25.07–27.74] | 1.11× |
| samtools flagstat | synthetic BAM | 755M | 3.47 | 1 | 34.51 [28.53–34.66] | 69.57 [65.09–76.03] | 0.45× |
| samtools flagstat | synthetic BAM | 755M | 3.47 | 4 | 16.38 [16.35–16.43] | 60.60 [60.33–61.01] | 0.27× |
| samtools sort | synthetic BAM | 86M | 0.39 | 1 | 43.66 [42.45–44.77] | 94.89 [93.55–97.71] | 0.46× |
| samtools sort | synthetic BAM | 86M | 0.39 | 2 | 52.44 [52.40–52.47] | 91.77 [91.50–91.93] | 0.57× |
| samtools sort | synthetic BAM | 86M | 0.39 | 4 | 23.18 [23.14–23.18] | 91.61 [91.11–91.68] | 0.25× |
| UMI-tools directional dedup | synthetic BAM | 71M | 0.16 | 1 | 228.66 [223.20–237.04] | 15.27 [15.26–15.29] | 14.98× |
| UMI-tools directional dedup | synthetic BAM | 71M | 0.16 | 4 | 215.23 [215.09–216.18] | 14.80 [14.76–14.83] | 14.54× |
| FastQC (descriptive) | real FASTQ subset | 9M | 3.08 | 1 | 34.31 [34.29–34.31] | 40.15 [40.13–40.19] | — unequal report work |
| FastQC (descriptive) | real FASTQ subset | 9M | 3.08 | 4 | 15.09 [15.09–15.11] | 41.24 [41.02–41.45] | — unequal report work |

### Resource observations

RSS is the maximum GNU time observation across the three measured runs.
Effective CPU use is the median of (user + system CPU time) / elapsed time.
Temporary bytes are the maximum sampled value; fsync is the median untimed
post-exit flush duration. Raw I/O counters, frequency, temperature, load,
conditioning times and per-process logs remain in the formal report.

| Condition | Peak RSS MiB upstream/current | Effective CPUs upstream/current | Sampled temp GiB upstream/current | Untimed fsync s upstream/current |
|---|---:|---:|---:|---:|
| real-cutadapt-c1 | 18.6/15.2 | 0.68/0.38 | 0.000/0.000 | 0.088/0.173 |
| real-cutadapt-c2 | 36.4/20.5 | 1.22/0.39 | 0.000/0.000 | 0.077/0.016 |
| real-cutadapt-c4 | 36.3/24.9 | 1.19/0.44 | 0.000/0.000 | 0.016/0.045 |
| synthetic-cutadapt-c1 | 18.7/12.9 | 0.74/0.47 | 0.000/0.000 | 0.180/0.019 |
| synthetic-cutadapt-c2 | 37.6/17.0 | 1.53/0.47 | 0.000/0.000 | 0.041/0.155 |
| synthetic-cutadapt-c4 | 37.7/23.1 | 1.97/0.79 | 0.000/0.000 | 0.039/0.044 |
| real-cutadapt-gzip-c1 | 18.9/13.2 | 1.00/1.00 | 0.000/0.000 | 0.007/0.012 |
| real-cutadapt-gzip-c2 | 40.9/16.3 | 1.29/2.00 | 0.000/0.000 | 0.014/0.016 |
| real-cutadapt-gzip-c4 | 55.9/21.0 | 3.69/3.98 | 0.000/0.000 | 0.013/0.014 |
| real-stats-c1 | 35.1/5.4 | 1.00/1.00 | 0.000/0.000 | 0.002/0.002 |
| real-stats-c2 | 36.5/5.4 | 1.00/1.00 | 0.000/0.000 | 0.002/0.002 |
| real-stats-c4 | 34.8/5.5 | 1.00/0.99 | 0.000/0.000 | 0.002/0.002 |
| synthetic-stats-c1 | 35.7/5.5 | 1.00/1.00 | 0.000/0.000 | 0.002/0.002 |
| synthetic-stats-c2 | 32.5/5.2 | 1.00/1.00 | 0.000/0.000 | 0.002/0.002 |
| synthetic-stats-c4 | 36.0/5.2 | 0.99/0.99 | 0.000/0.000 | 0.002/0.002 |
| synthetic-fq2fa-c1 | 33.8/5.2 | 0.13/0.14 | 0.000/0.000 | 0.011/0.011 |
| synthetic-fq2fa-c4 | 28.3/5.0 | 0.19/0.14 | 0.000/0.000 | 0.084/0.081 |
| synthetic-lint-c1 | 4.3/5.0 | 0.96/0.95 | 0.000/0.000 | 0.014/0.013 |
| synthetic-lint-c4 | 4.3/4.9 | 0.94/0.94 | 0.000/0.000 | 0.014/0.013 |
| bam-flagstat-c1 | 3.6/4.0 | 0.99/0.99 | 0.000/0.000 | 0.013/0.014 |
| bam-flagstat-c4 | 4.0/4.1 | 2.61/0.99 | 0.000/0.000 | 0.046/0.053 |
| bam-sort-c1 | 648.9/678.0 | 0.99/0.99 | 0.332/1.966 | 0.001/0.015 |
| bam-sort-c2 | 395.4/678.2 | 0.99/1.16 | 0.334/1.966 | 0.001/0.016 |
| bam-sort-c4 | 576.0/678.5 | 3.06/1.21 | 0.455/1.966 | 0.002/0.015 |
| umi-dedup-c1 | 304.8/255.4 | 1.00/1.00 | 0.001/0.000 | 0.002/0.004 |
| umi-dedup-c4 | 304.8/255.8 | 0.99/1.00 | 0.001/0.000 | 0.002/0.004 |
| real-fastqc-c1 | 356.6/92.1 | 0.97/1.00 | 0.000/0.000 | 0.023/0.006 |
| real-fastqc-c4 | 506.4/91.7 | 2.35/0.99 | 0.000/0.000 | 0.021/0.006 |

### Interpretation and next optimization targets

- Gzip output trimming was faster for all three budgets: native medians were
  58.41, 29.47 and 16.83 s, with paired ratios of 1.12×, 1.83× and 1.28×.
  Native effective CPU use rose from 1.00 to 2.00 and 3.98.
- Plain-output trimming and FASTQ-to-FASTA conversion varied widely and spent
  much wall time outside process CPU execution. Native conversion used about
  0.14 effective CPUs. These are end-to-end compute/I/O measurements; a
  separate trace is needed to distinguish storage, buffering and scheduling
  costs. No SSD cache mechanism is inferred from the drive model or timings.
- Real-read statistics were close at one core and slower at the larger budgets.
  Synthetic statistics remained slower, at paired ratios of 0.72–0.76×.
  Native statistics used about one CPU and 5.2–5.5 MiB RSS. Its sequence and
  quality loops are a profiling target; ordered floating-point sums must stay
  unchanged.
- Lint was faster in both budgets, at 1.23× and 1.11×. Its native peak RSS was
  about 5 MiB for this valid single-file population. Paired-name state,
  diagnostics and large records have different memory requirements.
- Flagstat and sort remain slower than samtools. Four-core-budget medians were
  60.60 versus 16.38 s for flagstat and 91.61 versus 23.18 s for sort. Native
  sort used only 1.21 effective CPUs in that budget, reached 678.5 MiB RSS and
  1.966 GiB sampled temporary storage. Final compression parallelism leaves
  decoding, sorting and temporary merging serial. Further work needs profiles
  while retaining full BAM validation and the existing error/output order.
  Samtools sort at the two-core budget used `-@ 1 -m 256M`, about 0.99 effective
  CPUs, and was slower than its one-core setting. These rows describe the
  recorded configurations and are not a controlled test of thread count alone.
- UMI deduplication had paired ratios of 14.98× and 14.54×, with native medians
  of 15.27 and 14.80 s. These results apply to the frozen four-UMI graph and its
  unique highest-MAPQ representatives. Both implementations used about one
  effective CPU; the large speed difference does not establish parallel scaling.
- FastQC's reported times are descriptive. Native report hashes were stable
  across blocks and budgets, but GC/duplication and report-resource differences
  prevent an equal-work ratio. The underlying distinctions remain in each
  paired check.

Input streaming does not establish a constant total memory bound. Extended
statistics retain length counts; paired lint can retain names; QC retains
per-position and per-distinct-sequence state. Input repetition here does not
exercise increasing sequence diversity. The earlier [before/after report](OPTIMIZATION.md)
and [initial README measurements](RESULTS-README.md) use their own populations,
artifacts and storage protocols and remain supplementary evidence.


## Reproduction

Follow the [reproduction guide](SCALE-REPRODUCTION.md).
Use bench/scale_seal.py to build the locked workspace, rebuild package-only FX,
and compile the canonicalizer. Start with scale_plan.py, prepare inputs with
scale_inputs.py, run scale_benchmark.py --calibration, propose/reuse populations
until the minimum is met, freeze the plan, run without --calibration, and invoke
summarize_scale.py. Use new output directories at each stage. Input reuse requires
a completed manifest and matching source metadata/multiplicity/hashes. A new
host or changed instrumentation requires new calibration; archived times are
historical observations, not a substitute for that calibration.
