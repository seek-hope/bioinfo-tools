# bioinfo-tools

[English](README.md) | [简体中文](README.zh-CN.md)

Native Rust command-line tools for sequencing data preparation, sequence-file
operations and quality control. Five CLIs share FASTX parsing, compression and
reporting infrastructure, with behavior checked against pinned upstream tools.
Performance varies by command; the [local benchmarks](bench/RESULTS-README.md)
include both faster and slower results.

## Tools and their place in an analysis

FASTQ stores reads and their quality scores; FASTA stores sequences without
qualities. SAM/BAM stores alignments. A UMI is a sequence tag used to identify
original molecules and distinguish them from amplification duplicates.

A typical workflow checks reads, extracts UMIs when the library uses them,
removes adapters, aligns reads with an **external aligner**, then sorts, indexes
and summarizes alignments or deduplicates molecules. This repository supplies
selected data-processing steps; it does not include an aligner.

| CLI | Typical work | Compatibility target |
| --- | --- | --- |
| `rust-cutadapt` | Adapter trimming, quality/length filtering, paired reads and demultiplexing | [cutadapt](https://cutadapt.readthedocs.io/) 4.6 |
| `rust-umitools` | UMI extraction, whitelists, deduplication, grouping and counting | [UMI-tools](https://umi-tools.readthedocs.io/) 1.1.6 |
| `rust-bam` | SAM/BAM conversion, coordinate sorting, indexing, flag statistics and selected depth/coverage operations | [samtools](https://www.htslib.org/) 1.24 |
| `rust-fastqc` | Twelve analysis modules, configurable thresholds/adapters/contaminants, data and summary reports | [FastQC](https://www.bioinformatics.babraham.ac.uk/projects/fastqc/) 0.13.0 |
| `rust-fx` | FASTA/FASTQ statistics, conversion, filtering, sequence transformations and validation | [SeqKit](https://bioinf.shenwei.me/seqkit/) 2.14.0 / [fq](https://github.com/stjude-rust-labs/fq) 0.12.0 |

`bioinfo-fastx` provides shared sequence I/O and codecs; `bioinfo-common` provides
shared diagnostics, exit-code helpers and provenance. Native commands do not
invoke the upstream Python or Java programs. Development and comparison tests
use those programs as references.

## Build and install

Build all five CLIs from this source tree:

```bash
git clone https://github.com/seek-hope/bioinfo-tools.git
cd bioinfo-tools
cargo build --release --workspace --bins --locked
export PATH="$PWD/target/release:$PATH"
```

The workspace declares Rust **1.91** as its minimum and pins **1.99.1** in
`rust-toolchain.toml`. A C build toolchain is also needed for codec dependencies.
`Cargo.lock` fixes dependencies; normal builds may download missing packages.

The [distribution repository](https://github.com/seek-hope/bioinfo-tools-dist)
and [installation guide](dist/README.md) cover prebuilt `rust-cutadapt` and
`rust-umitools`. The current release workflow packages those two tools; build
the other three from source. A published binary may predate the changes and
measurements documented here.

## Quick start

These examples use your own input files. Check each command's `--help` and the
[compatibility matrix](compat/README.md) before replacing an existing pipeline step.

### Inspect, validate and convert reads

```bash
rust-fx stats -a -T reads.fastq.gz
rust-fx lint reads.fastq.gz
rust-fx fq2fa -w 0 reads.fastq.gz -o reads.fasta
```

### Trim adapters and create a QC report

Use the adapter sequence appropriate for your library; this sequence is an example.

```bash
rust-cutadapt -a AGATCGGAAGAGC -m 30 -j 4 \
  -o trimmed.fastq.gz reads.fastq.gz
mkdir -p qc
rust-fastqc --extract -o qc trimmed.fastq.gz
```

FastQC output includes `fastqc_data.txt`, `summary.txt`, HTML and ZIP artifacts.
The native HTML currently presents data tables; full plot resources remain a
compatibility gap. Optional modules depend on input and configuration.

### Extract UMIs, then process alignments

This example places an eight-base UMI from the start of each read into its name.
Change the barcode pattern to match your library design.

```bash
rust-umitools extract -p NNNNNNNN -I reads.fastq.gz -S umi.fastq.gz
rust-cutadapt -a AGATCGGAAGAGC -m 30 -j 4 \
  -o umi.trimmed.fastq.gz umi.fastq.gz
```

Align `umi.trimmed.fastq.gz` using an external aligner that preserves those read names,
producing `aligned.bam`. Then run:

```bash
rust-bam sort -o sorted.bam aligned.bam
rust-bam index sorted.bam
rust-bam flagstat sorted.bam
rust-umitools dedup --random-seed 1 --compresslevel 6 \
  -I sorted.bam -S dedup.bam
```

Deduplication here uses UMIs in read names. For an existing BAM with UMIs in a
`UB` tag, use `--extract-umi-method=tag --umi-tag=UB` instead. The repository also
contains a [small executable UMI workflow](examples/umi-workflow/README.md).

## Compatibility and current limits

Support is versioned and command-specific. The [matrix](compat/README.md) has
266 rows: **189 verified, 9 experimental, 57 unsupported and 11 exception**.
Rows cover different kinds of behavior and are not a completion percentage.

- `verified` means the named inputs and comparison rules have executable evidence.
- `experimental` records implemented behavior with remaining gaps;
  `unsupported` records absent or rejected capabilities.
- `exception` records a reviewed behavior difference with a rule and proof. The
  separate `unachievable` column contains reasons only for two JVM launcher options
  excluded by the native-runtime policy.

Known limits include incomplete samtools/SeqKit command coverage, no CRAM in
`rust-bam`, single-threaded `rust-bam sort`, and an accepted but currently unused
`rust-fx -j` thread count. FastQC HTML/ZIP resources are incomplete. The new
150 bp benchmark also exposes unresolved FastQC GC-distribution and duplication
estimate differences; its complete reports are **not** established as equivalent.

Some upstream defects have been independently demonstrated and corrected; see the
[proofs and registered exceptions](docs/EVIDENCE-UPSTREAM-DEFECTS.md). Newly
observed mismatches require investigation before they can use that policy.
Streaming input does not guarantee fixed total memory: QC retains data by read
position, tile and distinct sequence, while sorting and grouping have their own
memory and temporary-storage requirements.

## Measured performance

<!-- README_BENCHMARK_START -->
Measured on **2026-10-31**: Linux, i7-13700H, 31.1 GiB visible RAM, with inputs,
outputs and temporary files on **tmpfs**. Commands and descendants used one P-core
logical CPU; only the labeled four-CPU case used four distinct P cores. Each
condition had one warm-up and five paired fresh-process repetitions. The shared
laptop was not frequency-locked; recorded package temperatures reached 100°C.
The measured `rust-fx` used a per-package release rebuild after the workspace
build; [build details](bench/RESULTS-README.md#source-failed-pilot-and-review)
identify the exact artifacts.

The first six rows use **1M real 150 bp reads** from SRR2584863; the last three use
**1M synthetic BAM records**. Times are medians. Ratios are medians of paired
upstream/native times: **above 1 favors native; below 1 means native is slower**.
RSS is the maximum GNU time value across five runs, not process-tree total memory.

| Operation | Native s | Upstream s | Paired ratio | Peak RSS MiB native/upstream |
| --- | ---: | ---: | ---: | ---: |
| cutadapt, plain, 1 CPU | 2.872 | 1.840 | 0.64× | 9.7 / 18.7 |
| cutadapt, plain, 4 CPUs | 0.779 | 0.772 | 0.99× | 13.2 / 36.3 |
| cutadapt, gzip level 6 | 9.438 | 8.573 | 0.91× | 13.0 / 19.0 |
| SeqKit stats -a -T | 2.225 | 0.566 | 0.25× | 5.6 / 36.6 |
| SeqKit fq2fa | 0.212 | 0.270 | 1.29× | 5.4 / 34.3 |
| fq lint | 0.581 | 0.201 | 0.34× | 150.2 / 4.4 |
| samtools flagstat | 0.093 | 0.038 | 0.41× | 4.2 / 4.2 |
| samtools sort (32 MiB chunks) | 1.701 | 0.614 | 0.36× | 43.5 / 54.8 |
| UMI-tools directional dedup | 0.453 | 5.116 | 11.20× | 256.3 / 326.8 |

The prespecified **10M synthetic-record extension** checks short streaming cases:

| Operation | Native s | Upstream s | Paired ratio | Peak RSS MiB native/upstream |
| --- | ---: | ---: | ---: | ---: |
| SeqKit fq2fa | 1.980 | 1.289 | 0.65× | 5.4 / 34.7 |
| fq lint | 6.467 | 1.959 | 0.30× | 829.4 / 4.4 |
| samtools flagstat | 0.960 | 0.374 | 0.39× | 4.1 / 4.1 |

The UMI advantage applies to this synthetic four-UMI-per-bundle workload. Several
commands are slower, and native lint retains substantially more memory here.
Timings include startup, compression and file I/O; they do not isolate algorithms.
All 100k/1M synthetic cases and observed min–max ranges are in the full report.

**FastQC has unequal report work and unresolved GC/duplication differences**, so
these are descriptive timings without a speed ratio. Independent benchmark checks
cover total reads, displayed total bases and length, not complete QC equivalence.

| FastQC input | Native s | Upstream s | Peak RSS MiB native/upstream |
| --- | ---: | ---: | ---: |
| 1M real reads | 9.306 | 6.197 | 92.1 / 322.9 |
| 1M synthetic reads | 9.290 | 6.188 | 110.0 / 298.4 |
<!-- README_BENCHMARK_END -->

Full commands, individual observations, data hashes, resource measurements and
reproduction instructions are in the [benchmark report](bench/RESULTS-README.md)
and [benchmark guide](bench/README.md). FastQC is reported separately because its
report work and some module values differ. Results apply to the measured inputs,
binaries and machine; they do not establish a suite-wide speedup.

## Roadmap

The near-term priority is to complete and verify the existing tools: remaining
command/argument/format coverage, complete FastQC graphics and resources, and the
GC/duplication discrepancies exposed by these benchmarks. Performance work will
use profiling and same-input correctness checks to guide optimization. Packaging
additional CLIs follows their validation and release review.

Future candidates include genomic interval operations (BED/GFF and overlaps),
feature counting and expression-data utilities, and VCF/BCF queries, filtering
and statistics. These are candidates, with no committed implementation dates.
See the [development plan](docs/DEVELOPMENT-PLAN.md) for scope and priorities.

## Test and contribute

```bash
bash tests/ci/check.sh
# After installing the pinned external references:
bash tests/ci/check.sh --with-fastx-qc-oracles
```

The base gate checks documentation, compatibility policy, formatting, lint,
release tests/builds and a cutadapt smoke test. The optional gate adds the pinned
FASTX/QC comparisons; it is not a rerun of every historical component oracle.
See [CI setup and coverage](tests/ci/README.md). Contributions should include
reproducible cases and respect the [workspace rules](AGENTS.md).

This monorepo is the canonical source tree; sibling checkouts are unnecessary.
[HISTORY.md](HISTORY.md) records the original repositories and their archives.
Historical `docs/EVIDENCE-*.md` files describe particular builds and are not
substitutes for current support or performance evidence.
