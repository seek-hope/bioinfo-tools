# Benchmark runner

`run.py` measures the five shipped CLIs with deterministic generated inputs and a
correctness check on the same input. It writes one JSON document conforming to
[`schema.json`](schema.json).

```bash
# Build all binaries first.
cargo build --release --workspace --bins --locked

# Smoke matrix (1x/10x, one repetition).
python3 bench/run.py --sizes 1,10 --repeats 1 --out /tmp/bench-smoke.json

# Larger matrix; keep the temporary inputs for inspection.
python3 bench/run.py --sizes 1,10,100 --repeats 2 --keep --out bench/results/current.json
```

The base workload is 1,000 FASTQ reads of 100 bp (with an adapter in every tenth
read) and a synthetic SAM with one UMI-tagged record per three reads. BAM inputs
are prepared outside the timed section with `rust-bam view -b` and `rust-bam
sort`; `umi_dedup` uses the sorted BAM. This is a throughput/resource benchmark,
not an alignment benchmark and not a claim that synthetic UMI groups reproduce a
biological library.

## Cases

| Case | Command family | Correctness check |
| --- | --- | --- |
| `cutadapt` | `-a AGATCGGAAGAGC -m 30 -j 4` | output gzip decodes; record count does not exceed input |
| `cutadapt_j1` | same with `-j 1` | same; same-resource comparison |
| `fastqc` | `--stdout`/`-o` data report | `Total Sequences` equals input count |
| `fx_stats` | `stats -a -T` | `num_seqs` equals input count |
| `bam_sort` | default `sort` | `quickcheck` plus record count equals input |
| `bam_sort_lowmem` | `--max-memory 1MiB --max-open-files 8` | same; external-sort path |
| `umi_dedup` | `dedup --extract-umi-method=tag --umi-tag=UB` | `quickcheck` plus nonzero record count |

## Recorded fields

Every result records the command vector, working directory, thread count,
compression, repeats, input SHA-256, binary SHA-256 and version, per-sample
wall/user/sys/RSS/temp-disk/output sizes, a summary, and the correctness verdict.
The document records the Git commit and dirty paths, CPU/memory, platform, Rust
and Cargo versions, and explicit limitations.

`/usr/bin/time -v` supplies wall, CPU and peak RSS. A small sampler measures the
peak bytes under the case's `TMPDIR`; it is a sampled observation, not a hard
filesystem quota. Every timed command runs in its own case directory. Inputs are
generated outside the timed command, so generation is not included in the
measurement.

## Scope and limits

- It does not compare every case to cutadapt/SeqKit/FastQC/UMI-tools oracles; the
  correctness checks are structural/record-count based. Oracle suites remain
  separate and are never replaced by this runner.
- It does not establish a universal speedup. A number belongs only to the recorded
  binary, commit, input and hardware. The historical cutadapt 6.60x and UMI 15.49x
  are not reproduced or generalized here.
- It does not measure a hard memory limit for tools without one. For BAM sort,
  `--max-memory` is the chunk budget; the process/merge overhead is reported by
  RSS.
- It does not claim hosted-runner or cross-platform performance. Shared-machine
  noise is reduced by repeats but not eliminated.

## Upstream correctness benchmark

`upstream.py` complements this structural runner with direct upstream output
comparison. It runs the rust binary and the pinned upstream binary on the same
generated input, for cutadapt, FastQC, SeqKit stats, samtools sort and UMI-tools
dedup. It records the same `/usr/bin/time -v` and provenance fields, plus output
hashes and a normalized comparison:

```sh
export PATH="$PWD/tests/ci/env/bin:$PATH"
export CUTADAPT_ORACLE=/path/to/cutadapt-4.6
python3 bench/upstream.py --sizes 1,10 --repeats 2 \
  --out bench/results/upstream-current.json
```

In the recorded 1x/10x, two-repeat run, all 10 size/case combinations were
output-equivalent ([`results/upstream-current.json`](results/upstream-current.json)).
`fastqc_data.txt` was byte-identical after the FastQC Adapter Content and Basic
Statistics unit fixes; BAM cases compare records plus a header with `@PG`
provenance lines removed, while FASTQ and TSV cases compare bytes. Upstream binaries are supplied via
`CUTADAPT_ORACLE`/`UMI_TOOLS_ORACLE`/`SAMTOOLS_ORACLE`/`FASTQC_ORACLE`/
`SEQKIT_ORACLE`, or resolved from `tests/ci/env/bin`.

FastQC upstream timing is dominated by JVM startup and temporary files; the
recorded 1x/10x speed ratios are not universal performance claims. Small
`fx_stats`/`bam_sort` cases are sub-20ms and their ratios are startup-noise
dominated. This runner compares `fastqc_data.txt`; it does not compare the HTML
or ZIP artifacts.
