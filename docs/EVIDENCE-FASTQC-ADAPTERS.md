# FastQC adapter configuration and correctness evidence

## Scope and policy

The user's instruction is: do not reproduce erroneous upstream behavior; prove
that the upstream behavior is wrong and that the native result is correct. The
FastQC runtime remains native Rust. This batch implements `-a/--adapters`, fixes
short-read adapter reporting, and registers the two adapter defects below as
`FASTQC-ADAPTER-CORRECTNESS`. Approved differences are not exact oracle matches.

Reference: FastQC 0.13.0, source tag `v0.13.0`, commit
`da2674c2687d1e3296b34a696421b33b93cc8123`. The executable's version is checked by
the differential. The implementation adds no dependency and does not execute
Java or FastQC at runtime.

## A complete adapter must match an equal-length read

Use a custom adapter list containing `motif<TAB>ACG`, and this FASTQ:

```text
@r
ACG
+
!!!
```

Run FastQC and Rust with `--adapters adapters.tsv --extract reads.fq`, using
separate output directories. The upstream Adapter Content data is:

```text
>>Adapter Content	warn
>>END_MODULE
```

Rust reports the direct substring count:

```text
>>Adapter Content	fail
#Position	motif
1	100.0
>>END_MODULE
```

The proof does not depend on the Rust algorithm: the only possible full-length
start is position 1, and `ACG == ACG`, so one of one reads contains the adapter.
A three-base nonmatching read similarly has one possible start and zero hits.
The default 12-base adapter list exhibits the same defect.

There is also an unambiguous order-invariance counterexample. Use two reads:
`AGATCGGAAGAG` (a complete default adapter) and 25 `T` bases. Upstream reports
**0% / pass** if the equal-length read comes first, but **50% / fail** when the
same reads are reversed. Rust reports **50% / fail** in both orders. Changing
record order cannot change the presence of a literal adapter in these two reads.

The [pinned AdapterContent source](https://github.com/s-andrews/FastQC/blob/da2674c2687d1e3296b34a696421b33b93cc8123/uk/ac/babraham/FastQC/Modules/AdapterContent.java)
updates its stored longest sequence only when the read is longer than the
previous stored value **and** `sequence.getSequence().length() - longestAdapter > 0`.
An equal-length first read leaves
the stored length at zero, and the subsequent position loop never counts its
match. A preceding longer read initializes the array and changes the result.
Rust retains first-occurrence counts independently of the current reporting
range, and applies the final longest-motif boundary when rendering the report.
This includes the equal-length position and fixes the lost match.

A second order-dependent error occurs with `short<TAB>AA`, `long<TAB>CCCC` and
reads `TTTTAA`, `TTTTTTTT`. The final eight-base maximum permits reporting starts
1 through 5; `AA` occurs at position 5 in one of two reads, so the value there
must be 50%. Upstream gives 0% in that order and 50% after reversing the reads:
its first read discards the hit outside the then-current reporting range, and
later expansion cannot recover it. Native first-hit histograms retain it and
produce 50% in both orders, while still clipping the final table to the
longest-motif boundary.

The independent checker in `tests/oracle/fastqc_adapters_diff.py` enumerates
literal substring starts and computes the percentage for each position. It
compares the full native block, including status and column order, to that
model for every valid case with reads up to 74 bases, not merely the six known
counting differences. It checks both record-order counterexamples. The matching
upstream differences remain visible in its JSON report.

## An empty configuration must fail before report creation

An empty file, a comment-only file, or a file containing only malformed rows
leaves upstream's adapter array empty. Upstream subsequently throws
`ArrayIndexOutOfBoundsException` during report construction and exits 1. Rust
rejects the configuration before reading input or creating output, with exit 3
(the workspace input-precondition code) and:

```text
adapter list contains no usable name<TAB>sequence entries
```

The differential requires the pinned upstream exception to reproduce. It also
requires the native exit code and diagnostic and checks that the output
directory remains empty. A process regression uses an unread input path to
verify that configuration validation takes precedence over input access. Rust
retains its short diagnostic rather than reproducing the Java stack trace.

## Matching configuration and report behavior

The dedicated differential compares the entire Adapter Content data block,
the summary verdict, and custom-list parsing diagnostics. Its normal cases
cover default and custom lists, repeated names, UTF-8 names, tab runs, trailing
tabs, LF/CR/CRLF line endings, malformed rows mixed with valid rows, short reads, first-hit cumulative
counts, repeated motifs, variable read lengths, grouped and ungrouped output,
and literal N/case-sensitive custom motifs.

Mixed-length adapters retain upstream's documented longest-motif reporting
boundary and warning: starts too late for the longest configured motif are
excluded even for shorter motifs. Input sequences are uppercased; configured
motifs retain their case. Custom configuration files are UTF-8 text. This
batch does not claim arbitrary non-nucleotide sequence encoding equivalence.

For reads shorter than every default adapter, upstream emits `warn` with no
table. Rust previously emitted `pass` with an invented zero row; this is now
fixed. The historical 22-fixture QC oracle explicitly excludes Adapter Content,
so that older pass count is not adapter verification. Its comparisons and
fixtures remain unchanged; the new dedicated suite covers this gap.

## Reproduction and limits

```sh
cargo test -p rust-fastqc --locked
python3 tests/oracle/fastqc_adapters_diff.py \
  --fastqc tests/ci/env/bin/fastqc \
  --rust target/release/rust-fastqc \
  --report /tmp/bioinfo-fastqc-adapters.json
```

The suite contains 44 cases: 35 exact comparisons, six counting corrections,
and three configuration rejections. Six Rust regressions cover short reads,
equal-length matches/nonmatches and config line endings, both order-invariance
counterexamples, the mixed-length final boundary, and validation before output.
Negative controls run against captured earlier binaries detect both the previous
short-read pass/zero-table error and the intermediate mixed-length counting
error which otherwise agreed with upstream. The latter fails the independent
model at `mixed-growing`, proving equality alone cannot pass the corrected gate.
Logs are `/tmp/bioinfo-adapters-negative-control.log` and
`/tmp/bioinfo-adapters-order-negative-control.log`.
Final whole-workspace validation is recorded in
`docs/EVIDENCE-UPSTREAM-DEFECTS.md`; both required gates passed after these fixes.

Adapter accumulation uses O(A × L + S) additional memory for A configured
adapters, longest read length L, and S bytes of adapter definitions. Existing
QC position and distinct-sequence structures still contribute to total memory;
this does not claim constant-memory QC or a measured performance improvement.

At this batch's boundary `--limits` and Kmer Content were unsupported. The
subsequent [Kmer/limits batch](EVIDENCE-FASTQC-KMER-LIMITS.md) implements them,
including custom adapter thresholds. Per tile sequence quality is implemented;
its observed-only missing-data correction and grouping evidence are recorded in
[EVIDENCE-FASTQC-TILES.md](EVIDENCE-FASTQC-TILES.md). HTML/ZIP plot completeness
remains experimental.
