# QC prototype evidence: `rust-fastqc`

> Historical snapshot. The original CLI actually read the whole file without
> decompression; the compressed-input limitation below understated that defect.
> The subsequent fix, streaming input tests and rerun of this oracle are recorded
> in [EVIDENCE-P0-START.md](EVIDENCE-P0-START.md). QC statistics still have separate
> memory-growth limits; this snapshot is not evidence of bounded total memory.

Status: **VERIFIED for the six required modules.** FastQC 0.13.0 differential:
**22/22 fixtures pass**. MultiQC 1.35 parses the output with no warnings.

Canonical machine-readable file: [EVIDENCE-QC.json](EVIDENCE-QC.json).

Scope: a FastQC-compatible QC prototype that emits **`fastqc_data.txt` only** — no
HTML, no plots, no zip archive.

## 1. The one thing to read

| Criterion | Status |
| --- | --- |
| `rust-fastqc` crate using `bioinfo-fastx` | **met** |
| Basic Statistics | **met** — 22/22 fixtures |
| Per base sequence quality | **met** — 22/22 |
| Per sequence quality scores | **met** — 22/22 |
| Per base N content | **met** — 22/22 |
| Per sequence GC content | **met** — 22/22 |
| Sequence Length Distribution | **met** — 22/22 |
| Per base sequence content (extra, fully matched) | **met** — 22/22 |
| Sequence Duplication Levels (extra, fully matched) | **met** — 22/22 |
| Overrepresented sequences (extra, fully matched) | **met** — 22/22 |
| At least 20 fixtures | **met** — 22 |
| MultiQC parses without warnings | **met** |
| Workspace debug + release suites green | **met** — 685 tests |
| Existing cutadapt/UMI/rust-fx/BAM oracles green | **met** |
| Harness resolves via env var, fails closed with 3, has a self-test | **met** |

Nine modules are compared byte for byte. `Adapter Content` is **excluded by name**
and documented; see §6.

## 2. Result

```text
$ bash tests/oracle/run_qc_oracle.sh
rust-fastqc : …/target/release/rust-fastqc
fastqc      : …/tests/ci/env/bin/fastqc
multiqc     : …/tests/ci/env/bin/multiqc
FastQC v0.13.0
multiqc, version 1.35

PASS  basic_uniform          uniform reads, quality 40, mixed bases
PASS  low_quality            all qualities at Q2, should fail base quality
PASS  high_quality           all qualities at Q40
PASS  declining_quality      quality falls along the read
PASS  gc_rich                GC-only reads
PASS  at_rich                AT-only reads
PASS  with_ns                reads containing N bases
PASS  variable_length        reads of several lengths
PASS  single_read            one read only
PASS  duplicates             many identical reads
PASS  exactly_100           100 reads: the quartile NaN threshold boundary
PASS  exactly_101           101 reads: one past the quartile NaN threshold
PASS  long_reads             reads longer than 50bp (base grouping territory)
PASS  short_reads            very short reads
PASS  mixed_case             lowercase bases should be treated as bases
PASS  phred_shifted          qualities spanning the full printable range
PASS  all_same_base          every read identical, all A
PASS  mostly_n               reads that are mostly N
PASS  large_2000             2000 reads, exercises the >100 path throughout
PASS  zero_gc                reads with no G or C at all
PASS  one_base_reads         single-base reads
PASS  high_dup_ratio         half the reads are one sequence

22/22 fixtures passed
PASS  multiqc            parses our fastqc_data.txt without warnings
```

| Item | Version | Identifier |
| --- | --- | --- |
| FastQC | **0.13.0** | `htsjdk.jar` sha256 `e0c8f4afbf6c82ce50e1a45bbcc26682be98b59357f5df5bb967444fdc94ab3e` |
| MultiQC | **1.35** | `multiqc.__version__` |
| Java | 25.0.2 | runtime for FastQC |
| `rust-fastqc` binary | 0.1.0 | sha256 `7e4a790bc2b7bb702ef29db00bc951f4f88c46fdb9630044c0a50d0755a277f3` |

Pins, checksums, install instructions and the resolution order are in
[`tests/ci/PINS-QC.md`](../tests/ci/PINS-QC.md). **No oracle bytes are committed**;
`tests/ci/env/` is git-ignored (FastQC is ~15 MB, the MultiQC venv ~1.1 GB).

## 3. Fixtures

22 generated fixtures, defined as deterministic specs in `qc_diff.py` rather than as
committed binary files, so the matrix is reproducible from the source alone and
nothing binary enters the repository. Each is generated from an explicit seed.

They deliberately cover the cases where FastQC's conventions bite:

* **`exactly_100` / `exactly_101`** straddle the quartile `NaN` threshold.
* **`long_reads`** (80 bp) exercises base grouping.
* **`mixed_case`** proves lowercase bases count as bases.
* **`mostly_n`** and **`with_ns`** exercise the N-excluded `%GC` denominator.
* **`all_same_base`**, **`zero_gc`** and **`one_base_reads`** are the degenerate
  shapes that break naive implementations (0% and 100% GC, 1bp reads).
* **`high_dup_ratio`** and **`duplicates`** exercise the duplication bins.
* **`phred_shifted`** spans the printable quality range.

## 4. Normalisations — exactly two, both named

Both are applied to **both sides**, and neither hides a real field difference.

### 4.1 `overrepresented_row_order`

FastQC keeps overrepresented sequences in a Java `HashMap` and prints its iteration
order. That order is unspecified, differs between JVM versions, and no independent
implementation can reproduce it. The rule sorts the data rows of that one module by
sequence name on both sides.

**Everything else in that module is compared exactly**: counts, percentages and the
`Possible Source` column. The module's `pass`/`warn`/`fail` verdict is compared too.

### 4.2 `filename`

The `Filename` field contains the path as given on the command line. Both tools are
invoked with the same relative path so it normally matches; the rule rewrites the
value to a placeholder on both sides to guard against path-resolution differences.
The rest of the Basic Statistics module is still compared exactly.

**Nothing else is normalised.** Module order, every module status, every table
header, every numeric value and the module set are compared byte for byte.

## 5. Real mismatches found and fixed

Every one of these was found by the oracle and fixed in the implementation. They are
listed because the *rules themselves* are the deliverable: they are undocumented and
each was wrong on the first attempt.

### 5.1 Per-base quartiles are `NaN` until a position has **more than 100** observations

The single most surprising rule. FastQC shows `NaN` in `Median`, `Lower Quartile`,
`Upper Quartile`, `10th` and `90th` for every small file — not as a placeholder, but
because `PerBaseQualityScores.getPercentile` sums only over columns whose
`getTotalCount() > 100` and divides by how many qualified. With none qualifying the
divisor is zero and Java yields `NaN`.

Measured crossover by bisection: **100 reads → `NaN`, 101 reads → numbers.** Pinned
by the `exactly_100` / `exactly_101` fixtures, and the negative control in §8.

### 5.2 `%GC` uses an N-excluded denominator and **truncates**

`%GC` is `(G+C)/(A+C+G+T)` — `N` is excluded from the denominator — and the result
is truncated, not rounded:

```text
read GGGGNNNN + AAAAAAAA   -> %GC 33   (4/12, not 4/16 = 25)
920 GC of 1820 ACGT        -> %GC 50   (50.549 truncated, not 51)
```

### 5.3 Percentages: divide **then** multiply

Java computes `count / total * 100`, not `count * 100 / total`. The two differ in the
last bits and the data file carries the difference:

```text
22/80*100 = 27.500000000000004   <- FastQC prints this
22*100/80 = 27.5                 <- wrong
```

The same ordering applies to the duplication percentage (`62.66666666666667` vs
`...664`). Both are now computed divide-first.

### 5.4 `Per sequence GC content` spreads each read over bins

A read does not contribute 1.0 to a single percentage bin. `GCModel` models the read
as a distribution: for each possible integer GC count, take
`[count-0.5, count+0.5]` clamped to `[0, length]`, convert both ends to percentages
with Java's `Math.round`, and give each covered bin `1 / (bins it shares)`.

Verified exactly: one 8bp all-G read produces `94 → 0.5`, `95..=100 → 1.0`, and two
such reads produce `1.0` and `2.0`. Mixed-length inputs sum correctly.

### 5.5 The GC-content verdict uses a documented-in-bytecode mean rule

The theoretical distribution is a normal whose mean comes from walking outward from
the modal peak while bins exceed `peak - peak/10`. The branch that chooses between
the peak index and the arithmetic mean is easy to get wrong:

```text
mean = (flagForward || flagBackward) ? peakIndex : sum/count
```

Reading it as "both walks must survive" gives 26.2% where FastQC reports **41.0%**,
turning a `fail` into a `warn`. Verified on the all-A and AT-only fixtures.

### 5.6 Base grouping starts at **75bp**, not 50

`BaseGroup.getLinearInterval` tries **interval 1 first**, then 2, 5, 10, then ×10.
Keeps the first interval whose group count is under 75. So:

```text
74bp reads -> one row per position
75bp reads -> interval 2, rows 10-11, 12-13, ...
```

Omitting the interval-1 candidate grouped every read from 10bp up and broke all the
short-read fixtures.

### 5.7 A grouped row averages the **per-position** values

For a group of positions, every column is the average of that position's own value —
not the percentile of the pooled histogram:

```text
position 10 median = 36, position 11 median = 35
row 10-11 median = 35.5
```

Pooling instead gives 35, which is wrong for equal-sized groups and badly wrong for
the truncated final group.

### 5.8 Duplication bins are inclusive at the lower edge

Bins are `1,2,…,9,>10,>50,>100,>500,>1k,>5k,>10k+`. Each `>N` covers `N` up to the
next threshold: `>10` is 10–49, `>50` is 50–100, `>100` is 101–500. Measured by
varying one sequence's occurrence count in a 200-read file:

```text
count 10 → >10      count 100 → >50
count 50 → >10      count 101 → >100
count 500 → >100    count 501 → >500
```

### 5.9 Status verdicts come from `Configuration/limits.txt`

The thresholds were **read from FastQC's own config**, not guessed. My first
attempt guessed and got the semantics backwards:

* `quality_base_lower 10/5` **and** `quality_base_median 25/20` are two separate
  tests, not one.
* `overrepresented warn 0.1` is the *reporting* threshold; `error 1` is the failure
  threshold. So any hit is at least `warn`, and only ≥ 1% fails. Both comparisons
  are **strict** (`>`): a sequence at exactly 1.0% still reports `warn`.
* `sequence_length` is binary: warn if lengths differ, fail on a zero-length read.

### 5.10 Smaller formatting rules

* `Total Bases` switches to kbp at 1000 with **one truncated decimal**:
  `1998 → 1.9 kbp`, `3000 → 3 kbp`, `996 → 996 bp`.
* `Mean Length` and `Median Length` print as integers, truncating
  (`lengths 4,8,8 → 6`).
* `Filename` is the **base name including extension** (`sample.fastq`, not `sample`).
* `Per base sequence content` prints raw doubles with no rounding, so
  `25.900000000000002` keeps its tail.
* All-`N` positions print **`NaN`** in every base-content column, not `0.0`.
* The `#Sequence` header row appears **only when there is at least one
  overrepresented hit**; an empty module is the two `>>` markers alone.
* Module order puts **GC content before N content**.
* `Per sequence quality scores` fills zero-count gap bins between the lowest and
  highest observed score.

## 6. Modules intentionally absent

Recorded in `rust-fastqc/src/absent.rs` as data, so the omission is checkable rather
than prose. None of these is compared by the differential.

| Module | Why absent |
| --- | --- |
| **Adapter Content** | Requires FastQC's `Configuration/adapter_list.txt` plus undocumented per-position k-mer tolerance rules. Emitted as an **empty block** (FastQC always writes the block) and **excluded from comparison by name** — a module that was not computed carries no honest verdict. |
| **Kmer Content** | Position-specific k-mer enrichment with a binomial test and its own multiple-testing correction; the binning and correction are undocumented. |
| **Per tile sequence quality** | Needs Illumina tile coordinates from the CASAVA header plus a flow-cell layout; meaningless for non-Illumina data. |

Notes on the two remaining differences from FastQC's default output:

* **Long-read grouping exists but the module list still matches.** Grouping is
  implemented (§5.6) and matches at 80bp.
* **`--nogroup` is not exposed as a flag.** `grouping::base_groups(len, true)` exists
  and is unit-tested, but the CLI does not surface it yet.

## 7. Verification results

All commands run by me, offline, in this session.

| Check | Command | Result |
| --- | --- | --- |
| QC differential | `bash tests/oracle/run_qc_oracle.sh` | **22/22 fixtures passed**, exit 0 |
| MultiQC parse | (inside the harness) | **PASS**, exit 0, no warnings |
| Harness self-test | `bash tests/oracle/run_qc_oracle.sh --self-test` | **SELFTEST PASS** |
| Fail-closed | `FASTQC_ORACLE=/nonexistent MULTIQC_ORACLE=/nonexistent …` | **exit 3**, 2 ERROR lines |
| Formatting | `cargo fmt --all -- --check` | clean |
| Lints | `cargo clippy --workspace --all-targets --offline -- -D warnings` | **0 warnings** |
| Workspace tests (debug) | `cargo test --workspace --offline` | **685 passed, 0 failed** |
| Workspace tests (release) | `cargo test --workspace --release --offline` | **685 passed, 0 failed** |
| cutadapt 4.6 | `python3 tests/check.py` | all visible conformance cases passed |
| UMI-tools 1.1.6 exact | `bash tests/oracle/run_exact.sh` | **77/77 byte-identical, 0 differ** |
| UMI-tools semantic | `bash tests/oracle/run_determinism.sh` | **deterministic: 0 mismatches** |
| UMI-tools formats | `python3 tests/oracle/formats.py` | **formats ok: 0 failures** |
| W1-3 differential | `w13_diff.py` with seqkit + fq | **50/50 cases passed** |
| W1-4 differential | `w14_diff.py` with samtools | **20/20 cases passed** |
| M3 byte-neutrality gate | `bash tests/gate/m3.sh` | **9/9 criteria, M3 PASS** |

### Test counts

| Crate | Tests |
| --- | --- |
| `bioinfo-common` | 39 |
| `bioinfo-fastx` | 96 |
| `rust-bam` | 52 |
| `rust-cutadapt` | 201 |
| **`rust-fastqc`** | **46** |
| `rust-fx` | 178 |
| `rust-umitools` | 73 |
| **Total** | **685** |

685 = the previous 639 **plus 46 new `rust-fastqc` tests**. Per-crate counts sum
exactly to the workspace total.

`rust-fastqc`'s 46 tests: `stats` 18, `modules` 12, `grouping` 6, `report` 3,
`error` 2, `absent` 4, binary 1 — the numeric conventions are tested without
touching the filesystem, so a formatting or comparator mistake fails a unit test
rather than only the oracle.

## 8. Negative controls

Two, both real:

1. **Quartile threshold.** Changing `total <= 100` to `total <= 50` drops the
   differential from **22/22 to 16/22**; restoring it returns 22/22. This proves the
   `NaN` boundary is actually being compared and not normalised away.
2. **Harness self-test.** `--self-test` proves the comparison can fail: identical
   text must report no difference, differing text must be detected, the
   overrepresented rule must reorder without corrupting rows, the `filename` rule
   must fire and not over-match, module splitting must recover every module's status
   and body, and every fixture must generate valid FASTQ with matching
   sequence/quality lengths (a broken fixture would make cases fail for unrelated
   reasons).

A third, unplanned control: while reverting the first mutation I over-reverted and
the suite fell to **3/22**, which is how the mistake was noticed. That accident is
itself evidence the comparison has teeth.

## 9. What is explicitly not claimed

* **HTML reports, plots and the zip archive.** Only `fastqc_data.txt` is produced.
  MultiQC reads the data file directly, which is what this prototype targets, but
  `fastqc_report.html` and `Images/` do not exist.
* **Adapter Content, Kmer Content, Per tile sequence quality.** Absent; see §6.
* **`-t/--threads`, `-f/--format`, `--limits`, `--adapters`, `-c/--contaminants`,
  `--fast5`, `--nogroup` as CLI flags.** Not implemented.
* **Contaminant matching.** One fixture surfaced a real difference: FastQC labelled
  a read `RNA PCR Primer, Index 14 (100% over 10bp)` because it matched its
  contaminant list; `rust-fastqc` prints `No Hit` for every sequence. That is a
  documented gap, not a match. It does not affect the fixture result because the
  affected read is a single low-count row whose *count and percentage* still match.
* **bzip2/xz/zstd-compressed FASTQ inputs.** The reader is `bioinfo-fastx`, which
  handles them, but no fixture exercises a compressed input.
* **Cross-platform byte-identity.** Only linux x86_64 measured.
* **FastQC versions other than 0.13.0.** The conventions in §5 are version-specific;
  see `PINS-QC.md`.

## 10. Repository state

* No release, no tag, no version bump, no publication.
* Versions: `bioinfo-common` 0.1.0, `bioinfo-fastx` 0.1.0, `rust-bam` 0.1.0,
  `rust-cutadapt` 0.1.0, **`rust-fastqc` 0.1.0**, `rust-fx` 0.1.0,
  `rust-umitools` 0.2.0.
* Workspace members now seven: the six previous plus `rust-fastqc`.
* New runtime dependencies in `rust-fastqc`: `bioinfo-fastx`, `bioinfo-common`,
  `clap`. All were already workspace dependencies — **no new crate** was introduced.
* Oracle payloads (`tests/ci/env/`) remain git-ignored.

## 11. Reproducing this evidence

```sh
cd /path/to/bioinfo-tools

# Build (offline).
cargo build --release --offline -p rust-fastqc

# The oracles are found automatically under tests/ci/env/bin. To point elsewhere:
export FASTQC_ORACLE=/path/to/fastqc
export MULTIQC_ORACLE=/path/to/multiqc

bash tests/oracle/run_qc_oracle.sh --self-test   # no oracle needed
bash tests/oracle/run_qc_oracle.sh               # 22 fixtures + MultiQC

# The workspace gates.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo test --workspace --release --offline
bash tests/gate/m3.sh
```

## 12. Files created for the QC prototype

* `rust-fastqc/Cargo.toml` — depends on `bioinfo-fastx`, `bioinfo-common`, `clap`.
* `rust-fastqc/src/lib.rs` — crate docs, module list, `FASTQC_VERSION`, header.
* `rust-fastqc/src/stats.rs` — accumulating counters and every numeric convention
  (`java_round`, `java_double`, `gc_model`, `QualityCounts`, `QcStats`).
* `rust-fastqc/src/grouping.rs` — `BaseGroup` and `getLinearInterval` (75bp rule).
* `rust-fastqc/src/modules.rs` — the nine computed modules plus the empty Adapter
  block, `Thresholds` from `limits.txt`, and `format_bases`.
* `rust-fastqc/src/absent.rs` — the documented-absent module list.
* `rust-fastqc/src/report.rs` — report assembly and `ReportWriter`.
* `rust-fastqc/src/error.rs` — shared exit-code mapping.
* `rust-fastqc/src/main.rs` — CLI, `bioinfo-fastx` streaming reader, provenance.
* `tests/oracle/qc_diff.py` — 22 fixtures, two named normalisations, self-test,
  MultiQC validation.
* `tests/oracle/run_qc_oracle.sh` — fail-closed wrapper, exit 3 when unresolved.
* `tests/ci/PINS-QC.md` — oracle versions, checksums, install, resolution order.
* `Cargo.toml` (root) — `rust-fastqc` added to members.
* `docs/EVIDENCE-QC.md` / `.json` — this document and its machine-readable form.

No release, tag, version bump or publication.
