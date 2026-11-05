# Performance acceptance for implemented functionality

The maintainer selected coverage **B**, resource policy **A**, and a **10%**
margin. This is an ongoing objective, not a claim that the repository meets it.
The starting delivery is `d3d8edc3d5970415e5426ebc5255917b4e334afa`.

## Acceptance

Cover all implemented major functions and common small/large, short/long-read,
single/paired-end and compressed/uncompressed uses. Unsupported commands are not
silently included as implemented; experimental implementations remain visible.
Existing approved corrections and native-only capabilities retain their explicit
comparison limits. FastQC whole-report timing remains descriptive until its
statistical and graphical differences have been resolved.

For each eligible condition compute the native/upstream wall-time ratio **within
each measured block**, then take its median. Require that median to be at most
**0.90**: at least 10% less elapsed time. The reciprocal upstream/native ratio must
therefore be at least **10/9**, not merely 1.10. Report all conditions separately,
including losses, failures and incomplete populations. A suite average cannot
satisfy this gate. Correctness and resource compliance are prerequisites.

Use pinned references and native Rust runtime implementations. Keep complete
records, diagnostics, exits, partial-output behavior, deterministic outputs and
existing independently proved corrections. Investigate algorithmic work first,
then implementation overhead, then concurrency. Never substitute less validation
or different report work to obtain a faster result.

## Coverage inventory

The following inventory expands the previous 26 comparable conditions and two
descriptive QC conditions. It defines families to expand into frozen executable
case manifests; it is not evidence that these cases have already run. Existing
losing conditions remain in the population. The nine later statistics/sort rows
revisit existing families rather than adding nine new functional categories.

| Family | Required functions and modes | Discriminating populations and checks |
|---|---|---|
| cutadapt matching | 3′, 5′, anchored, anywhere and linked adapters; wildcard/indel modes; long adapters; repeated trimming; reverse complement | Adapter-rich/poor reads, 1/32 adapters, short/long reads; full records and reports |
| cutadapt modification/routing | Quality/NextSeq/poly-A/fixed/N trimming; filters/actions; renaming; auxiliary outputs; demultiplexing | SE, two-file PE and interleaved PE; FASTA/FASTQ; all destinations and JSON/report checks |
| FX statistics/conversion | `stats`, `seq`, `fq2fa`, `fx2tab`, `tab2fx`, `fa2fq` | Basic/extended statistics, varied lengths and quality offsets; wrapped/four-line input; complete tables/records |
| FX selection/coordinates | `head`, `range`, `subseq`, `sliding`, `restart`, `translate` | Negative tails, BED/GTF/flanks, circular windows, six frames and alternative tables; retain approved corrections |
| FX matching/set operations | `grep`, `common`, `rmdup` | ID/name/sequence, regex/degenerate/mismatch/embedded matches, 1/1000 patterns, 0%/90% duplicates, 2/8 files; every report/output |
| FX sorting/editing | `sort`, `duplicate`, `rename`, `replace` | ID/natural/length/sequence keys, implemented two-pass path, regex/key-value/UUID modes; complete artifacts and documented ordering/UUID limits |
| FX sampling/partition/index | `sample`, `split`, `faidx` | Proportion/count, records/parts, fresh/reused indexes, explicit/regex region queries; reproducibility/selection checks where RNGs differ |
| FX validation | `lint` | Valid and malformed-tail SE/PE, supported validation modes, paired-name growth; full diagnostics and exit contracts |
| BAM conversion/query/index | `view`, `flagstat`, `idxstats`, `index` | SAM/BAM conversions, filtering/count/header/regions, BAI/CSI; varied tags, reference names, paired flags and CIGARs; full fields/index-query checks |
| BAM sorting | `sort` | In-memory, one spill level and multiple levels; SAM/BAM output; two memory budgets/fan-ins; high/low compression and long records; full fields/order/EOF checks |
| BAM coverage | `depth`, `coverage` | Zero positions, overlapping mates, region/BED/multiple inputs and histogram modes; reference span/coverage diversity; full tables |
| BAM short commands | `quickcheck`, `head`, `flags` | 1/100 files, header-only/limited records, invalid files, numeric/named flags; fresh-process latency and exact contracts |
| UMI FASTQ | `extract`, `whitelist` | String/regex, 5′/3′, SE/PE, quality/masking, read/UMI counts, knees/corrections; barcode diversity and complete FASTQ/TSV |
| UMI alignment | `dedup`, `group` | Five clustering methods, name/tag/umis sources, SE/PE, gene/cell/transcript modes, statistics and group artifacts; varied UMI graphs, not only uniform repetitions |
| UMI counting | `count`, `count_tab` | Gene/contig/transcript and sparse/wide cell output; gene×cell diversity and partition/spill paths; complete counts and independent reconciliation |
| FastQC | All 12 modules and implemented report/artifact paths | Default/Kmer/tile/custom configurations, long/grouped reads, duplication/length options and multiple files; module data, correction proofs and actual report assets |

Start common short-read populations at 100k/1M records (or pairs) and long-read
populations at 10k/100k variable-length records. Use both real and synthetic data;
repetition extends runtime but does not add diversity. Annotation/reference cases
need distinct contig and interval populations. Expand orthogonal modes deliberately,
record the chosen combinations before timing, and do not prune slow combinations.

Keep SAM/BAM in the BAM-tool comparison: this build explicitly rejects CRAM.
UMI's implemented CRAM paths require their own reference and MD/NM comparison
contract. Missing commands, FX unsupported two-pass variants and FastQC unsupported
launcher/format options are separate compatibility work. Native-only stable UMI
representatives and non-identical RNG/UUID behavior need labeled semantic checks;
they cannot become exact-equivalence passes by removing fields from comparison.

## Resources and measurement

Keep local disk storage and the established physical-core affinities: CPU `4`,
`4,6`, and `0,2,4,6`. Every condition records real CLI worker settings. Algorithms
that remain serial still run under each selected resource budget; CPU affinity
alone does not establish parallel scaling.

For the expanded suite, the initial per-invocation acceptance limits are **8 GiB
of observed aggregate process RSS**, **32 GiB of temporary/output files**, and
**1800 seconds**. Preserve at least **16 GiB** of filesystem headroom, separately
reserving inputs, both arms' outputs and any retained native anchor. Algorithm
options such as sort's chunk budget are recorded separately and are not RSS caps.
These limits permit bounded time/memory/disk tradeoffs; upstream receives the same
resource envelope and its settings must be chosen fairly within it.

A sampled process/file observer is an abort guard and a measured resource limit,
not an OS-enforced hard memory or disk bound. It may miss short peaks; report the
interval, GNU-time high-water RSS and descendant accounting limitations. The new
sampled guard and batch execution primitive implement these checks and have local
controls; the pure plan module adds complete-population, calibration, paired-order
and per-condition ratio checks. See [SUPERIORITY-RUNNER.md](SUPERIORITY-RUNNER.md).
Raw batches with deterministic complete-byte output contracts now have a strict
verifier and 204 fresh-CLI integration controls. A matrix-reconciled inventory now
preserves all historical conditions and immutable family obligations; 18 BAM short
cases have 108 concrete command specifications linked to 216 exact invocations.
It remains `complete=false`: broader semantic family adapters, the other concrete
cases, complete experiment integration and formal resource eligibility are unfinished.
Existing scale runners/results do not enforce this new envelope. Changes to limits require a new
plan/calibration and retention of earlier failures.

Retain the established calibrated throughput protocol (one warm-up, three measured
paired blocks, balanced order, target fastest arm about 15 seconds and every
arm/budget at least 10 seconds before freeze). Common-size and startup-dominated
cases also need a separately frozen **batch of fresh CLI invocations at fixed input
size**; calibrate the batch count, not the requested input size. State results as
batch throughput/mean invocation time, not a measured distribution of individual
latencies. Include process startup, normal I/O and launch overhead; validate all
outputs outside timing. The batch primitive now retains all fresh-process outputs
and applies the sampled resource/timeout guards. Twenty local controls and 128
actual pinned CLI invocations passed execution/output validation. Frozen family
manifests, complete calibration and formal population checks are still required
before accepting expanded measurements.

No builds, input generation, correctness suites, bulk transfers or competing
benchmarks run during accepted timing. Capture source/binary/runtime identities,
input hashes, commands, complete populations, temperatures, CPU use, memory,
filesystem/temp usage and failures. Fsync after exit remains untimed and visible.
New code or measurement scripts need fresh seals, calibration and formal runs.

## Current work

The active batch is `target/surpass-d3d8edc/`. A diagnostic trace established
88 initial runs for the frozen 86M-record BAM at 512 MiB/fan-in 64. The original
schedule rewrites all 86M records during reduction. A candidate rewrote 24,492,300
records while retaining full validation in the original groups. Eight exploratory
invocations produced identical outputs, but paired before/candidate medians were
only 1.0067 and 1.0047 under one/four-core budgets, with mixed single-core results.
This was not a formal or upstream comparison; the candidate was withdrawn. Its
source, checks, seal and complete screen remain in the work directory.

Coordinate radix sorting and the borrowed-quality encoder adapter are retained
provisionally. Radix uses stable passes over fixed-width coordinate fields, then
sorts names/input indices inside equal-coordinate groups. Its worst case remains
O(n log n), with the finer bound O(n + sum(g log g)); scratch adds at most five
usize values per record beyond returned indices. Each candidate's separate
16-invocation native before/after screen passed complete byte comparisons. The
combined source passed both full repository gates and 20 samtools comparisons.
These gates predate subsequent QC and compact-chunk changes.

A temporary BGZF compressor reuse/coalesced-write experiment passed correctness
checks but had no consistent performance benefit and was withdrawn completely.
Its complete source, successful checks and mixed timing results remain retained.

A compact initial BAM chunk candidate now stores small ordinary-encoder-normalized
frames, retaining original records for large frames or deferred serialization
errors. It reuses the full public decoder and preserves legacy chunk accounting,
spill boundaries and merge scheduling. SAM output keeps owned records. All 16
exploratory before/after outputs matched exactly. Against the radix/quality
baseline, median before/current ratios were 1.1236 (random 10M, one core), 1.2153
(four cores), 1.1317 (ordered 10M), and 1.0656 (100k with small spills). Maximum
observed single-process RSS on random 10M fell from about 715 MiB to 292 MiB.
These two-block, no-warmup observations are not formal upstream acceptance or
compliance with the expanded aggregate resource envelope. Subsequent validation
passed 144 valid, 32 malformed and 16 nonfinite-value invocations, including long
CIGARs, varied auxiliary data, SAM/BAM output and multi-level merges. Fresh full
gates and 20 sealed samtools comparisons also passed. A later header investigation
found existing loss of `@CO` comments and stale `GO`/`SS` metadata after sort; that
separate correctness fix requires its own updated seal and acceptance.

FastQC now applies the pinned per-sequence GC prefix window and puts duplication
count 50 in the correct legacy bucket. Of 35 discriminating complete-module
comparisons, 32 match exactly and three retain known sampling differences. An
independent finite-population proof and invocation of the unchanged installed
upstream method demonstrate a separate extrapolation shortcut defect. Sampling,
estimator assumptions and whole-report parity remain open; no new compatibility
exception or status change has been made.

This objective is not complete. Expanded case manifests, populations, latency and
resource controls, FastQC comparability, and many performance deficits remain open.
Protected `rust-umitools/` source remains untouched; its measurement coverage does
not override the repository's explicit-maintainer-authorization requirement.
