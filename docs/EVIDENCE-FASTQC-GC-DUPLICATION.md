# FastQC GC windows and duplication boundaries

This is an active compatibility investigation for the broader performance work.
It does not establish whole-report equivalence or a FastQC speed ratio.

## Observed native discrepancies

Six deterministic FASTQ populations were run against the pinned FastQC 0.13.0 and
the frozen native quality-slice build. Native `--stdout` and reference report data
were retained in full. No performance comparison was made between these output
modes. The reference checkout is
`da2674c2687d1e3296b34a696421b33b93cc8123`; executable identities, commands, input
hashes, record counts, complete modules and original reference ZIP/HTML are in the
active work directory `target/surpass-d3d8edc/qc-probes2/`. This work has not yet
been published as an immutable evidence archive.

The first probe attempt is retained separately in `qc-probes/`: its reference
invocation exited 2 because the 0.13.0 launcher accepts `--outdir`, not `-o`.
No failed observation was replaced. The second attempt completed all six cases.

| Input | Frozen native result | FastQC 0.13.0 result |
|---|---|---|
| One `A^100 G^50` read | GC bins 33: 0.5; 34: 1/3 | GC bins 0: 1.0; 1: 0.5 |
| One `A^150` read | GC bin 0: 0.5 | GC bins 0: 1.0; 1: 0.5 |
| Fifty identical 150 bp reads | 100% in `>50` duplication bin | 100% in `>10` bin |

`PerSequenceGCContent.truncateSequence` deliberately limits model lengths:
lengths above 1000 use a leading multiple of 1000 bases; lengths above 100 use a
leading multiple of 100; shorter reads use the full sequence. The reference
comments explain that this limits cached models. This is intentional approximation,
not evidence of an upstream defect. The native implementation now follows this
rule for **per-sequence GC distribution only**. Total bases, global GC counts,
length distributions and per-position statistics continue using the complete read.
The existing model weights are unchanged and are not normalized to sum to one.
Floating-point increments still occur individually in input order.

The native cache regression test previously assumed a full-length model for every
read. Its input population is preserved, and its expected model lengths now name
the reference prefix lengths explicitly. Additional 100/1000 boundary lengths
retain the bitwise ordered-sum check. This deliberately changes the former native
GC output to match the verified reference contract; no oracle fixture, golden
output or external comparison rule was edited.

`DuplicationLevel` assigns bins from `count - 1`. The legacy label `>10` covers
counts 10 through 50; `>50` begins at 51. The native implementation's old comment
already listed 50 in `>10`, but its range ended at 49. The range is corrected, with
checks on all duplication-bin boundaries. This does not change admission,
extrapolation or exhaustive-count behavior.

## Remaining sampling and extrapolation differences

The reference admits the first 100,000 distinct sequence prefixes and thereafter
counts only known prefixes. The native implementation currently retains all
prefixes. Two probes distinguish admission from repeat counting:

- 100,000 distinct prefixes followed by two copies of a new prefix: reference
  deduplicated percentage 100.0, native 99.9990000199996.
- The same initial population followed by a repeat of the first prefix and one new
  prefix: reference 99.9990000099999, native 99.9990000199996.

These differences do not by themselves prove an upstream defect. One result is a
sampling estimate and the other uses exhaustive counts. They remain a barrier to
claiming equal-work whole-report performance.

A separate source-level formula discrepancy has a larger discriminating case:
100,000 initial distinct prefixes, repeats of the first 1,000, then 50,000 new
prefixes. There are 151,000 reads and 150,000 distinct prefixes. The reference
reports 98.88525763023519% deduplicated; exhaustive native counting reports
99.33774834437085%.

For the reference's stated fixed-size inclusion formula, with N total reads, K
sampled reads, multiplicity d and m observed distinct prefixes in that class,

```
p_miss = C(N - d, K) / C(N, K)
corrected_class_count = m / (1 - p_miss)
```

The reference's early impossibility condition uses `N - m < K`; the stated event
is impossible when `N - d < K`. In the large probe's singleton class,
N=151000, K=100000, d=1 and m=99000. The unshortened formula gives exactly 149490,
while the early return gives 99000. This difference exceeds the separate intended
0.01-observation approximation cutoff. Exact rational identities are retained in
the probe report.

A method-level probe now invokes the private method in the installed, unmodified
reference class through Java reflection. It reproduces both wrong early returns
(99999 instead of 100000.99998, and 99000 instead of 149490); no-sampling and
impossible-to-miss controls pass. Class bytes were hashed before and after.
An independent Python proof enumerates 440 small fixed-size sampling cases,
reparses all six FASTQ populations, checks both CLIs' record counts, and confirms
the original native exhaustive deduplication percentages. It detects both wrong
early returns beyond the stated 0.01-observation cutoff. Artifacts are under
`target/surpass-d3d8edc/duplication-method/`.

This algebra does **not** establish unbiasedness for the adaptive admission rule,
nor does it make the extrapolated count equal the known population count.
Estimator assumptions, floating-point order and a native sampling implementation
still need to be addressed before classifying an approved correction. No new
compatibility exception or status change is made here.

## Validation status

The native GC-window and count-50 changes passed release component tests and
all-target Clippy. A fresh sealed build was checked on the six original inputs,
13 additional GC lengths around the 100/1000 boundaries, one mixed-length input,
and 15 duplication-bin boundaries: **32 complete module-data comparisons are
exact; three sampled-duplication comparisons remain open**. The first six reuse
hash-verified frozen reference outputs; the other 29 execute the reference afresh.
All module names, order, statuses and data lines are compared. The verifier and
raw outputs are in `target/surpass-d3d8edc/qcprefix-probes/`.

The previous full repository gates passed before these QC source changes; they
are not fresh acceptance of this QC candidate. Report graphics, HTML/ZIP parity
and sampled duplication remain unfinished, so FastQC performance observations
remain descriptive.
