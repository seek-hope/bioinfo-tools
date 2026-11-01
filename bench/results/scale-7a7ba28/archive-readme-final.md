# Scale comparison evidence after 7a7ba28

The formal experiment completed on 2026-11-01, 12:19:10–18:05:08 UTC. All
28 conditions, 168 measured observations, 56 warm-ups and 112 paired checks
passed their specified validation rules. Final input/runtime/identity rechecks
passed; the shortest measured invocation was 14.759773495985428 s, with none
below 10 s. Upstream/current is the primary comparison. Earlier before/after
measurements remain in the separate `optimization-e125519` archive.

Read the [complete report](../../SCALE.md), [protocol](../../SCALE-PROTOCOL.md)
and [reproduction guide](../../SCALE-REPRODUCTION.md).

- `sealed/build-provenance.json` identifies the exact five native binaries,
  canonicalizer and 122 native source/manifest hashes, plus the locked build
  recipe. `native-source.patch` records the native changes against 7a7ba28.
- `accept-*.log/status` retain newly executed controls and both full gates.
  `final-fx-behavior.*`, `final-cutadapt.*` and
  `canonical-sealed-crosscheck.json` identify sealed component validation.
- `base-calibration/` is preliminary sizing evidence from earlier instrumentation.
  Its executed sources are retained. `smoke-calibration/` exercises the final
  driver on fresh 1M populations; neither is a formal performance result.
- `calibration-*` retain every enlarged calibration. Calibration 1 passed output
  and identity checks but had two current UMI observations and one upstream
  FastQC observation below 10 s. Calibration 2 qualified with all 56 observations at least
  14.700760072999401 s. It increased only UMI to 71M and FastQC to 9M; the
  other nine populations were reused. `formal-plan.json` identifies that report.
- `*-inputs/inputs.json` and `inputs-*/inputs.json` retain plans, source metadata,
  exact repetition counts, file sizes/hashes, reuse provenance and storage
  preflights. Their executed preparation scripts and BAM generation records
  are retained. Initial flagstat size was 755M: the earlier progress-summary 754M
  value was a transcription error, not a change to any input/observation.
- `formal/report.json` contains all 28 conditions, 56 warm-ups and 168 measured
  observations, with 112 complete paired checks, exact commands, orders,
  affinities, runtime/artifact identities, resource samples and final rechecks.
  Its `executed/` directory holds the measurement and runtime support scripts.
  Per-invocation stdout, stderr and GNU time logs are retained.
- `support/` preserves the untimed planning/summary/input/control sources,
  protocol and dependency rationale. These are source snapshots; run the
  reproduction commands from a matching repository checkout.
- `formal-summary/` contains derived timing/resource tables and JSON/TSV. It
  excludes calibration and warm-ups from the three-sample statistics. FastQC
  remains descriptive, with no speed ratio. No formal observation was below 10 s.
- `runtime-routes*.json` verify the staged UMI/FastQC directories resolve to the
  selected cache. `storage-device.txt` supplements filesystem records without
  inferring NAND type or device-cache behavior.
- `phase*`, `trial*`, earlier provenance/source patches and failed control logs
  are developmental evidence. They are not formal performance conclusions.
  Trial 5's `phase4` arm ran with four CPUs because of a suffix-label error;
  its actual command is retained and must not be relabeled single-core.
- `validator-controls-initial.log` preserves the negative-count acceptance bug
  found before these runs; the fixed parser rejects it and all controls pass.
  Compilation/fixture failures in earlier phase logs are also retained.
- `static-review.md`, `next-flagstat-static-review.md` and
  `next-qc-static-review.md` are read-only reviews, not
  executed tests. The latter proposes future GC/duplication probes and changes
  no native implementation or approved exception. Historical checkpoint files
  describe the state at their respective times.
- `audit-formal.py` runs from the
  matching repository root with the original working directory. It independently
  recomputes all 28 time medians and 26 paired ratios and rehashes all 151
  identity files. `formal-audit.json/log` retain its successful execution.
- `readme-pause.json` records the user-requested documentation pause from
  13:41:34 to 14:10:26 UTC, after both timed processes had exited for the
  synthetic-cutadapt-c2 warm-up. No timed invocation was paused. The original
  process group resumed and completed this same experiment. `readme-ci.json`
  records CI for the separate README commit `dacff08`.

Large input/output payloads and executable binaries are not committed to Git.
Their identities, counts, sizes and generation/build recipes are recorded.
The runner compared full outputs before cleanup, retained the first native
payload until every core budget matched, and would retain failing pairs and
anchors on a mismatch. A 64 KiB sort-frame capture threshold bounds only the
added copy; it does not bound total merge memory.

`sha256.json` covers every other archived file. Verify it before recomputing
summaries. Executed driver snapshots preserve the versions used in each run;
`support/` contains the final untimed planning, summary and validation sources.
The large input files and binaries remain reproducible from their recorded
recipes and hashes.
