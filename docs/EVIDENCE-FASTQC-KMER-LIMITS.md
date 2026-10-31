# Native FastQC limits and Kmer Content

Reference: FastQC 0.13.0, source tag `v0.13.0`, commit
`da2674c2687d1e3296b34a696421b33b93cc8123`. The executable is pinned by the CI
preflight and by the dedicated differential suite. The native implementation uses
Rust's standard library; no dependency, Java runtime or upstream delegation was
introduced.

## Configuration and computation

`-l/--limits` now reads module switches and all warn/error settings. A custom
file overlays FastQC's hard-coded defaults, which differ from its bundled file:
custom files enable Kmer Content unless disabled explicitly and default
`quality_sequence` to warn=20/error=27. The bundled file uses warn=27/error=20
and disables Kmer Content. These surprising defaults are preserved and tested.

All eleven optional modules can be hidden independently; Basic Statistics is
always present. Shared statistics still support duplication when overrepresented
sequences are hidden. `sequence_length` warn/error values are binary switches:
zero disables the corresponding test. Other comparisons retain the upstream's
strict inequality. Duplicate settings use the last value. Unknown keys are
inert. Invalid levels/numbers and extra fields retain upstream diagnostics.
CR/LF/CRLF, comments, leading whitespace, decimal suffixes, hexadecimal floats,
NaN and Infinity follow the pinned parser. Rows that abort upstream configuration
loading are instead rejected before input processing with exit 3.

The Kmer module samples eligible reads 50, 100, 150, ... after length filtering,
uses their first 500 bases, and counts literal seven-base words excluding windows
containing N. It uses the complete sampled opportunity counts for each position,
FastQC's base grouping, a positional binomial null model, the `4^7` correction,
and the existing candidate filters (corrected p < 0.01 and observed/expected > 5).
At most twenty enriched words are displayed, ordered by decreasing enrichment,
with lexical ties. Threshold comparison and ranking use exact integer ratios;
continued-fraction comparison avoids overflowing cross products. The verdict uses the most significant qualifying word before
display truncation, as the limits file specifies. Report fields retain the
FastQC table schema and binary32 numerical display; verdict calculation retains
log probabilities even when a displayed probability rounds to zero.

The baseline 22-fixture oracle does not establish this functionality. A new
`tests/oracle/fastqc_kmer_limits_diff.py` generates separate inputs and preserves
raw observations from both executables. Existing fixtures and oracle expectations
are unchanged. The `--limits` and Kmer matrix rows are explicit exceptions because
of the independently established corrections below. They have empty
`unachievable` cells: these are implemented computations.

## Independently reproduced upstream errors

### Sampling scale and exclusive tail

Let every read be seven A bases followed by 99 C bases. There are 100 seven-mer
opportunities per read; `AAAAAAA` occurs once at position 1. For 200 identical
reads, four reads are sampled. The motif has sampled count 4 and background
probability 4/400 = 0.01. The full input contains exactly 200 occurrences.

Upstream reports Count=20 because `ResultsTable` multiplies by 5 despite sampling
one read in 50. Native Count=200 uses the actual sampling interval. This is also
verified against a full-input literal count, not only the estimator formula.

For position 1, all four of four trials succeed. An inclusive binomial upper tail
is `Pr[X >= 4] = 0.01^4`, and the corrected p-value is:

```text
4^7 × 0.01^4 = 0.00016384
```

The native row is:

```text
AAAAAAA  200  1.6384E-4  100.0  1
```

FastQC computes `1 - cumulativeProbability(4)`, which is `Pr[X > 4] = 0`, and
reports zero. With just two sampled reads, the correct corrected p-value exceeds
one and does not qualify at all, while upstream still reports a zero-probability
enrichment. The suite reproduces both cases. Its independent probability model
sums binomial masses using 80-digit Decimal arithmetic; Rust instead uses a
log-space recurrence.

### Opportunity denominators and input order

Construct sampled blocks from the short read `C^9 A^7` and long read
`C^9 A^7 G^90`, repeating each block 200 times. Each sampled read contains
`AAAAAAA` beginning at position 10. Its group is positions 10–11. There are
8 hits but 12 opportunities: the four sampled short reads reach only position
10; the four sampled long reads reach both positions. Global word frequency is
8/440. Correct enrichment is therefore `8 / (12 × 8/440) = 36.666...`.

FastQC limits the opportunity sum to the size of the word's own position array.
When the short block comes first, the array ends at position 10; later hits at
the same position do not grow it. FastQC counts only eight opportunities and
reports enrichment 55. Reversing the blocks initially allocates the full array
and changes the result. Native counters retain the global opportunity domain and
produce identical output for both block orders. Separate correct and defective
models must reproduce both native and upstream reports.

### Verdict selection and numerical underflow

The upstream limits file says the kmer filter uses the most significant kmer's
`-log10(p)`. The implementation instead uses the first row after sorting by
enrichment. The variable-length counterexample above has tied enrichment scores
but differing p-values: FastQC's first row has p about `5.5685e-5` and yields
`warn`, even though other reported words have p=0 in its own table. Native uses
the most significant qualifying word and produces `fail`; the result cannot
change with hash iteration or display truncation. Upstream tie order is checked
semantically: all displayed rows must have independently valid values, the count
must be correct, order must be nonincreasing in enrichment, and every selected
word must rank at or above the twentieth score. Native lexical ties are checked
exactly.

The display-truncation control has 2,500 reads, yielding fifty samples and
54 qualifying words. Its first twenty displayed words have p about 0.008798
(each only a warning), while `TAAAAAA` at display rank 31 has p about
`8.04306e-87`. The module must fail. A validator control rejects the warning
that would result from calculating significance only after truncation.

A further regression uses 8,150 identical reads of `A^7 C^99`. The corrected
probability is `4^7 × 0.01^163 = 1.6384e-322`. Computing the raw probability first
underflows before correction. Native retains its log probability; limits
warn=320/error=323 yield `warn` because significance is about 321.78558, even
though the binary32 report field is `0.0`. This tests the native numerical
implementation independently of FastQC's already erroneous exclusive tail.

### Partial configuration loading

This file requests disabling sequence content and adapters:

```text
sequence ignore 1
kmer ignore
adapter ignore 1
```

FastQC catches `ArrayIndexOutOfBoundsException` while reading the second line,
then exits successfully with a report. Sequence content is absent, but Adapter
Content is still present: the last requested override was never loaded. Native
exits 3 with a three-field diagnostic before reading input or creating a report.
A process regression uses a nonexistent input to establish validation precedence.
The exception covers failing closed on structurally truncated configuration;
recoverable diagnostic rows retain their upstream behavior. Missing paths and
directory paths are rejected with the pinned launcher diagnostic and exit 1.

## Adjacent native fixes

Custom-limit tests exposed two existing discrepancies. Grouped per-base quality
must average position means equally, and percentile averages must include only
positions with more than 100 observations. A fixture with 101 ten-base reads and
100 eighty-base reads has Q0 at position 10 and Q40 elsewhere. FastQC's `10-11`
row has mean 20 and median/lower quartile 0, producing `fail`. The old native
implementation pooled means and propagated the position-11 NaN, hiding the
failure. Native now agrees with the pinned executable.

FastQC uses the upper middle length for an even number of reads. Lengths 80 and
84 yield Median Length=84; the old native code chose 80. The implementation and
its incorrect inline assertion were corrected from executable evidence. Empty
FASTQ records now remain valid FASTQ (identified by the separator, not nonempty
qualities), allowing sequence-length error switches to execute. Empty sequences
are excluded from the GC histogram as upstream specifies. Length-distribution
rows now follow its interval selection and end-padding rules. The single-observation
GC calculation preserves its infinite-scale density arithmetic. When filtering
removes every read, quality modules are omitted, duplication retains its defined
100% deduplicated total with NaN histogram percentages, and empty position tables
do not fail merely because a configured threshold is negative.

Two additional native arithmetic controls enforce exact enrichment decisions.
Eighteen sampled `AAAAAAACCCC` reads and two sampled `TTTTTTTCCCC` reads give
exact enrichment 5 for the A-containing words, which must not qualify. The
intermediate native binary rounded this to 5.000000000000001 and falsely failed.
Integer-ratio comparisons now agree with the exact Fraction model and pinned
output. A second fixture varies word counts while keeping enrichment exactly
100, requiring lexical ordering even when the original floating calculations
produced 99.99999999999999 versus 100.0. The previous binary is a failing negative
control for the fivefold case.

## Validation and resource scope

```sh
cargo test -p rust-fastqc --locked
python3 tests/ci/test_kmer_proof.py
python3 tests/oracle/fastqc_kmer_limits_diff.py \
  --rust target/release/rust-fastqc --report /tmp/bioinfo-kmer-report.json
bash tests/ci/check.sh
bash tests/ci/check.sh --with-fastx-qc-oracles
```

The final dedicated suite passed **141 cases**: **111 exact configuration or
selected-module comparisons**, **27 independently proved Kmer corrections**,
**one partial-configuration rejection**, and **two exact launcher preflight
rejections**. Exact labels refer to the checked configuration surfaces, module
verdicts and selected module blocks; they do not assert whole-report equivalence.
All native Kmer blocks are checked against the independent model, and every
upstream Kmer block must also satisfy the separately reconstructed defect model.

After the exact-ratio fixes, both required workspace gates exited **0**. Actual
final output:

```text
QUALITY PASS (base only; external oracles NOT RUN)
PASS FastQC limits/kmer: 141 cases; {'exact': 111, 'proved-kmer-correction': 27, 'proved-config-rejection': 1, 'exact-preflight-rejection': 2}
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
```

Logs: `/tmp/bioinfo-kmer-acceptance-base.log` and
`/tmp/bioinfo-kmer-acceptance-oracles.log`, with matching `.status` files containing
0. These are the final implementation runs; earlier `kmer-final*` logs predate
some corrections. The gates include formatting, clippy with denied warnings,
locked release tests/builds, rustdoc, matrix/docs validation and the pinned
oracles. Existing FASTX 50/50, SeqKit correctness 434, QC 22/22 with MultiQC,
adapter 44-case and tile 53-case suites remain passing. Native FastQC has 64
library tests and five new limits process tests; nine Kmer proof controls and
the separate twelve CI-harness controls pass.

Raw count/tail, order and display-truncation proof reports are retained at
`/tmp/bioinfo-kmer-count-tail-proof.json`, `/tmp/bioinfo-kmer-order-proof.json`
and `/tmp/bioinfo-kmer-truncation-proof.json`. The old tile release fails the new
limits suite because it rejects `-l`; a separate negative control records its
incorrect grouped-quality PASS in `/tmp/bioinfo-kmer-quality-negative.json`.
The intermediate floating-ratio binary fails the exact-fivefold case in
`/tmp/bioinfo-kmer-rational-negative.log`.

The matrix has 266 rows: 189 verified, 9 experimental, 57 unsupported and 11
exception. Two rows retain approved JVM `unachievable` constraints. The earlier
tile commit `1c7ee2b` passed remote quality run `41192242981`; that run does not
validate this later batch.

The new suite checks all ignore controls, default/custom fallback, threshold
boundaries, sequence-length switches, parsing and diagnostics, sampling boundaries,
short and N-only reads, case, grouping, 500-base truncation, top-twenty ties,
length filtering, compression, data/summary/ZIP propagation and extract/delete.
Nine proof controls reject wrong scale, exclusive tails, altered enrichment or
position, missing/duplicate/reordered rows, incorrect verdicts, nonfinite values,
inclusive fivefold cutoffs and significance derived only from displayed rows. Process regressions cover configuration precedence, missing files,
independent duplication, per-file sampling state and binary length switches.

Kmer accumulation uses O(W × P + P) counters, where P <= 494 positions and W is
the number of distinct observed seven-mers. Canonical A/C/G/T input has at most
4^7 words; other accepted literal symbols can increase W. Reporting adds O(W)
candidates. Input remains streamed, but the existing per-position, tile and
distinct-sequence structures still contribute to total QC memory. Limits parsing
holds the configuration text. Ignoring a module does not necessarily remove
shared statistics required by another module. No speedup or total constant-memory
claim is made. Full upstream HTML/ZIP plots and auxiliary resources remain open.
