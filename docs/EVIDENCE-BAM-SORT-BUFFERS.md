# Sort buffer candidates and a rejected storage experiment

These experiments follow the quickcheck correctness seal. All three candidate
changes described below were withdrawn: zero-compression temporary runs regressed,
and the two buffer changes did not demonstrate stable whole-command gains across
the tested CPU budgets. Their source, binaries and evidence remain preserved.
All 129 runtime hashes were restored to the quickcheck seal. No dependency or
compression backend was added; no upstream superiority target is established.

## Sequential coordinate histograms

The preserved candidate `radixscan-source/rust-bam/src/radix_sort.rs` counted each
active coordinate byte by scanning the
cached position tuples in storage order. A histogram is invariant under the
permutation of those tuples. The stable scatter still traverses the current
index order, so all coordinate bytes, name comparisons and global input-index
ties keep their original semantics. The small-input, already-ordered and strictly
descending fast paths are unchanged. Complexity and scratch allocation are
unchanged: fixed-width coordinate work plus comparison sorting within equal
positions, at most five extra `usize` values per record beyond returned indices.

The added full-key differential test includes byte carries, sparse varying
bytes, high bits and `usize::MAX`, duplicate names and input indices unrelated to
vector offsets. The candidate passed 128 release BAM tests, Clippy, 144 native
CLI pairs spanning diverse valid and malformed input, a held-open stdin failure,
both full repository gates, and the 20-case pinned BAM oracle. Evidence:
`target/surpass-d3d8edc/radixscan-{source,sealed}/`,
`radixscan-candidate-gates.json`, `radixscan-errors/`, `radixscan-diverse/`.

## Direct append of normalized frames

The preserved candidate `directarena2-source/rust-bam/src/sort_chunk.rs` let the
existing capped encoder sink own the whole
chunk arena. A successful ordinary BAM encoding appends directly to that arena,
removing the second per-frame copy into a separate arena. The sink records the
start of each attempted frame. On error or oversized output, it rolls back only
that append, retains earlier frames, releases the private encoding buffer and
keeps the original record for serialization at its original sorted emission.
SAM storage, decoding, encoding normalization, chunk estimates, spill boundaries,
merge fan-in and final compression remain unchanged.

The sink holds the four-byte length prefix separately until the size check
accepts it. Review of the first candidate found that appending even a rejected
prefix could grow the entire arena before rollback. An executable control using
the actual first-candidate sink reproduced growth from 131072 to 262144 bytes;
the corrected sink passed the same unchanged-capacity assertion. It also preserves
bytes across multiple full-size frames, split prefixes, a rejected append and a
subsequent valid frame. The first candidate and its interrupted gate remain in
`directarena-source/`, `directarena-sealed/` and
`directarena-review-transition.json`. The corrected candidate is separately sealed
under `directarena2-*`; the capacity proof is `arena-capacity-proof/`.
The corrected source passed 129 release BAM tests and Clippy, 72 diverse valid
CLI pairs and 72 malformed/boundary CLI pairs, the held-open stdin control, both
full repository gates and the 20/20 pinned BAM oracle. Final 129 runtime and
12 validation-source identities were rechecked unchanged; see
`directarena2-candidate-gates.json`. The focused prefix-buffer follow-up review
found no remaining concrete defect in that change; it was static inspection,
not independent execution.

The 64 KiB limit applies to an individual cached frame, not the whole arena,
private encoder peak allocation or process RSS. Total memory still includes the
largest arena capacity with `Vec` growth slack, keys, fallback records, decoder
buffers, radix indices and merge/compression state. Failed oversized encodings
can allocate privately before their prefix is exposed. The corrected prefix
handling prevents that rejected prefix from growing the retained arena.

## Instruction diagnostics, not elapsed improvements

Each pair used the same first 100000 records prepared from the frozen 86M BAM,
the sealed native executables, one thread and a 512 MiB sort estimate. Full output
bytes and diagnostics matched. Callgrind ran with resource observation, while
other host work was active; its instrumented elapsed times are not speed evidence.
The whole-CLI instruction counts are diagnostic observations, including startup,
I/O and differing artifact paths:

| Isolated change | Baseline instructions | Candidate instructions | Reduction |
|---|---:|---:|---:|
| Sequential histogram versus quickcheck seal | 1181102589 | 1179448671 | 0.1400% |
| Corrected direct arena versus histogram seal | 1179439211 | 1178863030 | 0.0489% |

These small reductions do not establish useful elapsed gains. Reports are
`radixscan-profile2/report.json` and `directarena2-profile/report.json` under the
active evidence directory. Do not multiply these changes into a cumulative gain
or pair them with historical upstream timings. The first profile attempt is
retained in `radixscan-profile/`: default Valgrind debugger FIFOs triggered the
strict ordinary-artifact resource check. The retry disabled unused vgdb IPC;
resource accounting was not weakened.

## Rejected zero-compression temporary runs

A separately sealed prototype used level-0 BGZF for initial and intermediate
runs, keeping final level-6 output. All 12 complete outputs matched on the frozen
86M input. In two paired exploratory blocks at each budget, median baseline /
candidate time was 0.7318 at one core, 0.7647 at two cores and 0.8330 at four cores;
every individual pair was slower. Peak observed logical files reached about
21.34 GiB. The prototype was withdrawn and all 129 prior runtime hashes were
restored before the histogram candidate was applied.

The screen had no warmup, calibration or upstream arm. Resource checks were
sampled observations, not OS caps; inputs and previous batch outputs were outside
each invocation workspace. The subsequently observed four-core Java verification
job started after this screen ended, as recorded in `external-load-observation.json`.
This does not establish an otherwise exclusive host. Full evidence is retained
in `stored-{source,sealed,large}/` and `stored-decision.json`.

## Completed buffer screen and withdrawal

The first attempt, `sort-buffers-preflight1/report.json`, detected a competing TLC
job and started zero measurements. On 2026-11-04, a new preflight found no TLC,
build or profiling job. Desktop processes remained active; this was not an
exclusive host. `sort-buffers-screen2/` completed all 18 runs on the same 86M BAM,
512 MiB estimated chunks and fan-in 64. Three native arms used 1/2/4 physical-core
budgets and two reversed-order blocks. Reversal leaves the middle arm in the
middle, so this is partial counterbalancing, with no warmup or calibration.

The median of the two within-block elapsed ratios is shown below. Above 1 favors
the denominator. The combined ratio is calculated directly, not by multiplying
stage summaries.

| Cores | Baseline / histogram | Histogram / direct arena | Baseline / both changes |
|---:|---:|---:|---:|
| 1 | 1.0301 | 1.0304 | 1.0564 |
| 2 | 1.0059 | 1.0500 | 1.0573 |
| 4 | 0.9453 | 1.0560 | 0.9922 |

Each comparison includes mixed pair signs except the two-core arena increment.
Thus neither the histogram change nor the combined changes showed a stable gain
across budgets. The arena increment's favorable medians do not establish the
performance of an unmeasured arena-only build. Both micro-optimizations were
withdrawn rather than keeping complexity based on noisy marginal evidence.
This small exploratory screen does not prove that they can never be beneficial.

The post-run audit rechecked all 18 raw specs/events/identities, complete artifact
inventories, final output hashes, diagnostics, cleanup, resource samples and
paired ratios. All outputs matched the complete native byte anchor. Largest
sampled aggregate RSS was 322031616 bytes; largest logical workspace 2110042672
bytes; minimum filesystem headroom 132199989248 bytes. Retained final BAM outputs
totaled 2264473494 bytes, outside later individual batch workspaces. Recorded
telemetry reached 102050 millidegrees Celsius and maximum sampling gap was about
1.001 seconds. Temperature/frequency/desktop activity are limitations, not a
proved explanation of individual timing differences.

See `sort-buffers-screen2-audit.json`, `sort-buffers-decision.json` and the
executed scripts under `target/surpass-d3d8edc/`. The restored runtime is separately
rerun through both repository gates. A next experiment explicitly tests a larger
`--max-memory` argument to avoid an intermediate merge level; defaults and the
original 512 MiB condition remain unchanged.
