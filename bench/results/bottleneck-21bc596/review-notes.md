# Review and candidate decisions

A read-only source review found no concrete correctness defect in the serial
candidate. It checked integer carry bounds, quality thresholds, floating-point
order, selector tie order, inactive leaves, full decoding and refill ordering.
This was source review, not an independent execution or exhaustive proof.

Two suggested coverage additions were implemented and passed in the final gates:
- Mixed small/over-64-KiB/small records through actual intermediate and final
  merges, with SAM/BAM outputs, fan-in2/64 and final compression workers1/4.
- Per-record quality merges across mixed probability magnitudes and the65535
  integer-lane boundary, checking the aggregate floating-point bits after each
  record against the scalar implementation.

The review also hypothesized a new retained output RecordBuf, based on an
inaccurate baseline description supplied to it. Direct git-diff inspection showed
that the record/frame swap and oversized uncached-record release statements are
unchanged. That hypothesis is not an accepted finding.

Candidate1 was selected after component tests, instruction profiles and32 matched
screening invocations with identical output bytes. The final sealed binaries are
byte-identical to candidate1; later changes added regression coverage only.

Only then was candidate2 evaluated: parallel level1 compression for temporary
runs, using the existing noodles ordered multithreaded writer. Its first build
failed because the noodles alignment Write trait was not imported; the log is
retained. Candidate2b fixed that import and added output-failure coverage. Its
component tests and clippy passed, and all32 screening outputs matched candidate1.
The 2/4-core10M timings showed no improvement; small-run measurements also did not
support adopting it. The prototype was reverted and its sealed binaries, exact
source snapshots, build/test logs and observations remain in this work directory.

Each temporary writer in the prototype created a thread pool and queued blocks.
Those are real implementation costs, but this experiment did not isolate their
individual causal contribution. Do not present the result as a general claim
that parallel compression is slower.

The screening script uses subprocess.run(timeout=...), whose wait polling affects
subsecond elapsed resolution. The1M stats and100k small-run timings are candidate
screening, not calibrated speed claims. Formal upstream/current measurements use
fresh qualifying calibration and the existing scale protocol. No timings were
selected or discarded to produce a favorable subset.
