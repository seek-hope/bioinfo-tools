# Follow-up lead: reduce the amount of intermediate merge work

This is a static finding, not an executed performance result or an implemented
change. The current batch is frozen and its original formal process is running.
Do not edit its source/plan/artifacts or restart it to pursue this lead.

`rust-bam/src/commands.rs::reduce_runs` currently merges every nonsingleton group
in a full level whenever the run count R exceeds fan-in F. For example, at R90,
F64, it merges groups64 and26, then merges those two outputs. All records pass
through an intermediate read/full-decode/level1-compress cycle. At R65,F64, the
first64 runs undergo that cycle and the last run is forwarded unchanged.

A candidate queue schedule can reduce only enough runs to fit the final fan-in:
merge the first g=((R-1)%(F-1))+1 runs (use F when g==1), append the resulting run
to the queue, then repeatedly merge F queue entries until at most F remain.
For R90,F64, g27 reduces the population directly to64: only27 initial runs are
rewritten before the final merge. For R65,F64, g2 is sufficient. A weighted
schedule might reduce bytes further, but weights/error/resource contracts need
careful design. Do not claim weighted optimality for an unweighted FIFO queue.

Tradeoffs and checks before implementation:
- Final merging may compare more keys because it has more active heads; this is
  an I/O/decoding/compression versus selector-work tradeoff. Quantify it.
- Keep the existing limit on simultaneously open runs (each run also has an
  index file), full BAM decoding, global input-index tie break, index EOF policy,
  final refill-before-output and intermediate write/index-before-refill order.
- Reordering internal merges can change the order of failures in corrupted
  temporary runs. Review the declared partial-output policy and add discriminating
  tests; do not silently weaken diagnostics or public malformed-input checks.
- Keep lifecycle/cleanup/sync behavior; never delete inputs before their merged
  output and sidecar have completed their required finish/sync/flush operations.
- Exercise R around F,2F,F^2, empty/short runs, unequal record sizes and capture
  threshold transitions. Compare complete native bytes across budgets/workers.
- Derive or measure actual initial run counts on the86M workload before attributing
  its timing to this mechanism. No run-count instrumentation has been executed
  for this lead. The source examples above are arithmetic examples, not a claim
  that the measured workload produced exactly90 runs.
- New source changes need fresh gates, seal, calibration and complete formal
  measurements in a separate directory; preserve this batch's pending evidence.

The accepted selection tree also has a fixed height based on the original slots.
After most runs end it does no comparisons against empty leaves but still repairs
that path. A one-active-run fast path is another profiling lead, not a current
performance claim.
