# README performance measurements — 2026-10-31

These measurements describe the initial build before the
[optimization after e125519](OPTIMIZATION.md). The original observations and
interpretation below remain the record for that build.

The complete run contains **33 conditions and 330 timed process executions**, plus
66 warm-up executions. There are **29 conditions with output or validation-verdict
checks** and **4 descriptive FastQC conditions** with independently checked basic
counts/lengths. All passed those declared checks. FastQC's complete tables differ;
this is not a claim of full report equivalence.

[All 33 tables, ranges and resources](results/readme-2026-10-31/tables.md) ·
[Numeric TSV](results/readme-2026-10-31/tables.tsv) ·
[Formal raw JSON](results/readme-2026-10-31/formal.json) ·
[10M raw JSON](results/readme-2026-10-31/large.json) ·
[Protocol](README-PROTOCOL.md)

## What the results establish

On one million synthetic alignments, native directional UMI deduplication took a
median **0.453 s**, versus **5.116 s** for UMI-tools; the median of paired
upstream/native ratios was **11.20×**. The synthetic graph has four UMI types per
coordinate bundle, uneven abundances and independently known representatives.
It does not represent real UMI libraries or dense barcode graphs. These are
whole-command timings, including interpreter startup, compression and I/O.

Several native commands were slower. On one million real reads, `stats -a -T`
took **2.225 / 0.566 s** and single-CPU adapter trimming **2.872 / 1.840 s**
(native / upstream). Four-CPU trimming on the same reads was **0.779 / 0.772 s**;
those observations do not support a material speed advantage. At ten million
synthetic reads, native `lint` took **6.467 / 1.959 s** and reached **829.4 / 4.4 MiB**
reported peak RSS. No average speedup across the suite is calculated.

Input type matters: one-million-read FASTA conversion was faster on the real
subset (**0.212 / 0.270 s**) and slower on synthetic reads (**0.200 / 0.139 s**).
Both datasets and all smaller cases remain in the full tables.

## Machine and execution conditions

- Linux x86-64, Intel **i7-13700H**, 6 P cores + 8 E cores, 20 logical CPUs,
  31.1 GiB OS-visible RAM. Exact platform/topology is recorded in JSON.
- **tmpfs** held inputs, outputs and temporary files. Inputs were reread before
  each run, followed by two seconds of idle time. No cache drop or durability
  `fsync` was requested. These are not physical-disk or cold-storage measurements.
- Single-CPU commands and descendants were bound to logical CPU **4**; four-CPU
  cutadapt used **0,2,4,6**, one logical CPU from each of four P cores.
  Affinity did not reserve those CPUs or their SMT siblings from other work.
- The laptop was shared and frequency was not locked. Recorded package
  temperatures before/after formal executions ranged **44–100°C**; one-minute
  load averages ranged **0.91–3.16**. Two-second pauses did not equalize thermal
  state. These conditions limit generalization and may influence timings.
- One warm-up and five fresh-process repetitions per implementation/condition;
  seeded, balanced/randomized order within pairs. Conditions themselves ran in
  the recorded order. Generation and correctness checks were outside timing;
  no benchmark, compilation or quality gate ran concurrently with measurements.
- Wall time uses `perf_counter` around GNU time and the command, including
  startup. User/system time and RSS come from GNU time. RSS is the maximum across
  five executions, **not total concurrent process-tree RSS**. Small residual
  sub-second cases remain sensitive to startup and instrumentation.
- Temporary storage is sampled every 20 ms; peaks shorter than that can be
  missed. JSON records physical payload size and total output-directory bytes.
  Same gzip/BGZF level does not imply the same codec or compressed bytes.

Tables report medians and observed min–max, not confidence intervals. Ratios are
the median of the five paired ratios, not the ratio of two medians. Throughput
is input records divided by median elapsed time. Repetitions are executions of
one dataset, not independent biological samples.

## Inputs

Real data are mate 1 of ENA [SRR2584863](https://www.ebi.ac.uk/ena/browser/view/SRR2584863),
*Escherichia coli B str. REL606*, Illumina HiSeq 2500. The downloaded source has
1,553,259 reads, 183,292,913 compressed bytes and official MD5
`4a1529a4165970302d6d8cb50b6e7a43`. The generator checked that MD5 and byte count.
The 100k and 1M subsets sample indices without replacement with seed **20261031**,
then retain source order. The subsets share one biological source; they are not
independent biological replicates.

Synthetic FASTQ uses the same seed, 150 bp A/C/G/T reads and a fixed quality
profile (Q30 for 100 bases, Q20 for 50, with every thousandth read at Q2).
An adapter is inserted into every tenth generated read; every twentieth read at
offset one copies the preceding sequence. Copying can also copy an adapter, so
insertion frequency is not the final fraction of adapter-containing reads.
The 100k, 1M and 10M datasets are prefixes of one deterministic generator.

Synthetic BAM uses 50 bp alignments and `UB` UMIs. Nominal coordinate-bundle
scales 1/10/100 give UMI abundances 6s/2s/s/s, with edit distances 0/1/2/10 from
the major UMI. The last bundle may be truncated to the requested record count.
The 1M input has 35,701 bundles and 71,402 expected deduplicated representatives.
Unique highest MAPQ values determine those representative QNAMEs independently.
Sort receives reverse-coordinate input; dedup receives a pre-sorted, indexed BAM.

[Original manifests](results/readme-2026-10-31/inputs.json) and the
[10M extension manifest](results/readme-2026-10-31/inputs-large.json) contain input,
index and expected-QNAME hashes. Archived `bundle_scales` lists nominal values;
the final 1M bundle has scale 40. This metadata limitation is annotated rather
than changing historical manifest bytes.

## Commands and correctness rules

Exact argv, environment controls, output hashes, stderr and per-pair checks are
in the raw JSON. The following settings apply to both sides unless stated:

| Case | Timed work | Check outside timing |
| --- | --- | --- |
| cutadapt plain | `-a AGATCGGAAGAGC -m 30 -j 1`; plain FASTQ input/output | Exact decoded FASTQ bytes |
| cutadapt gzip | Same, gzip output level 6; explicit upstream `--compression-level=6` and native `RUST_CUTADAPT_COMPRESSION_LEVEL=6` | Exact decompressed bytes; compressed bytes may differ |
| cutadapt 4 CPU | Plain output, `-j 4`, four-CPU affinity | Exact FASTQ bytes |
| SeqKit stats | `stats -a -T -j 1` | Exact TSV bytes |
| SeqKit fq2fa | Default unwrapped conversion, `-j 1` | Exact FASTA bytes |
| fq lint | Default validators, valid input | Independent FASTQ scan, success exits, and rejection of a malformed last record; logs are not byte-compared |
| samtools flagstat | Unsorted synthetic BAM, one CPU | Exact statistics |
| samtools sort | One CPU, about 32 MiB per chunk, final BGZF level 6 | BGZF EOF/quickcheck, record count, coordinate order and canonical BAM records/header |
| UMI-tools dedup | `--extract-umi-method=tag --umi-tag=UB --method=directional --random-seed=1 --compresslevel=6` | Same BAM checks and independent representative QNAMEs |
| FastQC | `--quiet --extract --outdir`; default modules, one CPU | Independent read count, displayed total bases and length; raw module comparisons preserved |

BAM comparison removes `@PG` provenance headers and sorts records **only within
coordinate ties**. It rejects decreasing coordinates and missing BGZF EOF.
The sort chunk budget is not a hard RSS cap; intermediate compression formats
are implementation-specific. Lint results establish the measured valid-input
verdict and invalid-tail rejection, not exhaustive equivalence of all validators.

### FastQC's separate scope

No FastQC speed ratio is reported. Native HTML/ZIP lacks the upstream plot
resources, and these inputs also expose statistical discrepancies:

| Dataset | Raw-equal modules in all five pairs | Differing modules |
| --- | ---: | --- |
| synthetic 100k | 9/10 | Per sequence GC content |
| synthetic 1M | 8/10 | Per sequence GC content; Sequence Duplication Levels |
| real 100k | 10/11 | Per sequence GC content |
| real 1M | 9/11 | Per sequence GC content; Sequence Duplication Levels |

Twelve modules are implemented, but optional tile/Kmer modules are not all
present by default/on every input. These denominators describe the modules
actually emitted. The GC/duplication differences are **unresolved**, not granted
upstream-defect exceptions. Native totals and lengths passed the independent
checks; that does not establish the remaining tables' mathematical correctness.

## Source, failed pilot and review

Measured native source is **`aaf4ba4` plus two corrections**, not unmodified
`aaf4ba4`: inclusive Q20/Q30 counts and default unwrapped `fq2fa`. The
[exact patch](results/readme-2026-10-31/native-source.patch), all native source
hashes and executable hashes identify the build. After the workspace release
build, the corrected FASTX CLI was rebuilt with
`cargo build --release --locked -p rust-fx`. Cargo resolves `memchr`/`flate2`
features differently for that package-only build versus the workspace build;
repeating the package build reproduced the exact measured `rust-fx` SHA-256.
See [build provenance](results/readme-2026-10-31/build-provenance.json). Timings
refer to those recorded binaries, including that build distinction.
Pinned references are cutadapt
4.6, UMI-tools 1.1.6, samtools 1.24, FastQC 0.13.0, SeqKit 2.14.0 and fq 0.12.0;
Python packages, runtime payload hashes, Java and toolchain details are recorded.

The [failed first pilot](results/readme-2026-10-31/pilot.json) is retained.
The [second pilot](results/readme-2026-10-31/pilot2.json) passed all 30 declared
conditions after implementation and command/setup fixes; pilot timings are not
substituted for the five-repeat run. An independent boundary comparison recorded
[16/22 for the old binary](results/readme-2026-10-31/boundaries-old.json) and
[22/22 for the fixed binary](results/readme-2026-10-31/boundaries-fixed.json).

[Archived executed scripts](results/readme-2026-10-31/executed-scripts/) match the
hashes in both raw measurement reports. Maintained scripts add post-run review
fixes for invalid case selection, incomplete matrices, pilot/failed-summary
handling and BAM subprocess cleanup. No native implementation changed after
measurement. [Checksums](results/readme-2026-10-31/sha256.json) cover the original
observations, manifests, scripts and patch; tables are generated separately.

Gemini **4 Argon** reviewed the design and drafted bilingual text; its suggestions
were checked against source and measurements. It did not execute or certify the
benchmarks. See [collaboration notes](../docs/readme-review/README.md).

## Reproduce

Use Linux with `/usr/bin/time`, `taskset`, Python 3, Rust and the pinned references
from [CI provisioning notes](../tests/ci/README.md). Oracle installations and
local wrapper scripts under `tests/ci/env/` are git-ignored, not bundled with this
repository. Install UMI-tools 1.1.6, samtools 1.24 and OpenJDK in one environment
(the measured Java was 25.0.2), unpack FastQC 0.13.0, and install the pinned
SeqKit/fq binaries. For example, a conda environment can be created with
`conda create -p /path/to/umi-env -c conda-forge -c bioconda umi_tools=1.1.6 samtools=1.24 openjdk=25.0.2`.
Install cutadapt 4.6 in a separate Python environment. Dependency resolutions may
differ from this host; compare the runtime versions/payload hashes in each result.

The included staging helper checks versions and creates portable launchers using
those existing installations; it downloads nothing and refuses an existing cache:

```bash
python3 bench/prepare_readme_oracles.py --cache /tmp/readme-oracles \
  --umi-prefix /path/to/umi-env --fastqc-dir /path/to/FastQC \
  --seqkit /path/to/seqkit --fq /path/to/fq
```

No toolchain extraction occurs inside timed commands. Select CPU IDs from your
machine's actual topology. Choose a fresh work directory with sufficient
storage/RAM. To compare with this run's tmpfs behavior, also use tmpfs; a disk run
measures different I/O conditions.

```bash
CARGO_TARGET_DIR="$PWD/target" cargo build --release --workspace --bins --locked
CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked -p rust-fx
export CUTADAPT_ORACLE=/path/to/cutadapt-4.6
export BENCH_WORK=/tmp/bioinfo-readme-reproduction
export BENCH_ORACLES=/tmp/readme-oracles
python3 bench/readme_inputs.py --workdir "$BENCH_WORK" \
  --samtools "$BENCH_ORACLES/bin/samtools"
python3 bench/readme_benchmark.py --inputs "$BENCH_WORK/inputs.json" \
  --out "$BENCH_WORK/formal.json" --run-name formal-runs \
  --cutadapt "$CUTADAPT_ORACLE" --repeats 5 --warmups 1 --cooldown 2 \
  --oracle-dir "$BENCH_ORACLES/bin" --toolchain-cache "$BENCH_ORACLES" \
  --cpu 4 --multi-cpu 0,2,4,6
python3 bench/extend_readme_inputs.py --inputs "$BENCH_WORK/inputs.json" \
  --samtools "$BENCH_ORACLES/bin/samtools"
python3 bench/readme_benchmark.py --inputs "$BENCH_WORK/inputs-large.json" \
  --out "$BENCH_WORK/large.json" --run-name large-runs \
  --cutadapt "$CUTADAPT_ORACLE" --repeats 5 --warmups 1 --cooldown 2 \
  --oracle-dir "$BENCH_ORACLES/bin" --toolchain-cache "$BENCH_ORACLES" \
  --cases fx_fq2fa,fq_lint,bam_flagstat --cpu 4 --multi-cpu 0,2,4,6
python3 bench/summarize_readme.py "$BENCH_WORK/formal.json" "$BENCH_WORK/large.json" \
  --out "$BENCH_WORK/tables.md"
```

The example CPU IDs match this host; choose appropriate CPU options for another
environment. The recorded run used the older local launchers and
`/tmp/bioinfo-ci-toolchain`, whose hashes and runtime payloads are in JSON; reference
launcher startup is included in those timings. The data download
is about 183 MB; generated inputs, paired outputs and temporary SAMs require
several GiB. Prepare inputs before benchmarking and avoid simultaneous builds or
quality gates. The README's small-code correctness changes can be replayed from
the recorded baseline using the saved patch; copy archived scripts into that
checkout's `bench/` directory to reproduce their exact versions.

The older `upstream-current.json` is unchanged. Its measured revision is
`3b9bea4`; `0125a9b` is the commit that recorded that result. It has 1k/10k synthetic
inputs and two repetitions, so it is not current README performance evidence.
