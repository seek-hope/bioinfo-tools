# Development plan: `bioinfo-tools` (historical)

> Archived when the revised [development plan](DEVELOPMENT-PLAN.md) was adopted.
> The original text below is retained for task IDs, sources and acceptance criteria;
> its status statements are historical, not a description of the current workspace.

Status: **draft for review** · Written: 2026-10-19 · All web sources accessed 2026-10-19.
Scope: this document only. No source file, manifest, tag, or release is changed by this plan.

This plan is deliberately conservative about claims. Every statement about the local tree was checked
by reading the file or running a command in this workspace; the commands are listed in the evidence
log below. Every statement about external tools cites a URL from `## Sources`; where a source could
not be retrieved, or where the claim comes from a project's own marketing, it is marked
**[unverified]** or **[vendor claim]**.
No benchmark in this plan is new: the only local numbers quoted are the ones already recorded in the
repository, and they are attributed to the commit/version that produced them.

The plan answers four questions:

1. What already exists here, and how trustworthy is it?
2. What does the practical bioinformatics tool landscape look like, layer by layer?
3. Where is a Rust implementation genuinely worth building, and what would prove it correct?
4. What should be done next, in what order, with which acceptance gates?

---

## Current state

### 1. What this directory is

`bioinfo-tools/` is the canonical Cargo workspace and monorepo. The two original crate
repositories were preserved as historical archives outside the tree; their source is now
tracked directly by this repository:

| Path | Kind | State |
| --- | --- | --- |
| `rust-cutadapt/` | Git repo, Rust crate `rust-cutadapt` 0.1.0 | Clean tree, HEAD `c337fc8` (2026-10-19), **no tags** |
| `rust-umitools/` | Git repo, Rust crate `rust-umitools` 0.2.0 | Clean tree, HEAD `ceb7c23` = `v0.2.0-4-gceb7c23` |
| `.work/` | Scratch: stale copy of `rust-cutadapt`, oracle venv, `dist` checkout | Not part of either crate |
| `.leader-verify/` | Acceptance harness for the historical cutadapt trial | Read-only reference |
| `AGENTS.md`, `TASK.md` | Trial rules and the original cutadapt task | Binding for that trial |
| `docs/` | Created by this plan | Sole deliverable of this task |

`AGENTS.md` states the original trial constraints: the target was `rust-cutadapt/`; `rust-umitools/`
must not be modified; **"Do not publish a release and do not commit inside `rust-umitools`"**; builds
must use `--offline`; fixtures and expected FASTQ files must not be edited; and dependencies were
limited to the standard library plus `flate2` and `clap`. That last constraint has since been
superseded in the tree itself: `rust-cutadapt/Cargo.toml` now also depends on `bzip2 0.6.1` and
`liblzma 0.3.6`, and the code implements bzip2/xz I/O. The plan records this as governance debt
(see `## Risks and decisions`) rather than silently accepting it.

The **release freeze** referenced by this task is therefore read as: no new Git tags, no version
bumps, no dist-repository uploads, and no crates.io/Bioconda publication while this trial is open.
Everything release-related in this plan is preparation and a gated checklist, not an action.

### 2. `rust-cutadapt` — verified state

| Item | Value | How verified |
| --- | --- | --- |
| Version / edition / MSRV | `0.1.0`, edition 2021, `rust-version = "1.88"` | `Cargo.toml` |
| Source size | 11,335 lines across `src/**/*.rs` (largest: `pipeline.rs` 1,482; `cli.rs` 1,193; `report/text.rs` 1,045) | `wc -l` |
| CLI surface | 60 options in `--help` | `--help` + count |
| Unit/integration tests | 208 passing: 171 lib + 14 `cli` + 21 `cli_e2e` + 1 `differential` + 1 doc-test | `cargo test --release --offline` |
| Differential suite | **119/119 cases pass** against a real cutadapt **4.6** binary | `python3 tests/oracle/differential.py --matrix` |
| Oracle binary used | `/home/rimuru/.conda/envs/ta-cutadapt/bin/cutadapt`, reports `4.6` | `cutadapt --version`, harness discovery |
| Implemented surface | Full documented cutadapt 4.6 option set: adapter types `-a/-g/-b` incl. anchored/linked, `file:` lists, indels, `--times`, read modifications, filters, `--action`, `--revcomp`, renaming, auxiliary outputs, FASTA, paired-end/interleaved, text + JSON report, demultiplexing, gzip/bzip2/xz | `docs/FULL-COMPATIBILITY.md`, probes |
| Known deviations | compressed bytes differ (level 3 default vs cutadapt 6); extra "output file is incomplete" note; report "expect" column compared with tolerance; `-j` output deterministic | `docs/FULL-COMPATIBILITY.md` |
| Recorded performance | cutadapt 4.6 baseline 4.4363 s / 24,412 kB on 1,000,000 reads; this crate 0.64–0.70 s, 11.6–13.4 MB peak RSS; 6.3x–6.9x across six hyperfine sessions | `bench/reference.json`, `BENCHMARK.md` |
| Release artifact | Dist repo `seek-hope/bioinfo-tools-dist` is the consolidated public repository; each release contains both `rust-cutadapt` and `rust-umitools` | Dist releases page; local Git |
| Artifact staleness | `d9c4c52..HEAD` = **47 commits**, including the entire full-compatibility effort | `git rev-list --count d9c4c52..HEAD` |
| Platform | Linux x86_64 glibc only; no aarch64/macOS artifact | Dist README / release notes |

Two behavioural probes I ran are worth recording because they contradict the current README:

* `-q 20` on a read whose qualities are all `!` emitted an **empty sequence record** (`@r2` followed
  by an empty line) and exited 0 — cutadapt's documented quality-trimming behaviour.
* `-g ACGT` trimmed a 5' `ACGT` (12 bp → 8 bp), and `-u 4` removed 4 leading bases, both exit 0.

Yet `rust-cutadapt/README.md` still says the crate supports "one 3' adapter, single-end" and that
`-g`, `-q`, paired-end, `-j`, `--json`, `-Z`, wildcards etc. are "rejected with exit code 2".
Probes show `-g`, `-q`, `-A`, `-N`, `--poly-a`, `-j 4`, and `--json` all exit 0 and work. The README,
the `Unsupported behaviour` section, and the "Layout" table describe the **v0.1.0 subset**, not the
current binary. `docs/FULL-COMPATIBILITY.md` is the accurate document. This is the single largest
documentation-truth risk in the repository.

Similarly, `bench/reference.json` and the published dist binary predate the full-surface work; the
6.3x–6.9x figure is valid for the recorded build/command on that 1M-read workload but has not been
re-measured on the current `c337fc8` binary, and `BENCHMARK.md` itself notes that later mimalloc/
compression changes (those are `rust-umitools` changes) were not re-measured on that input. Treat the
number as "historical baseline for the v0.1.0 code path", not as a property of the current tree.

### 3. `rust-umitools` — verified state

| Item | Value | How verified |
| --- | --- | --- |
| Version | `0.2.0`; tag `v0.2.0` exists; HEAD is 4 commits past it | `Cargo.toml`, `git describe` |
| Source size | 8,514 lines across `src/**/*.rs` (largest: `cli.rs` 906; `cluster.rs` 705; `bundle.rs` 625) | `wc -l` |
| Subcommands | `whitelist`, `extract`, `dedup`, `group`, `count`, `count_tab` | `--help` |
| Dependencies | `clap 4`, `rust-htslib 0.49` (bzip2/lzma/libdeflate), `rayon`, `rustc-hash`, `regex`, `flate2`, `mimalloc` | `Cargo.toml` |
| Unit tests | 41 `#[test]` functions | `grep` |
| Semantic oracle suite | **39 passed, 0 failed** against UMI-tools **1.1.6** | `bash tests/oracle/run_oracle.sh` |
| Pinned oracle environment | umi_tools 1.1.6, samtools 1.24, NumPy 2.5.3, pysam 0.24.1, Python 3.12.14 | harness prints this; `tests/oracle/versions.py` |
| Byte-exact suite | 77 cases claimed by README/`docs/COMPATIBILITY.md` | **Not re-run in this session** (see below) |
| Compatibility matrix | `docs/compatibility.tsv`, 50 capability rows, `verified`/`experimental`/`unsupported` | `wc -l`, file head |
| Recorded performance | 4,428,466-read `small.bam`: UMI-tools 1.1.6 143.08 s / 1.10 GiB; this crate 9.24 s / 0.78 GiB; 15.49x; same molecule count 3,718,913 | `docs/BENCHMARK.md` |
| Known limits | `--output-stats` rejected (null model absent); `--gene-transcript-map` only with `count --per-contig`; CRAM may add derived `MD/NM`; compressed BAM bytes differ; default `--compresslevel 1`; seed defaults to 0 where upstream is non-deterministic | `README.en.md`, `docs/COMPATIBILITY.md` |

Why the byte-exact suite was not re-run: `tests/oracle/run_exact.sh` falls back to a hard-coded
personal path (`/home/rimuru/Projects/Code/homework/AI4Biomedicine/dedup/envs/dedup/bin`). The
semantic suite resolved that oracle and passed; the exact suite would also rebuild the crate and run
77 cases, which is legitimate but was not necessary for this planning pass. **The 77/77 claim is
therefore documented-but-not-independently-reconfirmed here.** Mark it **[unverified in this
session]** and make portability of the oracle path a P0 task.

The other structural risk: the semantic suite passed only because a personal conda environment was
discoverable through that absolute path. On a clean machine or in CI the harness would exit 2 with
"umi_tools not found". That is a reproducibility defect, not a correctness defect, and it is cheap to
fix.

### 4. What the two crates share today

Almost nothing, which is the central architectural finding of this review:

| Concern | `rust-cutadapt` | `rust-umitools` |
| --- | --- | --- |
| FASTQ parsing | Own strict parser in `src/fastq.rs` (238 lines) | Own parser in `src/io/fastq.rs` (191 lines) |
| Compression | `flate2`, `bzip2`, `liblzma`, custom multi-member gzip writer | `flate2` + rust-htslib BGZF |
| Alignment formats | None | rust-htslib (SAM/BAM/CRAM) |
| CLI framework | `clap 4` derive, hand-built 60-option grammar | `clap 4` derive, 906-line CLI |
| Oracle harness | Python `differential.py` (single file) | Python package `tests/oracle/*.py` (12 modules) + shell runners |
| Version pinning | Oracle version printed, not enforced | `versions.py` fails closed on mismatch |
| Benchmarks | `BENCHMARK.md` + `bench/reference.json` (cutadapt baseline) | `docs/BENCHMARK.md` + `docs/bench/*.json` (provenance-rich) |
| Packaging | Dist repo + release workflow; x86_64 glibc only | Dist repo + release workflow + `Containerfile`; x86_64 glibc only |
| Documentation language | English | Chinese primary, English README |

The two projects also use different definitions of "verified": cutadapt's harness compares 119 CLI
invocations including reports and JSON; UMI-tools' harness distinguishes `verified`, `experimental`,
and `unsupported` in a machine-readable matrix. The UMI-tools matrix is the better pattern and should
be adopted as the shared standard.

### 5. Workspace hygiene

* Stray files exist in `rust-cutadapt/`: `i.txt`, `r.json`, `r.txt`, `tl.fastq`, `ts.fastq`,
  `u.fastq`. `git status` is clean, so they are ignored or committed; either way they are scratch
  material from earlier sessions and should be removed or ignored explicitly.
* `.work/` contains a **second full copy** of the cutadapt repository (`.work/src`, 47-commit history
  matching the main tree), an oracle venv, and a `dist` checkout. It is not referenced by either
  crate's build; it is a stale duplicate that hides which tree is authoritative.
* Neither crate has a CI job that runs the *other* project's oracle, and there is no root-level
  command that checks both. There is no `docs/` index at the root before this file.
* `rust-umitools` has a `Containerfile`; `rust-cutadapt` has none.

### 6. Evidence log (commands actually run for this plan)

```text
git -C rust-cutadapt rev-parse --short HEAD              -> c337fc8
git -C rust-cutadapt status --porcelain | wc -l          -> 0
git -C rust-umitools describe --tags                     -> v0.2.0-4-gceb7c23
git -C rust-umitools status --porcelain | wc -l          -> 0
git -C rust-cutadapt rev-list --count d9c4c52..HEAD      -> 47
cargo test --release --offline  (rust-cutadapt)          -> 208 passed, 0 failed
python3 tests/oracle/differential.py --matrix            -> 119/119 cases passed
bash tests/oracle/run_oracle.sh (rust-umitools)          -> 39 passed, 0 failed
rust-cutadapt --help | grep -oE '^\s+-[-a-zA-Z0-9, ]+' | wc -l  -> 60 option lines
rust-cutadapt -q 20 / -g ACGT / -u 4 probes              -> exit 0, documented cutadapt behaviour
which samtools ; which bcftools                          -> not found on PATH
```

Not run: `run_exact.sh` (77 cases), `run_spill.sh`, `run_determinism.sh`, the benchmark suites, and
any clippy/fmt gate. Those are milestone entry criteria, not evidence claimed here.

---

## Tool landscape

The landscape below is organized by pipeline layer. For each layer the table records the tools that
are actually used in practice, the interchange format that constrains interoperability, what already
exists in Rust, and what a compatibility oracle would have to be. Version numbers are the latest
observed on the cited page on 2026-10-19; they are not a guarantee of the latest release.

### 1. Read/FASTQ/FASTA and compression

FASTQ has **no formal specification**; the HTS specifications explicitly say it "has no formal
definition and several incompatible variants" and point to Cock et al. [S8]. FASTA likewise predates
any standards body. This matters for a Rust reimplementation: "FASTQ compatibility" must be defined
against a concrete tool's parser, not against a standard. In practice the de-facto contract is
4-line records, `@`/`+` separators, sequence and quality of equal length, optional multi-line FASTA,
and transparent gzip/BGZF/zstd/xz/bzip2 depending on tool.

| Tool | Version seen | Role | Interface notes | Rust prior art |
| --- | --- | --- | --- | --- |
| cutadapt [S1][S2] | changelog lists **5.2** (2025-10-23); this repo pins oracle **4.6** | Adapter/quality trimming, filtering, demux; MIT licence | Reads/writes FASTQ & FASTA, gzip/bzip2/xz, stdin/stdout, `-j` threads; adapter search parameters include `anywhere`, `rightmost`, `min_overlap` [S3] | `rust-cutadapt` (this repo) |
| SeqKit | **2.14.0** (2026-09-24) | 39 subcommands: `stats`, `seq`, `sample`, `split`, `fx2tab`, `faidx`, … | gzip/xz/zstd/bzip2/lz4 stdin/stdout; `stats` documented for MultiQC | none mature; `seq_io`/`needletail` are libraries only |
| fq (St. Jude) | **0.12.0** | FASTQ filter/sub-sample/lint | Rust, Bioconda, static binaries | itself Rust |
| fqtk (Fulcrum) [S53] | current | FASTQ demux/shard/subsample with read structures | Rust; Illumina header conventions; `--umi-tag RX` | itself Rust |
| fastp | **1.4.0** | All-in-one QC + trim + dedup + merge | C++; SIMD; parallel gzip; JSON+HTML report | none |
| Trimmomatic [S51] | **0.41** | Java trimmer with `ILLUMINACLIP`, sliding window | Requires Java 25 in 0.41; PE/SE; adapters FASTA | none |
| needletail | **0.7.3** | FASTX parsing + k-mers (library) | MIT, minimal-copy | itself Rust |
| noodles | **0.118.0** (2026-10-15) | SAM/BAM/CRAM/VCF/BCF/FASTA/FASTQ/BED/GFF/GTF (libraries) | MSRV **1.91**; API described as experimental | itself Rust |

Compression is where FASTQ tools quietly disagree. cutadapt 5.0 made gzip level 1 the default and
deprecated `-Z` [S2]; `rust-cutadapt` ships level 3 to hold its 5x target; `rust-umitools` ships
BGZF level 1. Any byte-level contract must therefore be stated on **decompressed** bytes. Multi-member
gzip (one member per chunk) is valid and decodes transparently, but it is not byte-identical to a
single-member stream — a fact both projects already document.

### 2. QC and trimming/filtering

| Tool | Version seen | What it produces | Oracle strategy |
| --- | --- | --- | --- |
| FastQC | **0.13.0** (latest release listed); project changelog page still shows 0.12.0 and appears stale [S17][S18] | HTML + `fastqc_data.txt` + zip per file; modules for per-base quality, GC, N content, duplication, overrepresented sequences, adapter content | Module-by-module numeric comparison against `fastqc_data.txt`; the HTML/plots need structural comparison only |
| sequali | current | HTML+JSON-like report; ~2 GB / 3–30 min on 2 cores **[vendor claim]**; MultiQC-supported | Report field comparison; has no long history to pin |
| MultiQC [S20] | **1.35** | Aggregates 150+ tools into one report | Use as a **consumer**: if our output parses in MultiQC, that is a strong interop test |
| fastp | 1.4.0 | QC + trimming + HTML/JSON in one pass | Compare JSON keys/values with tolerance on floats |
| Trimmomatic | 0.41 | Trimmed FASTQ + log | Byte comparison of read outputs; log numbers with tolerance |

Two facts shape the opportunity. First, **FastQC is GPLv3** [S17]. A Rust reimplementation must be
clean-room (behaviour and output fields, not code), and should be a separate MIT/Apache crate whose
compatibility is demonstrated by differential tests. Second, the ecosystem already consumes FastQC-
shaped output: MultiQC parses FastQC, and `seqkit stats` documents MultiQC usage [S21]. A fast Rust
QC tool that emits FastQC-compatible data files is therefore immediately useful in existing
pipelines, without asking anyone to change their reporting stack.

### 3. Alignment and mapping

| Tool | Version seen | Algorithm / niche | Notes relevant to a Rust plan |
| --- | --- | --- | --- |
| BWA-MEM / BWA-MEM2 | bwa-mem2 **2.2.1** | BWT + SMEM seeding for short reads | Authors claim identical output to bwa 0.7.17 and ~1.3–3.1x faster **[vendor claim]** [S22] |
| minimap2 | **2.31** | Minimizer seed-chain-align; long reads, splice, asm, short reads | Presets (`map-ont`, `map-hifi`, `sr`, `splice`, `splice:sr`); SIMD-heavy; x86-64/ARM builds [S23] |
| STAR | **2.7.11b** (Jan 2024) | Spliced alignment for RNA-seq; STARsolo | 16 GB RAM minimum for mammal genomes, 32 GB ideal **[vendor claim]** [S24] |
| HISAT2 | current | Hierarchical graph FM index; graph genomes | Human+SNP index 6.2 GB, footprint 6.7 GB **[vendor claim]** [S25] |
| Bowtie2 | current | BWT short-read aligner | Manual page not retrieved in this session — **[unverified]** |
| BBMap/BBTools | current | Java short/long-read aligner + suite | Pure Java, ~3 MB download; source is SourceForge [S26] |
| piscem | current | Rust de Bruijn graph mapper for bulk/scRNA/scATAC | Proves a pure-Rust mapper is viable; not a BWA drop-in [S37] |

An aligner is the one layer where a from-scratch Rust rewrite is **not** a sensible early bet. The
incumbents are heavily SIMD-optimized C/C++, their output depends on seeding and tie-breaking
details that are poorly specified, and "compatible" would have to mean "identical SAM for the same
reads", which is a multi-year effort. The plan therefore chooses integration (call BWA/minimap2) or
narrow, well-specified sub-problems (e.g. a suffix-array-free exact-match counter) rather than an
aligner.

### 4. SAM/BAM/CRAM processing

This is the most standardized layer and the one with the strongest compatibility story.

| Component | Version seen | Role | Licence / notes |
| --- | --- | --- | --- |
| htslib | **1.24** (9 July 2026) | C library for SAM/BAM/CRAM/VCF/BCF | MIT; samtools/bcftools bundle their own copies [S7][S9] |
| samtools | **1.24** | `view`, `sort`, `index`, `markdup`, `flagstat`, `stats`, `depth`, `faidx`, `fastq`, `fixmate`, … | Manual page is authoritative [S4][S5] |
| bcftools | **1.24** | `mpileup`, `call`, `norm`, `filter`, `query`, `annotate`, `merge`, `isec`, … | [S6] |
| Specifications | SAMv1, CRAMv3, CRAMcodecs, SAMtags, VCFv4.5, CSIv1, tabix | Canonical format definitions | GA4GH-maintained hts-specs repo [S8] |
| sambamba | **1.0** | D-language `view/index/sort/markdup/depth` | GPLv2+; **CRAM support removed in 0.8** [S27] |
| noodles | **0.118.0** | Pure-Rust BAM/BCF/BED/BGZF/CRAM 3.0/3.1/CSI/FASTA/FASTQ/GFF/GTF/SAM/tabix/VCF | MSRV 1.91; API experimental [S40] |
| rust-htslib | crates.io **1.0.1**; repo changelog mentions 1.0.2 (2026-08-16) | Safe Rust bindings over htslib | MIT; needs C toolchain/clang [S41] |

Security is part of the compatibility story. The samtools 1.23.1 announcement lists a batch of CRAM
decoder and GZI-index CVEs (CVE-2026-31962 through CVE-2026-31971) fixed in that release and back-
ported to 1.21.1/1.22.2 [S10]. Any tool that parses untrusted BAM/CRAM must pin a patched htslib or
use a Rust parser with an explicit fuzzing story. This is a concrete argument for the pure-Rust path
(noodles) or at minimum for a documented minimum htslib version.

`samtools markdup` is also the reference for duplicate marking: it requires coordinate-sorted input
that has been through `fixmate -m`, uses position/orientation, keeps the highest-base-quality read,
and since 1.24 can move a UMI from QNAME into the `RX` tag [S5]. Picard `MarkDuplicates` uses the
same idea but ranks by summed base qualities and supports `BARCODE_TAG`, `DT` tags, and optical
duplicate detection [S28].

### 5. Variant calling and annotation

| Tool | Version seen | Role | Rewrite suitability |
| --- | --- | --- | --- |
| bcftools `mpileup` + `call` | 1.24 | Lightweight Bayesian SNP/indel calling and VCF manipulation | Medium for manipulation, low for calling |
| GATK | **4.7.0.0** | HaplotypeCaller, Mutect2, joint calling, CRAM 3.1 | Very low: JVM, HTSJDK 5.0.0, huge behavioural surface [S29] |
| DeepVariant | **1.10.0** | CNN-based germline caller | Very low: TensorFlow + trained models [S30] |
| freebayes [S54] | current | Haplotype-based Bayesian caller | Low: statistical parity |
| Ensembl VEP | **release 116** | Consequence annotation, frequencies, predictors | Medium for a narrow "consequence" subset, high for full parity [S31] |
| SnpEff | current | Functional annotation | Documentation not retrieved — **[unverified]** |

Variant calling is a **non-goal** for this repository. The interesting, tractable sub-problems are
VCF mechanics: `norm`-style left-alignment and splitting, `filter` expressions, `query` formatting,
`annotate` tag transfer, `isec`/`merge`, and a fast VCF/BCF reader. Those have crisp, testable
semantics and are dominated by I/O rather than statistics.

### 6. Expression quantification (bulk)

| Tool | Version seen | Method | Notes |
| --- | --- | --- | --- |
| salmon | **2.0 is a from-scratch Rust rewrite**; latest release **2.8.0** | Selective alignment or sketch mode + EM/VBEM over equivalence classes | Index format changed at 2.0 (rebuild required); `salmon alevin` removed; single-cell moved to alevin-fry ecosystem [S32] |
| kallisto | current | Pseudoalignment + EM | C++; BSD-2; paired with `bustools` for single-cell [S33] |
| featureCounts (subread) | current | Read summarization over GTF/BED | Documentation page not retrieved in this session — **[unverified]** |
| htseq-count | current | Union/intersection-strict/intersection-nonempty modes; `--nonunique none/all/fraction/random` | Python; precise documented counting semantics [S34] |
| tximport | 1.40.0 (Bioconductor 3.23) | Transcript → gene summarization, length offsets | Downstream consumer of `quant.sf` [S35] |

The existence of salmon 2.x — a full Rust rewrite that kept the workflow and output formats — is the
strongest evidence in the whole landscape that **Rust rewrites of numerically heavy tools are viable
when the output contract is explicit**. It is also a warning: salmon had to break its index format at
2.0. Compatibility promises must be scoped to file formats, not internal indices.

### 7. Single-cell expression

| Tool | Version seen | Role | Notes |
| --- | --- | --- | --- |
| Cell Ranger | **10.1.0** | 10x end-to-end: demux, barcode counting, gene counting, V(D)J, feature barcodes | Source is "informational purposes" only; official binaries are the product [S38] |
| alevin-fry | current | RAD → permit list → gene×cell counts | Rust; successor to `salmon alevin`; recommends `piscem` backend [S36] |
| piscem | current | Rust cDBG index + mapper producing RAD | Pure Rust; x86-64 and ARM binaries [S37] |
| STARsolo | bundled with STAR 2.7.11b | Barcode/UMI counting inside STAR | Same STAR resource profile [S24] |
| UMI-tools | 1.1.6 | `whitelist`, `extract`, `count`, `count_tab` with `--per-gene/--per-cell/--wide` | Upstream now recommends alevin for droplet scRNA [S11] |

The single-cell space is already partly Rust (alevin-fry, piscem, and this repo's `rust-umitools`),
so the opportunity is **interoperability**, not another end-to-end pipeline: read/write Matrix Market,
respect barcode/feature ordering, and produce matrices that `tximport`/`fishpond`/`SingleCellExperiment`
consumers already accept.

### 8. UMI processing and deduplication

| Tool | Version seen | Scope | Oracle notes |
| --- | --- | --- | --- |
| UMI-tools [S11][S14][S15] | **1.1.6** (latest on PyPI [S15]) | `extract`, `whitelist`, `group`, `dedup`, `count`, `count_tab`; 5 grouping methods (`unique`, `percentile`, `cluster`, `adjacency`, `directional` default) | Deterministic with `--random-seed` since 1.1.6; tie-breaking changed vs earlier versions [S12][S13] |
| `rust-umitools` | 0.2.0 | Byte-identical decompressed BAM/TSV/FASTQ for the verified matrix; bounded-memory paths | 39 semantic + 77 byte-exact (claimed) [local] |
| fgbio | current | `GroupReadsByUmi`, `CallMolecularConsensusReads`, `CallDuplexConsensusReads`, UMI correction | JVM; consensus reads are a capability this repo lacks [S39] |
| samtools markdup | 1.24 | Position/orientation dedup, `--barcode-tag`, `--move-umi-to-tag` | Not UMI-aware in the network sense [S5] |
| Picard MarkDuplicates | current | Duplicate marking with `BARCODE_TAG`, optical duplicates | JVM; `DT` tag semantics [S28] |
| fastp | 1.4.0 | FASTQ-level dedup + UMI preprocessing | Fast but not a UMI-tools replacement [S16] |

The semantic core is documented precisely enough to test: `directional` edges require
`count(A) >= 2*count(B) - 1`; `adjacency` selects high-count nodes and assigns unvisited nodes to the
selected neighbour with the highest count; `cluster` takes connected components; ties are broken
randomly [S12]. This is exactly the kind of algorithm where a differential harness against a pinned
Python implementation is both feasible and decisive — which is what `rust-umitools` already does.

### 9. Workflow/orchestration and reproducibility

| System | Version seen | Model | Relevance |
| --- | --- | --- | --- |
| Snakemake | **9.27.0** | Python DSL, rule graph, conda/container deployment, reports | Most common in academia; wrappers exist for most tools [S42] |
| Nextflow | current docs (mentions Nextflow 26.10 for static typing) | Dataflow channels, executor abstraction (local/SLURM/cloud) | nf-core is the largest curated pipeline collection [S43][S44] |
| WDL | spec **1.3** current, **2.0** next | Task/workflow standard with multiple engines (MiniWDL, Sprocket, Toil, Cromwell) | Engine-agnostic; good for portable tool wrappers [S45] |
| Containers + Bioconda | — | Reproducible environments | Distribution channel users actually consume |

The practical requirement this imposes on any new CLI is not "be a workflow engine". It is: stable
exit codes, no interactive prompts, predictable output paths, `--threads`/`--memory` knobs that are
honoured, machine-readable logs, no timestamps in reports that break reproducibility (cutadapt 5.0
removed runtime from its report for exactly this reason [S2]; sequali advertises timestamp-free
reports [S19]).

### 10. Rust reimplementation opportunities, oracles, performance, packaging

**Prior art, and what it proves.**

| Project | What it proves | Caveat |
| --- | --- | --- |
| noodles (0.118.0) | Pure-Rust SAM/BAM/CRAM/VCF/BGZF is real and actively maintained | API experimental; MSRV 1.91; CRAM 3.1 still maturing [S40] |
| rust-htslib (1.0.1/1.0.2) | FFI route is stable and complete | C toolchain, htslib CVEs, thread-pool ownership [S41] |
| needletail (0.7.3) | Fast minimal-copy FASTX parsing | Parsing only, no CLI semantics |
| rust-bio (MSRV 1.87) | Algorithms/data structures (FM-index, alignment, interval trees) | Library, not compatibility-oriented [S47] |
| fq, fqtk | Rust FASTQ CLIs ship on Bioconda with static binaries | Narrow scope |
| salmon 2.x | Full Rust rewrite can preserve output formats | Broke index format at 2.0 [S32] |
| piscem/alevin-fry | Rust is viable for single-cell mapping + counting | Different file ecosystem (RAD) |
| Riker | Rust ports of Picard QC metrics claim 12–38x speedups | **[vendor claim]**, benchmark not reproduced here [S46] |

**Compatibility oracles.** The only defensible definition of "compatible" is a pinned original binary
plus a comparison rule. The two existing crates already demonstrate the pattern; the plan generalizes
it:

| Target | Pin | Comparison rule |
| --- | --- | --- |
| cutadapt | 4.6 binary; record Python + compiled-extension versions | Decompressed read bytes byte-identical; text report after normalizing banner/timing/cores/paths; JSON field-wise; exit codes equal |
| UMI-tools | 1.1.6 + NumPy 2.5.3 + pysam 0.24.1 + samtools 1.24 + Python 3.12.14 | Decompressed BAM header+records byte-identical; TSV byte-identical; stats files only if a null model exists |
| samtools | 1.24 | SAM text identical for `view`; BAM header/records identical; `flagstat`/`idxstats` numbers identical; sort order rules documented |
| bcftools | 1.24 | VCF field-wise, floats with tolerance, header line order normalized |
| FastQC | 0.13.0 | `fastqc_data.txt` module fields identical, HTML/plots structurally compared |
| BWA-MEM2 | 2.2.1 | Not an oracle for a rewrite; used as a fixture generator for SAM inputs |

**Performance.** The repositories already have the right instincts: hyperfine, `/usr/bin/time -v`,
CPU time, peak RSS, thread peak, temp-disk peak, input checksum, commit, binary checksum, and raw JSON.
The gap is that only `rust-umitools` records provenance systematically. A shared benchmark schema
should require: workload checksum, command, tool versions, commit, binary SHA256, hardware, warm/cold
cache, median and distribution, and a stated regression threshold. The honest rule from `rust-umitools`
deserves to be project policy: **no speedup claim without a correctness result on the same input.**

**Packaging.** Both projects publish stripped Linux x86_64 glibc tarballs plus SHA256 to a separate
dist repository, installed by a `curl | bash` script that verifies the checksum [local][S48][S49].
Neither publishes aarch64 or macOS artifacts. Neither is on Bioconda or crates.io. A `Containerfile`
exists only for `rust-umitools`. This is a coherent MVP distribution story and a weak production one:
HPC users are frequently aarch64, clusters prefer modules/conda over `curl | bash`, and the dist
repositories are separate sources of truth that can drift from the source repo — as they already have
for `rust-cutadapt` (v0.1.0 binary is 47 commits behind).

---

## Gaps and opportunities

Each gap below is stated with its evidence, the opportunity it creates, and a priority. Priority is
about **order of work**, not importance: P0 items make existing claims true, P1 items build the shared
foundation and the first new capability, P2 items expand only where an oracle is crisp.

### Gap register

| ID | Gap | Evidence | Opportunity | Priority |
| --- | --- | --- | --- | --- |
| G1 | `rust-cutadapt` README describes the v0.1.0 subset while the binary implements the full 4.6 surface | README says `-g`, `-q`, paired-end, `-j`, `--json` are "rejected with exit code 2"; probes show exit 0 and correct behaviour; `docs/FULL-COMPATIBILITY.md` says 119/119 | Repair the documentation; make the option table machine-checked | **P0** |
| G2 | Oracle environment is not portable | `run_exact.sh` hard-codes `/home/rimuru/Projects/Code/homework/AI4Biomedicine/dedup/envs/dedup/bin`; `run_oracle.sh` only works because a conda env happens to exist | Any third party can reproduce every claim with one command | **P0** |
| G3 | No shared foundation; two FASTQ parsers, two compression stacks, two CLI conventions | `rust-cutadapt/src/fastq.rs` vs `rust-umitools/src/io/fastq.rs`; different exit-code docs | Extract `bioinfo-fastx` + `bioinfo-common`; one exit-code and diagnostics contract | P1 |
| G4 | No general FASTX manipulation CLI | Neither crate offers `stats`, `seq`, `sample`, `split`, `fq2fa`, `fx2tab`; users reach for SeqKit 2.14.0 or fq 0.12.0 | A Rust tool with a SeqKit-compatible subset and a pinned oracle | P1 |
| G5 | No read-QC layer | FastQC 0.13.0 is the de-facto standard and is GPLv3; sequali is the modern fast alternative | Clean-room FastQC-compatible `fastqc_data.txt` + MultiQC interop | P1 (prototype), P2 (breadth) |
| G6 | No general SAM/BAM CLI surface | `rust-umitools` only does UMI workflows; no `view`, `flagstat`, `idxstats`, `sort`, `index`, `quickcheck` | Build on noodles 0.118.0 with samtools 1.24 as oracle | P1 |
| G7 | No VCF/BCF mechanics | No VCF code in either crate | `norm`, `filter`, `query`, `stats`, `isec` subset vs bcftools 1.24 | P2 |
| G8 | No feature counting | No GTF/BED interval counting; `rust-umitools count` is UMI/gene-tag based, not a general summarizer | htseq-count semantics are precisely documented; featureCounts is the performance target | P2 |
| G9 | Single-cell output interop is thin | `count --per-cell --wide` exists; no Matrix Market, no barcode/feature metadata contract | Sparse matrix writer/reader that downstream R/Python consumers accept | P2 |
| G10 | Distribution is x86_64-glibc only and drifts from source | Dist has cutadapt v0.1.0 built from `d9c4c52`, 47 commits behind HEAD; no aarch64; no crates.io/Bioconda; no container for cutadapt | Reproducible build + multi-arch + conda recipe draft, **held** under the freeze | P1 (prep only) |
| G11 | Benchmarks are stale relative to current binaries | cutadapt 6.3x–6.9x was measured on the v0.1.0 code path; `rust-umitools` re-measured several times but not on `ceb7c23` | One provenance-complete benchmark schema; re-measure on the commit being claimed | P0/P1 |
| G12 | Workspace hygiene and governance | `.work/src` is a full duplicate repo; stray files `i.txt`, `r.json`, `r.txt`, `tl.fastq`, `ts.fastq`, `u.fastq`; `AGENTS.md` dependency rule is contradicted by `Cargo.toml` (bzip2, liblzma) | One authoritative tree; ADR recording the supersession; explicit ignore rules | **P0** |

### What not to build

Explicit non-goals, with reasons, so that the roadmap is not read as an invitation:

| Non-goal | Reason |
| --- | --- |
| A short-read aligner (BWA-MEM2/STAR class) | Seeding and tie-breaking are underspecified; parity means identical SAM, a multi-year effort; incumbents are SIMD-tuned C/C++ [S22][S23][S24] |
| A variant caller (GATK/DeepVariant/freebayes class) | Statistical and model parity is not testable to byte level; GATK 4.7.0.0 carries HTSJDK 5.0.0 and a huge surface [S29][S30] |
| A workflow engine (Snakemake/Nextflow/WDL) | Mature ecosystems already exist; the value is in tools that behave well *inside* them [S42][S43][S45] |
| A GUI, GPU path, or distributed execution | No profiling evidence that CPU time is dominated by anything a GPU helps; `rust-umitools` already documents deferral for the same reason |
| Byte-identical *compressed* BAM/gzip output | Deflate/CRAM encoder choices legitimately differ; the contract is decompressed content [S8] |
| Re-implementing htslib's C API in Rust for compatibility's own sake | noodles already exists; use it where it suffices, keep rust-htslib where it does not [S40][S41] |

### Recommended crate and CLI boundaries

The recommendation is a **Cargo workspace with one binary per upstream contract** and two shared
libraries. It is deliberately not a single mega-binary: `cutadapt`, `umi_tools`, `samtools`, and
`bcftools` have incompatible option grammars, exit-code expectations, and release cadences. Merging
them would make compatibility harder, not easier.

| Crate | Kind | Responsibility | May depend on | Oracle |
| --- | --- | --- | --- | --- |
| `bioinfo-common` | lib | Exit-code classes (0/1/2/3/4), diagnostic formatting, `--summary-json` schema, provenance block, thread/memory budget helpers | `serde`, `serde_json` only | none (unit tests) |
| `bioinfo-fastx` | lib | Streaming FASTQ/FASTA reader/writer, strict and lenient modes, compression detection (gzip/BGZF/bzip2/xz/zstd), multi-member gzip writer, byte-level round-trip guarantees | `flate2`, `bzip2`, `liblzma`, `zstd` | golden fixtures + fuzz corpus |
| `rust-cutadapt` | bin + lib (exists) | cutadapt 4.6 contract | `bioinfo-fastx`, `bioinfo-common` | cutadapt 4.6 |
| `rust-umitools` | bin + lib (exists) | UMI-tools 1.1.6 contract | keep rust-htslib; optionally `bioinfo-fastx` for `extract`/`whitelist` | UMI-tools 1.1.6 |
| `rust-fx` | bin | FASTX manipulation: `stats`, `seq`, `sample`, `split`, `fq2fa`, `fx2tab`, `lint` | `bioinfo-fastx` | SeqKit 2.14.0, fq 0.12.0 |
| `rust-fastqc` | bin | Clean-room FastQC-compatible QC: `fastqc_data.txt`, zip, later HTML | `bioinfo-fastx` | FastQC 0.13.0 |
| `rust-bam` | bin | `view`, `flagstat`, `idxstats`, `sort`, `index`, `quickcheck`, `depth` | noodles 0.118.0 (`bam`, `sam`, `bgzf`, `csi`, `cram`), `bioinfo-common` | samtools 1.24 |
| `rust-vcf` | bin | `norm`, `filter`, `query`, `stats`, `isec` subset | noodles 0.118.0 (`vcf`, `bcf`, `tabix`, `csi`) | bcftools 1.24 |
| `rust-featurecount` | bin | GTF/BED summarization with htseq-count overlap modes | `bioinfo-fastx`/noodles GFF/GTF | htseq-count, featureCounts |

**Boundary rules** (these are the decisions that keep the workspace coherent):

1. **One upstream contract per binary.** A binary may *read* formats from many tools; it may only
   *promise compatibility* with one.
2. **Libraries never exit and never print.** `bioinfo-*` crates return `Result<T, Error>` where `Error`
   carries an exit class; only binaries call `std::process::exit` and write to stderr.
3. **Shared exit-code contract:** `0` success, `1` runtime/data error, `2` usage or unsupported
   capability (rejected before any output is created), `3` input precondition violated (unsorted,
   mixed UMI lengths), `4` I/O or resource exhaustion. This already matches `rust-umitools`.
4. **Shared diagnostic surface:** every binary accepts `--quiet`, `--progress auto|on|off`,
   `--threads N`, `--summary-json FILE`, and `--version --verbose`. stdout carries data only.
5. **Upstream option names win.** Where a flag means the same thing, reuse the upstream spelling.
   Divergent extensions must be documented and must not change default output.
6. **No silent fallback.** An unimplemented option is rejected with exit 2 *before* outputs are
   opened. This is already the strongest behavioural property of both existing crates and must not
   regress.
7. **Decompressed bytes are the contract** for FASTQ/BAM; compressed bytes may differ, and any
   difference must be documented with the reason.
8. **`unsafe` requires a written justification** in the module header and a fuzz target.

**Naming.** Keep the `rust-` prefix for crate and binary names to match the existing two crates.
`rust-fx` is chosen over `fq` to avoid colliding with the St. Jude binary [S52]; if a user-facing
alias is ever wanted it can be an opt-in install-time symlink, not a second executable.

---

## Prioritized roadmap

Three waves. Each item has an ID, a size (S ≤ 2 days, M ≤ 1 week, L > 1 week for one experienced
Rust developer), explicit dependencies, and an acceptance check that a reviewer can run. Nothing in
Wave 0 or Wave 1 requires a release.

### Wave 0 — make the existing claims true (P0, ~1 week)

| ID | Task | Size | Depends on | Acceptance check |
| --- | --- | --- | --- | --- |
| **W0-1** | Replace `rust-cutadapt/README.md` with a description of the current binary. Move the v0.1.0 subset text into `docs/HISTORY.md`. Keep `docs/FULL-COMPATIBILITY.md` as the source of truth. | S | — | Every option described as "unsupported" actually exits 2; every option described as implemented exits 0 on a smoke input; the option count in README equals `--help` (60) and the table in `FULL-COMPATIBILITY.md` |
| **W0-2** | Make the oracle portable. Resolve the oracle via `--oracle`, `$CUTADAPT_ORACLE`/`$UMI_TOOLS_ORACLE_PATH`, then a `pixi`/`conda` env declared in `tests/ci/env.yml`, then `PATH`; fail closed with exit 3. Remove the personal absolute path from `run_exact.sh`. | M | — | In a fresh container built from the repo, `bash tests/oracle/run_oracle.sh` → `39 passed, 0 failed` and `bash tests/oracle/run_exact.sh` → `77` cases pass, with no `/home/...` path in the repo |
| **W0-3** | Record current-state evidence: run both suites on the exact commits, capture commit, binary SHA256, tool versions, commands, and results into `docs/EVIDENCE-2026-10.md` + JSON. | S | W0-2 | JSON validates against the schema in `## Milestones and acceptance`; no claim without a command |
| **W0-4** | Governance cleanup: ADR `docs/adr/0001-dependency-policy.md` recording that `AGENTS.md`'s `flate2`+`clap`-only rule was superseded by bzip2/xz support; delete or archive `.work/src`; add ignore rules for the stray files; document the release freeze. | S | — | `git status` clean in both repos; `.work/` contains no duplicate source tree; ADR merged; no tag created |
| **W0-5** | Re-benchmark `rust-cutadapt` at HEAD with the documented command and full provenance; replace or annotate the 6.3x–6.9x claim. | M | W0-1 | `docs/bench/cutadapt-<commit>.json` with input SHA256, binary SHA256, hyperfine JSON, RSS; README quotes that file only |

**Why this wave first.** Every later wave depends on the workspace being trustworthy. A team that
builds a new crate on top of a README that misdescribes the binary will reproduce the same defect at
a larger scale.

### Wave 1 — shared foundation and the first new layer (P1, ~4 weeks)

| ID | Task | Size | Depends on | Acceptance check |
| --- | --- | --- | --- | --- |
| **W1-1** | Create the workspace and `bioinfo-common`. Move exit-code classes, diagnostics, summary-JSON, and provenance out of both binaries. Behaviour-neutral. | M | W0 | Both oracles unchanged; no output byte changes; `cargo test --workspace` green |
| **W1-2** | Create `bioinfo-fastx`; migrate `rust-cutadapt` first, then `rust-umitools` FASTQ paths. Include compression detection, multi-member gzip, BGZF, and a lenient-vs-strict parser mode. | L | W1-1 | 119/119 and 39/39 still pass; decompressed outputs byte-identical; `cargo fuzz` corpus runs clean for 10 minutes per parser |
| **W1-3** | `rust-fx` with `stats`, `seq`, `fq2fa`, `fx2tab`, `sample --seed`, `split`, `lint`. | L | W1-2 | Differential vs SeqKit 2.14.0 and fq 0.12.0 on the agreed subcommand matrix; decompressed bytes identical; `sample` reproducible for a fixed seed |
| **W1-4** | `rust-bam` with `view`, `flagstat`, `idxstats`, `quickcheck`, `index`, `sort` on noodles. | L | W1-1 | SAM text identical to `samtools view`; BAM header+records identical after `samtools view -h`; `flagstat`/`idxstats` numbers identical; sort order documented and stable |
| **W1-5** | FastQC-compatible QC prototype: basic statistics, per-base quality, per-sequence quality, per-base N, per-sequence GC, length distribution; `fastqc_data.txt` only. | L | W1-2 | Module fields identical to FastQC 0.13.0 on ≥20 fixtures; MultiQC 1.35 parses the output without warnings; documented which modules are absent |
| **W1-6** | Packaging preparation, **no publication**: reproducible build (`--locked`, pinned toolchain), aarch64 cross-build check, `Containerfile` for cutadapt, conda recipe draft, SBOM. | M | W1-1 | Two builds from the same commit produce identical SHA256; container runs the smoke workflow; recipe lints but is not submitted |

**Sequencing note.** W1-1 and W1-2 block everything else, so they go first and are reviewed as
behaviour-neutral refactors. W1-3 and W1-4 can then proceed in parallel by two developers; W1-5
depends on W1-2 but not on W1-3/W1-4.

### Wave 2 — expand where the oracle is crisp (P2, ~1 quarter)

| ID | Task | Size | Depends on | Acceptance check |
| --- | --- | --- | --- | --- |
| **W2-1** | `rust-vcf`: `norm` (left-align, split multiallelics), `filter` (subset of `-i`/`-e` expressions), `query`, `stats`, `isec`. | L | W1-1 | VCF field-wise equality with bcftools 1.24 after header normalization; floats within a documented tolerance; header line order normalized |
| **W2-2** | `rust-featurecount`: GTF/BED summarization with `union`, `intersection-strict`, `intersection-nonempty`, and the htseq-count `--nonunique` modes (`none`, `all`, `fraction`). | L | W1-4 | Counts identical to htseq-count on a fixture matrix; per-read assignment reasons match; performance compared with featureCounts on the same BAM |
| **W2-3** | Single-cell interop: sparse Matrix Market writer/reader, barcode and feature ordering contract, `--summary-json`; validate with a documented R/Python reader. | M | existing `rust-umitools` | Round-trip matrix equality; dimension/ordering invariants tested; consumer smoke test documented with versions |
| **W2-4** | `rust-umitools`: implement the `--output-stats` null model **with** a pinned-oracle byte comparison, or keep it rejected. No third state. | L | W0-2 | Either byte-identical stats files or exit 2 with a clear message; no placeholder values can be emitted |
| **W2-5** | Advanced UMI: consensus/duplex reads (fgbio semantics) as a **separate** command with its own oracle. | L | W2-4 | fgbio `GroupReadsByUmi`/`CallMolecularConsensusReads` comparison on fixtures; documented as an extension, not as UMI-tools parity |
| **W2-6** | Benchmark unification: one schema, one runner, regression thresholds >10% on a dedicated runner. | M | W0-5 | `docs/bench/schema.json`; every published number links to a raw result file |

### Dependency graph (text form)

```text
W0-2 ──► W0-3
W0-1 ──► W0-5
W0-4
   └──► W1-1 ──► W1-2 ──► W1-3
            │        └──► W1-5
            ├──► W1-4 ──► W2-2
            └──► W1-6
W1-1 ──► W2-1
rust-umitools ──► W2-3, W2-4 ──► W2-5
W0-5 ──► W2-6
```

### Priority rationale

* **Correctness of claims before new code.** W0 items cost days and remove the largest credibility
  risk in the repository.
* **Shared parsing before more parsers.** Three new binaries each with their own FASTQ parser would
  triple the fuzzing surface; one library with one fuzz target is cheaper and safer.
* **BAM before VCF.** BAM has the crispest oracle (text SAM comparison), the largest user base, and
  noodles already covers the formats [S40].
* **QC before feature counting.** FastQC-compatible output is immediately consumable by MultiQC, and
  the module-level oracle is numeric and stable.
* **Aligners and callers never.** Documented as non-goals.

---

## Milestones and acceptance

### Near-term milestones (first 10 working days)

| Milestone | Day | Deliverable | Exit criteria (all must hold) |
| --- | --- | --- | --- |
| **M0 — Truth** | 1–2 | W0-1, W0-4 | README matches probes; ADR merged; `.work/src` gone; freeze documented; both repos `git status` clean |
| **M1 — Reproducible oracle** | 3–4 | W0-2, W0-3 | Fresh container: 39/39 and 77/77 pass with no personal paths; evidence JSON committed |
| **M2 — Honest numbers** | 5 | W0-5 | New benchmark JSON at HEAD with provenance; README cites it; old number marked historical or removed |
| **M3 — Workspace** | 6–8 | W1-1 | Workspace builds; both oracles unchanged; exit-code and summary-JSON contract documented and tested |
| **M4 — Shared FASTX** | 8–10 | W1-2 (first half) | `rust-cutadapt` runs on `bioinfo-fastx`; 119/119 still pass; fuzz target exists and runs in CI |

### Milestone gates (reviewer checklist)

A milestone is **not** met until a reviewer other than the author has run the checks. The gate is a
single script per milestone, `tests/gate/m<N>.sh`, which prints one PASS/FAIL line per criterion and
exits non-zero on any failure. Reviewers run the gate; they do not re-run every individual suite.

| Gate | Criteria |
| --- | --- |
| G-M0 | `README.md` option table count == `--help` count; every "unsupported" option exits 2; every "implemented" option exits 0; ADR present; `find .work -name '*.rs'` empty |
| G-M1 | clean-container oracle run; `grep -r '/home/' tests/` empty; `versions.py` fails on a wrong version |
| G-M2 | benchmark JSON has input/binary SHA256, commit, hardware, raw hyperfine file; README claim matches JSON median |
| G-M3 | `cargo test --workspace`; both oracles; exit-code table exercised by at least one test per code |
| G-M4 | 119/119; fuzz target runs ≥10 min; no per-record allocation regression measured by a counting allocator |

### Definition of done for any compatibility claim

A capability may be called `verified` only when **all** of these are recorded:

1. The upstream tool, version, and dependency versions are pinned and checked at runtime.
2. The fixture and its checksum are in the repository or generated deterministically by a seeded script.
3. The comparison rule is one of: byte-identical decompressed output, field-wise equality after a
   named normalization, or numeric equality within a stated tolerance.
4. The command that reproduces the result is in the test suite, not only in a document.
5. The result is recorded with commit and binary SHA256.

Anything less is `experimental`; anything not implemented is `unsupported` and must exit 2 before
writing output.

### Testing strategy against the original tools

The four existing harnesses already demonstrate the pattern; the plan standardizes them into tiers.

| Tier | Name | What it catches | Runtime | Required for |
| --- | --- | --- | --- | --- |
| **T0** | Unit + property tests | Parser edge cases, arithmetic, ordering | seconds | every commit |
| **T1** | Golden fixtures | Regressions on hand-chosen cases | seconds | every commit |
| **T2** | Differential oracle | Semantic divergence from the real tool | minutes | every PR |
| **T3** | Random differential + shrink | Unknown-unknowns; produces minimal reproducers | tens of minutes | nightly |
| **T4** | Fuzz + resource/determinism | Crashes, panics, OOM, non-determinism | hours | weekly / pre-release |

**Comparison levels** (pick the strongest that is meaningful):

| Level | Rule | Used for |
| --- | --- | --- |
| L0 | Exit code equal | every case |
| L1 | Decompressed bytes equal | FASTQ, SAM text, BAM records, TSV |
| L2 | Text equal after named normalization | reports (banner, timing, cores, paths) |
| L3 | Numeric equal within tolerance | floats in JSON, statistics |
| L4 | Semantic invariants | plots, HTML, matrices where ordering is not contractual |

**Oracle management rules:**

1. One pin file per upstream tool (`tests/oracle/versions.py` for UMI-tools; a `cutadapt_version.py`
   equivalent for cutadapt). Mismatch fails closed; an override env var exists for deliberate upgrades
   but CI must never set it.
2. Oracle environments are declared (conda/pixi YAML) and built in CI; no network at test time.
3. The oracle binary path is resolved, never hard-coded.
4. Every differential case records: case name, category, arguments, exit codes, first-difference
   offset, and the oracle version — so a failure is diagnosable without a rerun.
5. Random generators must emit a seed and a minimized fixture on failure; the minimized fixture is
   committed with the fix.

**Determinism and resources** (mirroring what `rust-umitools` already does):

* Byte-identical output across `--threads 1/2/8/16`, across chunk sizes, and across spill/no-spill.
* Byte-identical output between in-memory and bounded-memory paths.
* Peak RSS measured with `/usr/bin/time -v`; a declared budget must be honoured or the tool must exit
  4 before writing output.
* Temp-disk peak measured; ENOSPC and low-fd-limit tests.
* SIGPIPE and SIGINT behaviour tested for every stdout-writing command.

**What "testing against the original" explicitly does not mean:** it does not mean vendoring the
original, calling it at runtime in production, or accepting its bugs silently. Where the original has
a defect (cutadapt 4.6's `ZeroDivisionError` on `--max-average-error-rate` with an empty read is
already documented locally), the harness records the observable behaviour and the Rust implementation
either reproduces it exactly or documents the deliberate deviation.

---

## Risks and decisions

### Decisions

| ID | Decision | Alternatives considered | Rationale | Reversible? |
| --- | --- | --- | --- | --- |
| **D1** | Keep one binary per upstream contract; share code through libraries | Single multi-tool binary (BusyBox style) | Option grammars, exit codes, and release cadences conflict; compatibility is per tool | Yes (costly) |
| **D2** | New SAM/BAM/VCF work uses **noodles**; keep `rust-htslib` only where noodles lacks capability | Continue with rust-htslib everywhere | Pure Rust removes the C toolchain and the htslib CVE surface [S10][S40]; rust-htslib remains for CRAM/pileup edge cases [S41] | Yes |
| **D3** | `bioinfo-fastx` is written and owned here, not adopted from `needletail`/`seq_io` | Adopt needletail 0.7.3 | We need strict-vs-lenient modes, multi-member gzip, and byte-level error offsets that match cutadapt's diagnostics; needletail is a parser, not a policy layer [S50] | Yes (can wrap later) |
| **D4** | Compatibility is defined on decompressed bytes | Byte-identical compressed output | Deflate/BGZF/CRAM encoders legitimately differ; both existing crates already ship this rule | No |
| **D5** | Exit-code contract 0/1/2/3/4 across all binaries | Per-tool codes | Workflow engines branch on exit codes; a uniform contract is a feature | Yes (versioned) |
| **D6** | Unsupported capability → exit 2 before any output | Best-effort fallback | Silent divergence is the worst failure mode in bioinformatics; both crates already do this | No |
| **D7** | FastQC compatibility is a clean-room reimplementation of *outputs*, not of code | Port FastQC | FastQC is GPLv3 [S17]; reusing code would force GPL on this MIT workspace | No |
| **D8** | No aligner, no variant caller, no workflow engine | — | See non-goals; oracle infeasible at byte level [S22][S29] | No |
| **D9** | Release freeze: no tags, no dist uploads, no crates.io/Bioconda until the user lifts it | Publish 0.1.1/0.3.0 now | Explicit task constraint; also avoids publishing a doc-inaccurate cutadapt build | Lifted by user only |
| **D10** | Benchmark claims require a same-input correctness result | Speed-only claims | Already the documented policy of `rust-umitools`; prevents "fast but wrong" | No |

### Risk register

| ID | Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- | --- |
| **R1** | A stale dist artifact is mistaken for current behaviour | High (already true: 47 commits behind) | High | Dist release notes must name the source commit; README must say the binary is v0.1.0; W0-1 fixes this |
| **R2** | Oracle drift silently invalidates "verified" status | Medium | High | `versions.py` fail-closed; pin NumPy/pysam/samtools; record oracle provenance in every result |
| **R3** | Upstream releases change semantics (cutadapt 5.x, UMI-tools 1.2+, samtools 1.25) | High | Medium | Version-scoped claims; an upgrade is a deliberate, recorded migration with a new compatibility matrix |
| **R4** | htslib CVE exposure through `rust-htslib` | Medium | High | Track htslib 1.23.1/1.24 advisories [S10]; prefer noodles for new parsers; document minimum htslib |
| **R5** | FASTQ parser strictness divergence (the format has no spec) | High | Medium | Differential tests against each tool's own parser; strict mode by default; lenient mode opt-in and fuzzed [S8] |
| **R6** | Memory blow-up on pathological input (huge bundles, dense UMI graphs) | Medium | High | Existing bounded-memory paths and spill equivalence tests; exit 4 before OOM; adversarial fixtures |
| **R7** | Multi-member gzip breaks a downstream consumer | Low | Medium | Test decoders (`gzip`, Python `gzip`, `MultiGzDecoder`, `samtools`); offer single-member mode if needed |
| **R8** | Documentation drifts again after the fix | High | Medium | Machine-checked option table in CI (count + exit-code probes) |
| **R9** | Scope creep into aligners/callers | Medium | High | Non-goals are explicit; any proposal must name an oracle and a comparison level first |
| **R10** | Single-maintainer bus factor / personal paths | High | High | Portable oracles, containerized envs, ADRs, no `/home/...` in the repo |
| **R11** | Benchmark claims measured on a loaded shared machine | High | Medium | Record load average and raw JSON (already done in `BENCHMARK.md`); use median and range, not best case |
| **R12** | aarch64/macOS users cannot install | High (today) | Medium | Cross-build check in W1-6; document build-from-source until artifacts exist |

### Release and packaging note (consistent with the freeze)

**Current position: frozen.** No tag, no version bump, no dist upload, no crates.io or Bioconda
submission is part of this plan. The freeze is recorded in `docs/RELEASE-FREEZE.md` by W0-4 and can
only be lifted by the repository owner.

What is allowed during the freeze:

* Building from source with `--locked`.
* Producing local, unsigned artifacts for testing (never uploaded).
* Writing a release checklist and a conda recipe **draft** that is not submitted.
* Documenting the exact commands that *would* be run.

What is not allowed:

* `git tag`, `cargo publish`, dist-repository uploads, container registry pushes.
* Editing a published dist release note to imply newer behaviour.
* Changing the version of either crate.

**Preconditions for lifting the freeze** (each must be true, and the owner decides):

1. W0-1..W0-3 complete: documentation matches the binary and both oracles run in a clean container.
2. W0-5 complete: every published performance number is reproducible from a recorded JSON file.
3. Both release workflows build `--locked` with a pinned toolchain and produce identical SHA256 on a
   rebuild.
4. Dist release notes name the source commit and the compatibility matrix version.
5. A container image exists for both tools.
6. An aarch64 build is either published or explicitly declared unsupported in the README.

**Packaging recommendation for after the freeze** (do not execute now):

| Channel | Why | Notes |
| --- | --- | --- |
| Static Linux x86_64 + aarch64 tarballs | Current channel, extended | Keep SHA256 verification; add `--version --verbose` provenance |
| Container images (ghcr/quay) | Clusters and nf-core/Snakemake users | One image per tool; pin base image digest |
| Bioconda | The default install path for this audience | Requires a recipe, licence, and reproducible build; both licences are MIT-compatible |
| crates.io | Rust users and library reuse (`bioinfo-fastx`) | Publish libraries, not necessarily CLIs; requires a stable API and MSRV policy |
| `cargo-binstall` | Low-friction binary install | Optional |

The naming and checksum conventions already used by the two dist repositories are good and should be
kept; what is missing is multi-arch, provenance, and a single source of truth for "which commit is
this binary".

---

## Sources

All URLs below were accessed **2026-10-19** (UTC) via `web_search`/`web_fetch` from this session.
Where a page was retrieved only as a search snippet, that is stated. Local claims cite commands in
the evidence log instead of a URL.

| ID | Source | Used for |
| --- | --- | --- |
| S1 | Cutadapt documentation, <https://cutadapt.readthedocs.io/en/stable/> | What cutadapt does; MIT licence; citation |
| S2 | Cutadapt changelog, <https://cutadapt.readthedocs.io/en/stable/changes.html> | v5.2 (2025-10-23), v5.0 gzip level 1 default, `-Z` deprecation, report runtime removal, v4.6 (2023-12-06) baseline |
| S3 | Cutadapt user guide (v5.0 source text), <https://cutadapt.readthedocs.io/en/v5.0/_sources/guide.rst.txt> — retrieved as a search snippet | Adapter search parameters, `rightmost`, `anywhere`, `--revcomp` semantics |
| S4 | samtools manual page 1.24, <https://www.htslib.org/doc/samtools.html> | Command list, streaming model, index lookup rules, release date 9 July 2026 |
| S5 | samtools-markdup manual 1.24, <https://www.htslib.org/doc/samtools-markdup.html> | Duplicate-marking algorithm, `fixmate` prerequisite, optical duplicates, `--move-umi-to-tag` |
| S6 | bcftools manual page (git 1.24, updated 2026-06-16), <https://www.htslib.org/doc/bcftools.html> | Command list, VCF/BCF streaming and indexing rules |
| S7 | Samtools project home, <https://www.htslib.org/> | Samtools/bcftools/htslib relationship |
| S8 | HTS specifications, <https://github.com/samtools/hts-specs> and <http://samtools.github.io/hts-specs/> | SAM/BAM/CRAM/VCF/BCF/CSI/tabix specs; FASTQ has no formal definition |
| S9 | samtools 1.24 release page, <https://github.com/samtools/samtools/releases> | 1.24 changes: `fixmate` supplementary mates, `markdup --move-umi-to-tag`, subsample seed default |
| S10 | samtools 1.23.1 announcement, <https://sourceforge.net/p/samtools/mailman/message/59310678/> | CRAM/GZI security fixes, CVE-2026-31962..31971, backports |
| S11 | UMI-tools repository, <https://github.com/CGATOxford/UMI-tools> | Six commands, recommended `directional`, alevin recommendation for droplet scRNA |
| S12 | UMI-tools methods, <https://umi-tools.readthedocs.io/en/latest/the_methods.html> | `unique`, `percentile`, `cluster`, `adjacency`, `directional` definitions; tie-breaking |
| S13 | UMI-tools releases, <https://github.com/CGATOxford/UMI-tools/releases> | v1.1.6 determinism with `--random-seed`; tie-breaking changes vs earlier versions |
| S14 | UMI-tools `dedup` reference, <https://umi-tools.readthedocs.io/en/latest/reference/dedup.html> | Representative-read selection; `--output-stats` file list and null model description |
| S15 | PyPI `umi-tools` JSON, <https://pypi.org/pypi/umi-tools/json> | Latest version 1.1.6; dependency list |
| S16 | fastp repository, <https://github.com/OpenGene/fastp> | All-in-one QC/trim/dedup/merge features; JSON+HTML reports; citation to fastp 1.0 (2025) |
| S17 | FastQC project page, <https://www.bioinformatics.babraham.ac.uk/projects/fastqc/> | Java, GPLv3, modules, input formats, changelog (0.12.0) |
| S18 | FastQC releases, <https://github.com/s-andrews/FastQC/releases> | v0.13.0 changes: threads, SVG default, Phred32 default |
| S19 | sequali, <https://github.com/rhpvorderman/sequali> | Fast QC alternative; memory/runtime claims (**vendor claim**); timestamp-free reports |
| S20 | MultiQC, <https://multiqc.info/> and <https://github.com/ewels/MultiQC/releases> | 150+ tool modules; v1.35 |
| S21 | SeqKit, <https://github.com/shenwei356/seqkit> (+ releases) | 39 subcommands; compression formats; v2.14.0 (2026-09-24) |
| S22 | bwa-mem2, <https://github.com/bwa-mem2/bwa-mem2> | v2.2.1; identical output to bwa claim; 1.3–3.1x speed claim (**vendor claim**) |
| S23 | minimap2, <https://github.com/lh3/minimap2> | v2.31; presets; SIMD/x86-64 focus; splice and short-read modes |
| S24 | STAR, <https://github.com/alexdobin/STAR> | v2.7.11b (Jan 2024); memory requirements (**vendor claim**) |
| S25 | HISAT2, <https://github.com/DaehwanKimLab/hisat2> | Graph FM index; index size and footprint (**vendor claim**) |
| S26 | BBMap/BBTools, <https://github.com/BioInfoTools/BBMap> | Java suite; source location; ~3 MB download |
| S27 | sambamba, <https://github.com/biod/sambamba> | v1.0; D language; CRAM support removed in 0.8; GPLv2+ |
| S28 | Picard MarkDuplicates, <https://gatk.broadinstitute.org/hc/en-us/articles/360037052812-MarkDuplicates-Picard>; repo <https://github.com/broadinstitute/picard> | Duplicate marking, `BARCODE_TAG`, `DT`, quality-sum ranking, Java 17 |
| S29 | GATK releases, <https://github.com/broadinstitute/gatk/releases> | 4.7.0.0; HTSJDK 5.0.0; CRAM 3.1 write |
| S30 | DeepVariant, <https://github.com/google/deepvariant> | CNN caller; 1.10.0; ploidy limits |
| S31 | Ensembl VEP, <https://ensembl.org/info/docs/tools/vep/index.html> | Release 116; consequence and frequency annotation |
| S32 | salmon, <https://github.com/COMBINE-lab/salmon> and releases <https://github.com/COMBINE-lab/salmon/releases> | 2.0 from-scratch Rust rewrite; index format change; 2.8.0 |
| S33 | kallisto, <https://github.com/pachterlab/kallisto> | Pseudoalignment + EM; BSD-2; bustools for single-cell |
| S34 | htseq-count, <https://htseq.readthedocs.io/en/latest/htseqcount.html> | `union`, `intersection-strict`, `intersection-nonempty`; `--nonunique` modes |
| S35 | tximport, <https://bioconductor.org/packages/tximport/> | 1.40.0; transcript-to-gene summarization; consumer of `quant.sf` |
| S36 | alevin-fry, <https://github.com/COMBINE-lab/alevin-fry> | RAD input; permit list; gene×cell counting; recommends piscem |
| S37 | piscem, <https://github.com/COMBINE-lab/piscem> | Pure-Rust cDBG mapper; x86-64/ARM binaries; portable SIMD baseline |
| S38 | Cell Ranger, <https://github.com/10XGenomics/cellranger> | 10.1.0; source informational only; official binaries |
| S39 | fgbio, <https://github.com/fulcrumgenomics/fgbio> | `GroupReadsByUmi`, `CallMolecularConsensusReads`, `CallDuplexConsensusReads`, UMI correction |
| S40 | noodles, <https://github.com/zaeleus/noodles> and crates.io API | 0.118.0; supported formats; MSRV 1.91; experimental API |
| S41 | rust-htslib, <https://github.com/rust-bio/rust-htslib> and crates.io API | 1.0.1 on crates.io / 1.0.2 changelog; htslib bindings; CRAM features |
| S42 | Snakemake documentation, <https://snakemake.readthedocs.io/en/stable/> | 9.27.0; rule-based workflows; conda/container deployment |
| S43 | Nextflow overview, <https://www.nextflow.io/docs/latest/overview.html> | Dataflow model; executor abstraction |
| S44 | nf-core, <https://nf-co.re/> | Curated Nextflow pipeline community; CI-tested releases |
| S45 | WDL specification, <https://github.com/openwdl/wdl> | Spec 1.3 current, 2.0 next; multiple engines |
| S46 | Riker, <https://docs.rs/crate/riker-ngs/latest> and <https://docs.seqera.io/multiqc/modules/riker> | Rust Picard-metrics port; speed claims (**vendor claim**) |
| S47 | rust-bio, <https://github.com/rust-bio/rust-bio> | Algorithms/data structures library; MSRV 1.87 |
| S48 | bioinfo-tools distribution releases, <https://github.com/seek-hope/bioinfo-tools-dist/releases> | Consolidated repository; one release carries both binaries |
| S49 | ~~rust-cutadapt-dist~~ / ~~rust-umitools-dist~~ | Deleted on 2026-10-21; histories archived in the `archive-pre-monorepo-2026-10-21` release of `bioinfo-tools` |
| S50 | needletail, <https://github.com/onecodex/needletail> and crates.io API | 0.7.3; minimal-copy FASTX parsing; k-mer API |
| S51 | Trimmomatic releases, <https://github.com/usadellab/Trimmomatic/releases> | v0.41; Java 25; `ILLUMINACLIP`, `SLIDINGWINDOW`, `MINLEN` |
| S52 | fq (St. Jude), <https://github.com/stjude-rust-labs/fq> | 0.12.0; `filter`, `lint`, `subsample`; Bioconda |
| S53 | fqtk (Fulcrum), <https://github.com/fulcrumgenomics/fqtk> | Rust FASTQ demux/shard/subsample; read structures; UMI tag handling |
| S54 | freebayes, <https://github.com/freebayes/freebayes> | Haplotype-based Bayesian caller; scope limits |

### Local evidence (no URL)

| Claim | Command |
| --- | --- |
| Workspace layout, Git state, commit ids | `ls`, `git -C <crate> status/describe/rev-list` |
| `rust-cutadapt` 208 tests, 119/119 differential | `cargo test --release --offline`; `python3 tests/oracle/differential.py --matrix` |
| `rust-umitools` 39/39 semantic | `bash tests/oracle/run_oracle.sh` |
| 77-case exact suite is claimed but not re-run here | `run_exact.sh` not executed; README/`docs/COMPATIBILITY.md` claim 77 |
| cutadapt probe behaviour (`-q`, `-g`, `-u`, `-j`, `--json`, `-N`, `--poly-a`, `-A`) | direct binary runs on `tests/fixtures/simple.fastq` |
| Recorded benchmark numbers | `rust-cutadapt/BENCHMARK.md`, `bench/reference.json`, `rust-umitools/docs/BENCHMARK.md` |
| Oracle pins | `rust-umitools/tests/oracle/versions.py` and harness output |
| Dist artifact staleness | `git -C rust-cutadapt rev-list --count d9c4c52..HEAD` = 47 |
| htslib/samtools/bcftools absent from PATH | `which samtools bcftools` |

### Uncertainty register

| Claim | Status |
| --- | --- |
| `rust-umitools` 77/77 byte-exact | **[unverified in this session]** — documented, harness not run |
| cutadapt 6.3x–6.9x on the current HEAD binary | **[stale]** — measured on the v0.1.0 code path; W0-5 re-measures |
| FastQC 0.13.0 feature list beyond the release notes | **[partially verified]** — release notes read; module internals not audited |
| featureCounts / SnpEff / Bowtie2 details | **[unverified]** — pages not retrieved; cited only as names |
| Riker 12–38x speedups; sequali 2 GB/3–30 min; STAR 16–32 GB; HISAT2 6.7 GB; bwa-mem2 1.3–3.1x | **[vendor claim]** — project documentation, not reproduced |
| Single-cell downstream consumers accept a Matrix Market writer | **[unverified]** — planned validation in W2-3, not yet tested |
| crates.io `rust-htslib` 1.0.1 vs repo 1.0.2 | **[partially verified]** — crates.io API says 1.0.1; changelog mentions 1.0.2; the discrepancy was not resolved |
| htslib security fixes relevance to `rust-htslib` 0.49 | **[unverified]** — which htslib version 0.49 bundles was not checked |

---

*End of plan. The only file created by this task is `docs/DEVELOPMENT-PLAN.md`; no source, manifest,
tag, or release was modified.*
