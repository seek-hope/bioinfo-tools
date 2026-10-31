# README benchmark protocol

This protocol is separate from the historical `run.py` / `upstream.py` results.
It was specified before the new pilot and reviewed with Google Gemini's **4 Argon**
through the user-authorized web conversation. Raw measurements, failures and
scope limitations will be retained; no result is a universal speed claim.

## Questions and scope

Measure elapsed time, CPU time, peak RSS and disk/output use for representative
commands at source revision `aaf4ba4` plus the two pilot-discovered `rust-fx`
corrections (inclusive Q20/Q30 and default unwrapped `fq2fa`). The exact
[native patch](results/readme-2026-10-31/native-source.patch), source hashes,
binary hashes and [executed scripts](results/readme-2026-10-31/executed-scripts/)
are retained. Compare cutadapt 4.6, UMI-tools 1.1.6,
samtools 1.24, FastQC 0.13.0, SeqKit 2.14.0 and fq 0.12.0. This is a local Linux
CLI benchmark, not an alignment benchmark or a claim about all supported options.

Use 100,000 and 1,000,000 records initially. If both implementations of a streaming
case finish below one second at one million records, add a 10-million-record
synthetic case where the memory/disk pilot permits. Do not enlarge all cases
merely to obtain a more favorable ratio. Keep pilot results separate from the
five-repeat measurements.

## Inputs

- Real reads: ENA run **SRR2584863**, *Escherichia coli B str. REL606*, Illumina
  HiSeq 2500. Download mate 1 only, verify ENA MD5 and byte count, and select
  reproducible record indices without replacement using a fixed seed. This is a
  single-end workload drawn from one biological library, not independent
  biological replication. Record the URL, metadata and selected-input SHA-256.
- Synthetic reads: seeded A/C/G/T sequences, 150 bp, a declared fraction of
  inserted adapters and duplicate sequences, with position-dependent qualities.
  Sizes are prefixes of the same deterministic stream; no biological realism claim.
- Synthetic alignments: valid SAM coordinates, 50 bp alignments and DNA UMI tags.
  Coordinate bundles have unequal depths, one-/two-edit UMI neighbors and a
  distant UMI. Unique highest-MAPQ records make representatives independently
  checkable. Reverse coordinate order exercises sorting; dedup input is prepared
  and indexed outside timing. This does not model a complete UMI library, complex
  splicing, cell barcodes or large dense UMI graphs.

## Cases and controls

| Case | Settings and comparison |
| --- | --- |
| cutadapt plain | single-end adapter trim, minimum length 30; plain input/output, 1 CPU |
| cutadapt gzip | same, gzip output level 6 on both sides, 1 CPU |
| cutadapt 4 CPU | plain input/output, four distinct P cores, `-j 4` |
| SeqKit stats | `stats -a -T`, one CPU and `-j 1`; exact TSV |
| SeqKit fq2fa | plain FASTQ to plain FASTA, one CPU and `-j 1`; exact bytes |
| fq lint | default validators, valid input and separate invalid-tail negative control |
| samtools flagstat | synthetic BAM, one CPU; exact statistics |
| samtools sort | same unsorted BAM, 32 MiB approximate chunk budget, final BGZF level 6, one CPU |
| UMI-tools dedup | directional method, tag `UB`, seed 1, final BGZF level 6, one CPU |
| FastQC (separate) | default modules and extracted reports, one CPU; descriptive timings only |

Gzip and four-CPU trimming initially use the one-million-read inputs; the other
FASTQ cases use both sizes. BAM cases use both synthetic sizes. FastQC is never
included in the equal-work speed-ratio table: native plot resources are incomplete,
and independently proved statistical corrections can change its data.

## Execution and measurement

- Build native binaries with `cargo build --release --workspace --bins --locked`,
  then rebuild the corrected FASTX CLI with `cargo build --release --locked -p rust-fx`.
  This final per-package build is the measured `rust-fx` artifact. Cargo feature
  unification makes its dependency configuration differ from a workspace-only
  build; the [build provenance](results/readme-2026-10-31/build-provenance.json)
  records that distinction and an exact binary-hash reproduction.
  Record source revision, dirty paths, source/script/binary hashes and real upstream
  version commands (`seqkit version`, not `seqkit --version`).
- Inspect CPU topology. On this i7-13700H, bind single-CPU commands and their
  descendants to logical CPU 4; use 0,2,4,6 for four-CPU commands. These are distinct
  physical P cores. Affinity does not reserve sibling threads against other users.
- Run no concurrent benchmark, compilation or quality gate. Keep the normal CPU
  governor and power configuration. Record thermal/frequency/load observations;
  this shared laptop is not an isolated or frequency-locked laboratory machine.
- One untimed warm-up per implementation and condition. Then five paired blocks
  with a seeded, balanced/randomized implementation order. Every execution starts
  a new process; Python/JVM initialization remains included.
- Read the input immediately before each run to warm its filesystem cache, then
  allow a fixed two-second idle interval. For this run, inputs, outputs and
  temporary files are on **tmpfs**, not physical disk. No privileged cache
  flushing or governor changes. Output uses ordinary buffered file I/O, without
  a durability `fsync`; these results do not establish disk throughput.
- Measure high-resolution elapsed time around GNU time and the command. Also retain
  GNU time user/system seconds and maximum RSS. Its RSS is **not** the sum of
  concurrent processes' resident sets. Monitor temporary file bytes at 20 ms
  intervals; this is a sampled high-water mark, not a disk quota or exact peak.
- After each pair, check both outputs outside timing, retain digests and diagnostics,
  and remove successful large outputs before the next pair. Rewarming and the idle
  interval follow validation. Keep failed outputs for inspection.
- Report individual samples, median, min/max, throughput, peak RSS, output sizes
  and paired speed ratios. Five reruns estimate execution variation on this
  machine; they are not five independent datasets. Do not infer population-wide
  uncertainty or report significance tests from those reruns.

## Correctness and exclusions

Every measured execution must exit successfully and have a same-input check.
Compare decompressed trimmed FASTQ and converted FASTA byte-for-byte; compare
stats/flagstat tables exactly. BAM comparison removes `@PG` provenance header
lines and canonicalizes records only within equal-coordinate ties, whose order
is not guaranteed by samtools. Verify coordinate monotonicity and expected input
record count; dedup also checks independently known representative QNAMEs.

For lint, compare successful validation verdicts with an independent FASTQ scan
and require both implementations to reject an invalid record appended after the
entire input. No output-content equivalence is claimed for its timestamped logs.

For FastQC, retain module-by-module raw equality/difference results, independently
verify total reads, **displayed** total bases and length range, and declare the exact verified scope.
Do not silently exempt a whole module or treat known correction policies as proof
for an unexamined mismatch. A mismatch remains visible and receives no speed ratio.

Failed or incomplete cases stay in the results. Any later protocol amendment must
state its reason and preserve earlier observations. Historical results are not
overwritten. The README includes slower results and links to all measurements.

## Execution amendments and provenance

The pilot exposed two native defects; they were corrected before the formal run.
It also established the pinned cutadapt compression default as level 5, so both
measured implementations explicitly use level 6 for gzip. Command syntax was
corrected for FastQC `--outdir` and native BAM sort before the successful second
pilot. The failed first pilot remains archived separately.

The 10M extension includes `fq2fa`, `fq lint` and BAM `flagstat`, selected by the
sub-second rule above. Four-CPU cutadapt remains at 1M: a 10M plain FASTQ plus two
trimmed FASTQ outputs would approach the available tmpfs/RAM working-set budget.
The smaller FASTA output pairs fit; successful outputs were removed after each
pair. This resource decision was made before the extension, not from its ratios.

The archived BAM manifests' `bundle_scales: [1,10,100]` describes the **nominal**
scales. The final bundle can be shortened to meet the requested record count;
for example the 1M dataset ends at scale 40. Expected representatives use the
actual counts. The maintained generator now records observed and nominal scales
separately; original manifests and their hashes have not been rewritten.

After measurement, review added case-selection/completeness guards, closed the
BAM verifier's subprocess resources on exceptions, and corrected summary handling
of pilots and failed warm-ups. Those changes do not alter any native binary or
timed command. The archived scripts match the hashes in the raw formal and large
reports; the maintained scripts include these additional guards.
