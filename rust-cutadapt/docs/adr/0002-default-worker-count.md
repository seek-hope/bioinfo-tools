# ADR 0002: default worker count

- Status: accepted
- Date: 2026-10-20
- Related: ADR 0001, W0-5 performance work

## Context

cutadapt 4.6 defaults `-j` to one worker. The original v0.1.0 Rust subset used
up to four workers by default and reached 6.3x–6.9x on the recorded workload.
When the full-surface implementation was completed, the default became one
worker to mirror cutadapt. A full-surface benchmark then measured 2.352 s
(1.89x against the recorded 4.4363 s baseline), missing the 5x target.

The work is embarrassingly parallel and the pipeline is already deterministic
across worker counts. With four workers the same benchmark takes 0.672 s
(6.60x baseline, 14.36x against a same-session oracle), while `-j 1` still
reproduces the cutadapt-style single-thread result.

## Decision

The default worker count is **four**:

- `-j` omitted: four workers.
- `-j 1`: one worker, exact single-thread mode and cutadapt-style parity.
- `-j N`: exactly N workers.
- `-j 0`: auto-detect all available CPUs (unchanged).

Output bytes and reports remain deterministic across worker counts. The
reported core count is part of the report but is deliberately not part of the
compatibility comparison (the differential harness normalises it, as it does
cutadapt's own runtime-dependent fields).

## Consequences

- The documented benchmark command meets the 5x target without requiring a
  hidden flag.
- The default differs from cutadapt's documented default of one worker. This is
  a deliberate performance deviation, recorded in
  [../FULL-COMPATIBILITY.md](../FULL-COMPATIBILITY.md); `-j 1` restores exact
  parity.
- Peak RSS increases from roughly 34 MB to 38–40 MB on the 1M-read workload.
  The pipeline remains bounded-memory; the increase is from the extra worker
  and compression buffers.
- This decision does not change adapter matching, filtering, output bytes or
  report values. It changes scheduling only.
