# bioinfo-tools

English | [简体中文](README.zh-CN.md)

## Overview

`bioinfo-tools` is a Rust monorepo containing five command-line tools and two shared libraries for sequencing data preprocessing, sequence file manipulation, quality control, and alignment file processing. Reference software packages at fixed versions are used during development to compare command behavior and outputs.

### Command-Line Tools

- **`rust-cutadapt`** (reference: cutadapt 4.6): Adapter removal, quality and length filtering, paired-end read processing, and sample demultiplexing.
- **`rust-umitools`** (reference: UMI-tools 1.1.6): UMI extraction, barcode whitelist generation, and alignment deduplication, grouping, and counting.
- **`rust-bam`** (reference: samtools 1.24): SAM/BAM format conversion, coordinate sorting, indexing, and `flagstat`, `idxstats`, `quickcheck`, `depth`, `coverage`, `flags`, and `head`.
- **`rust-fastqc`** (reference: FastQC 0.13.0): Runs 12 analysis modules with configurable thresholds, adapters, and contaminant lists; outputs `fastqc_data.txt`, `summary.txt`, HTML, and ZIP archives. The HTML report currently displays data tables while graphical plots continue in development.
- **`rust-fx`** (reference: SeqKit 2.14.0, fq 0.12.0): FASTA/FASTQ statistics, format conversion, filtering, sequence transformation, and validation through SeqKit- and fq-style subcommands.

### Shared Libraries

- **`bioinfo-fastx`**: Shared FASTA/FASTQ parsing, reading, writing, and compression handling.
- **`bioinfo-common`**: Shared diagnostics, exit codes, and build information.

See [compat/README.md](compat/README.md) for command-by-command support status and [HISTORY.md](HISTORY.md) for project history.

## Sequencing Data Processing Pipeline and Tool Roles

Common sequencing workflows operate on several primary file formats and molecular tags:

- **FASTQ**: Stores nucleotide sequences together with per-base Phred quality scores. Standard alignment pipelines retain FASTQ format through preprocessing so the aligner and quality-aware steps can use base quality scores.
- **FASTA**: Stores nucleotide sequences without quality scores, used for reference sequences or standalone sequence exports.
- **UMI (Unique Molecular Identifier)**: Short random bases added to molecules before amplification to distinguish original templates from PCR duplicates after alignment.
- **SAM/BAM**: Stores read alignments against a reference genome in text (SAM) or compressed binary (BAM) format.

```mermaid
flowchart TD
    raw["Raw reads (FASTQ / FASTQ.GZ)"]
    inspect["Raw read inspection & initial QC<br/><b>rust-fx</b> (stats, lint) / <b>rust-fastqc</b>"]
    fa_export["Standalone sequence export (FASTA)<br/><b>rust-fx</b> (fq2fa)"]
    umi["UMI extraction (for UMI libraries)<br/><b>rust-umitools</b> (extract)"]
    trim["Adapter trimming & quality/length filtering (FASTQ)<br/><b>rust-cutadapt</b>"]
    postqc["Post-trimming QC<br/><b>rust-fastqc</b>"]
    align["Sequence alignment on FASTQ (External software)<br/>DNA: BWA / minimap2<br/>RNA: STAR / HISAT2"]
    bam["SAM/BAM sorting, indexing & alignment stats<br/><b>rust-bam</b> (sort, index, flagstat, idxstats, quickcheck, depth, coverage, flags, head)"]
    dedup["UMI deduplication & grouping<br/><b>rust-umitools</b> (dedup, group)"]
    assign["Gene assignment (External software)<br/>Write gene assignment tags to BAM"]
    count["Gene-level UMI counting<br/><b>rust-umitools</b> (count)"]
    down["Downstream analysis (External software)<br/>Variant calling / Differential expression"]

    raw --> inspect
    raw -.->|"Sequence-only export"| fa_export
    inspect -->|"UMI library (FASTQ)"| umi
    umi -->|"FASTQ with UMI in read name"| trim
    inspect -->|"Standard library (FASTQ)"| trim
    trim --> postqc
    trim -->|"Trimmed FASTQ (with quality scores)"| align
    align -->|"SAM / unsorted BAM"| bam
    bam -->|"UMI library (coordinate-sorted BAM)"| dedup
    bam -->|"UMI counting workflow"| assign
    assign -->|"BAM with gene tags"| count
    dedup --> down
    count --> down
    bam -->|"Standard library"| down
```

### Workflow Steps

1. **Raw Read Inspection and Initial QC**: `rust-fx lint` checks FASTQ syntax and read record integrity, `rust-fx stats` summarizes read counts and lengths, and `rust-fastqc` evaluates per-base quality, GC content, sequence duplication, and adapter content. When sequences are needed without quality scores for external tasks, `rust-fx fq2fa` exports FASTQ records to FASTA as a separate step.
2. **UMI Extraction**: For UMI libraries, `rust-umitools extract` extracts the UMI bases according to the library pattern (for example, moving an 8-bp 5' UMI from the read sequence into the read name). When a barcode whitelist is required prior to extraction, `rust-umitools whitelist` derives it from the raw reads.
3. **Adapter Trimming and Filtering**: `rust-cutadapt` trims adapter sequences and low-quality ends and filters out reads shorter than the specified threshold, outputting trimmed FASTQ files with quality scores preserved.
4. **Post-Trimming QC**: `rust-fastqc` runs on the trimmed FASTQ files to verify adapter removal and read quality distribution.
5. **Alignment (External Software)**: External aligners such as BWA or minimap2 (for DNA data) or STAR or HISAT2 (for RNA data) map the trimmed FASTQ reads to a reference genome. In UMI workflows, the aligner preserves the read names containing the extracted UMIs.
6. **SAM/BAM Processing and UMI Deduplication**: `rust-bam` sorts alignments by coordinate (`sort`), builds indexes (`index`), and inspects alignments (`flagstat`, `idxstats`, `quickcheck`, `depth`, `coverage`, `flags`, `head`). For UMI libraries, `rust-umitools dedup` (or `group`) takes the coordinate-sorted BAM and removes PCR duplicates using alignment positions and UMI strings.
7. **Gene Assignment and Further Analysis**: For gene-level UMI counting, an external gene assignment tool first annotates the BAM alignments with gene tags, after which `rust-umitools count` tabulates UMI counts per gene. Variant calling and differential expression analysis follow as downstream steps according to the experimental design.

See [examples/umi-workflow/README.md](examples/umi-workflow/README.md) for a runnable UMI workflow example. That directory includes a synthetic demo with pre-generated SAM/BAM files and also supports execution with external aligners.

## Installation and Usage

### Build from Source

Building from source requires a C compiler toolchain and Rust `1.91` or newer (`rust-toolchain.toml` pins `1.99.1` for repository development). Build using the repository's `Cargo.lock`:

```bash
git clone [https://github.com/seek-hope/bioinfo-tools.git](https://github.com/seek-hope/bioinfo-tools.git)
cd bioinfo-tools
cargo build --release --workspace --bins --locked
export PATH="$PWD/target/release:$PATH"
```

### Prebuilt Binaries

Prebuilt binary distributions currently include `rust-cutadapt` and `rust-umitools`. See [dist/README.md](dist/README.md) and [seek-hope/bioinfo-tools-dist](https://github.com/seek-hope/bioinfo-tools-dist) for installation details. The remaining CLIs are built from source.

### Usage Examples

#### Sequence Statistics, Validation, and FASTA Export (`rust-fx`)

```bash
rust-fx stats -a -T reads.fastq.gz
rust-fx lint reads.fastq.gz
rust-fx fq2fa -w 0 reads.fastq.gz -o reads.fasta
```

#### Adapter Trimming and Post-Trimming QC (`rust-cutadapt`, `rust-fastqc`)

Set the adapter sequence and length threshold according to your library design:

```bash
rust-cutadapt -a AGATCGGAAGAGC -m 30 -j 4 \
  -o trimmed.fastq.gz reads.fastq.gz
mkdir -p qc
rust-fastqc --extract -o qc trimmed.fastq.gz
```

#### UMI Extraction and Adapter Trimming (`rust-umitools`, `rust-cutadapt`)

This example moves the first 8 bases of each read into the read name as the UMI and then trims adapters (adjust the UMI pattern and adapter sequence for your library):

```bash
rust-umitools extract -p NNNNNNNN -I reads.fastq.gz -S umi.fastq.gz
rust-cutadapt -a AGATCGGAAGAGC -m 30 -j 4 \
  -o umi.trimmed.fastq.gz umi.fastq.gz
```

#### BAM Sorting, Indexing, and UMI Deduplication (`rust-bam`, `rust-umitools`)

After aligning `umi.trimmed.fastq.gz` into `aligned.bam` with an external aligner that preserves read names, sort, index, check alignment statistics, and deduplicate:

```bash
rust-bam sort -o sorted.bam aligned.bam
rust-bam index sorted.bam
rust-bam flagstat sorted.bam
rust-umitools dedup --random-seed 1 --compresslevel 6 \
  -I sorted.bam -S dedup.bam
```

This `dedup` command reads UMIs from the read name. For BAM files where the UMI is stored in the `UB` tag, pass `--extract-umi-method=tag --umi-tag=UB`.

### Development Checks

Run the repository verification scripts:

```bash
bash tests/ci/check.sh
bash tests/ci/check.sh --with-fastx-qc-oracles
```

The second command runs checks against locally installed, pinned versions of the reference tools. See [tests/ci/README.md](tests/ci/README.md) for details.

## Performance Comparison with Reference Software

A new round of single-core and multi-core benchmarks on large inputs is still running. Previous benchmark records and optimization notes are available in [bench/RESULTS-README.md](bench/RESULTS-README.md) and [bench/OPTIMIZATION.md](bench/OPTIMIZATION.md), with benchmark methodology documented in [bench/README.md](bench/README.md).

| Tool / Reference | Operation | Input Size | Cores | Reference Time (s) | Current Implementation Time (s) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `rust-cutadapt` vs. cutadapt 4.6 | Adapter trimming (`-a -m`) | | 1 | | |
| `rust-cutadapt` vs. cutadapt 4.6 | Adapter trimming (`-a -m`) | | 4 | | |
| `rust-umitools` vs. UMI-tools 1.1.6 | `extract` | | 1 | | |
| `rust-umitools` vs. UMI-tools 1.1.6 | `dedup` | | 1 | | |
| `rust-bam` vs. samtools 1.24 | `sort` | | 1 | | |
| `rust-bam` vs. samtools 1.24 | `sort` | | 4 | | |
| `rust-bam` vs. samtools 1.24 | `index` | | 1 | | |
| `rust-bam` vs. samtools 1.24 | `flagstat` | | 1 | | |
| `rust-bam` vs. samtools 1.24 | `depth` / `coverage` | | 1 | | |
| `rust-fx` vs. SeqKit 2.14.0 | `stats -a -T` | | 1 | | |
| `rust-fx` vs. SeqKit 2.14.0 | `fq2fa` | | 1 | | |
| `rust-fx` vs. fq 0.12.0 | `lint` | | 1 | | |

### FastQC Runtime

Statistics verification and graphical report generation for `rust-fastqc` are still being checked; its runtime comparison is listed separately below.

| Tool / Reference | Operation | Input Size | Cores | Reference Time (s) | Current Implementation Time (s) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `rust-fastqc` vs. FastQC 0.13.0 | QC report | | 1 | | |
| `rust-fastqc` vs. FastQC 0.13.0 | QC report | | 4 | | |

## Development Roadmap

Near-term priorities:

- Complete subcommand, option, and file format support across the existing tools.
- Finish GC and duplication statistics verification and HTML graphical reports in `rust-fastqc`.
- Continue optimizing runtime hotspots and expand prebuilt binary distributions.

Future directions:

- **BED/GFF Interval Operations**: Genomic interval manipulation and overlap statistics.
- **Feature Counting and Expression Data**: Read-to-feature counting and expression matrix processing.
- **VCF/BCF Processing**: Variant querying, filtering, and summary statistics.

See [docs/DEVELOPMENT-PLAN.md](docs/DEVELOPMENT-PLAN.md) for the full development plan.
