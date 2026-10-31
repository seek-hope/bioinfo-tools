# Native FastQC tile statistics and matrix constraints

This batch follows the pushed `c3c7477` adapter/translation corrections. The user
requested continuing compatibility closure and adding a matrix column named
`unachievable`. Native tile computation is achievable and is implemented here;
its deliberate missing-data correction is a separate approved exception.

## Matrix column

Matrix schema 3 adds a text column `unachievable`. Empty means no such constraint
has been established; it does not mean every remaining task has been proven
feasible. A nonempty cell states the approved constraint and points to its proof.
At present only FastQC `-j/--java` and `--debug` have values, because equivalent
JVM executable selection / Java-launch diagnostics conflict with the selected
pure-Rust runtime. Their existing status stays `exception`.

This column is independent of implementation status. A proven upstream bug,
unfinished work, complexity, or missing tests cannot populate it. The generator
uses the registered native-runtime constraints, and the validator requires exact
agreement with those reasons, proof/verifier paths and explicit exception rows.
It rejects empty exception surface lists. Nine policy controls cover the existing
rules and attempts to hide or invent constraints. No row is removed.

## Implemented tile contract

Reference: FastQC 0.13.0 executable and source tag `v0.13.0`, commit
`da2674c2687d1e3296b34a696421b33b93cc8123`.

`rust-fastqc/src/tile.rs` accumulates per-position counts and quality sums using
standard Rust. It needs no external executable or new dependency. It implements:

- Modern seven-field and legacy five-field Illumina headers; subsequent headers
  use the first detected field position. Signed 32-bit tile identifiers include
  Java-compatible BMP decimal digits; malformed/overflow IDs disable the optional
  module, as upstream does. Detection is independent for each input file.
- The first 10,000 eligible, nonempty reads and every tenth read thereafter.
  Length filters run before sampling and header detection. Unsampled reads do not
  extend the tile position domain or alter header detection.
- The pinned parsing heuristic: 2501 distinct IDs are allowed; the next new ID
  disables the module and emits the upstream warning. Ordinary non-Illumina
  inputs omit this optional module without affecting other statistics.
- Per-position means, equal-position means within groups, and equal-tile means
  across observed tiles. Relative deviations use strict warning/error thresholds
  above 5 and 10, respectively. Observed Q0 remains a real observation.
- Sorted tile IDs, grouped/ungrouped positions, Phred33/Phred64, compression,
  module ordering, `fastqc_data.txt`, summaries, ZIP data and native HTML data.
  Invalid retained Phred64 bytes below ASCII 64 exit 1 before native output.

Kmer Content and `--limits` remain unsupported. Custom module enablement and
thresholds must be completed with the Kmer implementation; they are not silently
accepted. The full upstream plot/HTML/ZIP resources remain experimental.

## Missing observation is not a measured zero quality

Create two reads, both with Q20 (`5` in Phred33), from tiles 1 and 2:

```text
@I:1:F:1:1:0:0
ACGT
+
5555
@I:1:F:1:2:0:0
ACGTACGT
+
55555555
```

Run the pinned executable and native binary in separate output directories:

```sh
fastqc --quiet --extract --outdir reference reads.fq
rust-fastqc --quiet --extract --outdir candidate reads.fq
```

At positions 5–8, tile 1 has **no observation**, while tile 2 has Q20. The
upstream's `PerTileQualityScores.getMean` returns zero for a group with no
observations. `getPercentages` includes that zero in its tile average, reporting
relative means -10 and +10 and a `warn` verdict. It has fabricated Q0 measurements
and penalized the actually observed Q20 tile.

Rust emits `NaN` for tile 1's unobserved groups, excludes them from the reference
mean, and emits deviation 0 for tile 2. The verdict is `pass`. This is the
registered `FASTQC-TILE-MISSING-QUALITY` exception, not a raw-equivalence pass.
The same result holds when the two reads are reversed. A separate observed-Q0
control confirms that genuinely measured zero quality is retained and affects
both deviations and the verdict.

The oracle's independent model uses declared tile labels and exact rational
means over the actual quality observations. Every native result is checked
against that model, including presence/absence, status, labels, rows and values.
Finite values allow only 1e-12 absolute floating-point error; their textual form
must obey Java-style formatting, including exact `NaN`. For each corrected case,
a separate zero-imputation model must also reproduce the upstream defect.
The raw differences stay visible in the JSON report.

## Corrected native grouping and number formatting

The former grouping implementation and an inline unit assertion incorrectly
claimed that 75 positions were already grouped. Actual pinned execution yields
75 individual rows, while the prior binary produced 42 rows. The threshold is
now corrected: grouping starts above 75.

For larger intervals, the pinned grouping grid has another boundary: a 20-base
interval begins `1, ..., 9, 10-19, 20-39, ...`, not `10-29, 30-49, ...`.
Equivalent alignment is needed for intervals 50 and 100. The prior implementation
missed this special first group. A 660-base fixture with low quality at positions
10–19 demonstrates the effect: upstream's first merged per-base-quality row is
`10-19` with mean 10; the previous binary reports `10-29` with mean 15, diluting
the local drop. The old binary has 42 rows versus the reference's 43.

The new suite directly compares shared quality, base-content, N-content and
adapter tables at boundary lengths, including 660, 1400 and 4000 bases. The
independent model constructs intervals on the aligned position grid. The wrong
inline unit expectation was corrected from executable evidence; existing oracle
fixtures and comparison scripts were not changed.

A three-tile fractional-mean fixture also exposed native decimal formatting:
`-0.000000000000000013877787807814457` instead of the reference's
`-1.3877787807814457E-17`. `java_double` now observes Java's scientific-notation
boundaries, with regressions for small values, large values and negative zero.
This changes representation, not the computed value.

## Executable evidence

```sh
cargo test -p rust-fastqc --locked
python3 tests/ci/test_tile_proof.py
python3 tests/oracle/fastqc_tile_diff.py \
  --rust target/release/rust-fastqc \
  --report /tmp/bioinfo-tile-report.json
```

The current generated suite has **53 cases**: **48 exact tile comparisons**,
**four independently proven missing-observation corrections**, and **one invalid
Phred64 rejection** (both programs exit 1; native output remains absent).
It covers header formats and Unicode IDs, score boundaries, unequal tile counts,
roundoff, missing positions, grouping, filters, sampling, compressed input,
tile-count limits, artifacts, and `--extract --delete`.

Artifact checks require ZIP data and summary to equal the extracted files. All
native HTML variants must agree and contain the complete tile table, including
missing values. This validates data propagation, not upstream plot completeness.
Seven proof controls reject fabricated zeros, hidden observations, noncanonical
values, wrong status, missing/duplicate rows and broken report envelopes.
Five native tile unit tests and four process regressions cover the computation,
sampling, Unicode integer bounds, input filtering, per-file isolation, malformed
tails and invalid Phred64. Grouping and formatting have additional unit tests.

The old `c3c7477` release binary fails the new suite at `modern-headers` because
it has no tile module. Direct grouping negative controls also reproduce both
prior grouping mistakes. Logs/artifacts:

- `/tmp/bioinfo-tile-diff.log`, `/tmp/bioinfo-tile-report.json`
- `/tmp/bioinfo-tile-negative-control.log`
- `/tmp/bioinfo-tile-grouping-negative.json`

Final local workspace validation for this batch ran:

```sh
bash tests/ci/check.sh
bash tests/ci/check.sh --with-fastx-qc-oracles
```

Both exited **0**. Their final lines were `QUALITY PASS (base only; external
oracles NOT RUN)` and `QUALITY PASS (base + FASTX/QC oracles)`. The new oracle
reported 53 tile cases with 48 exact comparisons, four corrected missing-data
cases and one invalid-Phred64 rejection. The full gate also retained the
previous 50/50 FASTX, 434 SeqKit correctness, 22/22 QC and MultiQC results.
The matrix check passed with 266 rows: 189 verified, 9 experimental, 59
unsupported and 9 exception. Two rows have nonempty `unachievable` constraints,
both approved JVM launcher exceptions.

The final gate logs are `/tmp/bioinfo-tile-gate-final2-base.log` and
`/tmp/bioinfo-tile-gate-final2-oracles.log`, with `.status` files containing 0.
These results cover the uncommitted tile working tree; the earlier pushed
adapter commit's remote run remains `41186678371`.

## Resource and scope limits

Counter storage is O(sum of the longest sampled read length for each retained
tile), bounded above by O(T × L), with T at most 2501 before the heuristic disables
the module. Reporting adds O(T × G) means and text, where G is the number of
groups (up to L with `--nogroup`). Existing per-position quality, GC and
distinct-sequence structures also contribute to total QC memory. This is not a
constant-memory or process-RSS guarantee, and no new speed claim is made.

The original strict FASTQ input and project resource limits remain in effect.
Whole-CLI diagnostic byte equivalence is not claimed: for invalid Phred64,
native diagnostics are concise rather than Java stack traces. Full custom limits,
Kmer statistics and report plots remain explicit unfinished work, not
`unachievable` capabilities.

## Pinned source references

- [PerTileQualityScores.java](https://github.com/s-andrews/FastQC/blob/da2674c2687d1e3296b34a696421b33b93cc8123/uk/ac/babraham/FastQC/Modules/PerTileQualityScores.java)
- [QualityCount.java](https://github.com/s-andrews/FastQC/blob/da2674c2687d1e3296b34a696421b33b93cc8123/uk/ac/babraham/FastQC/Utilities/QualityCount.java)
- [BaseGroup.java](https://github.com/s-andrews/FastQC/blob/da2674c2687d1e3296b34a696421b33b93cc8123/uk/ac/babraham/FastQC/Graphs/BaseGroup.java)
