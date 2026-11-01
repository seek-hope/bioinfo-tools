# Scale experiment completed; delivery in progress

The original pipeline has finished. Do not launch another formal experiment or
repeat calibration. Native and measured sources still match the sealed build.
The historical checkpoint is preserved in STATE-before-completion.md.

- Formal report: formal/report.json, passed=true, 28 conditions, 224 invocations
  (168 measured / 56 warm-ups), 112 paired checks. All specified checks passed.
- Started 2026-11-01T12:19:10.636636Z; completed 2026-11-01T18:05:08.187394Z.
  formal.status=0; formal-summary.status=0; pipeline.log ends PIPELINE COMPLETE.
  PIDs 1824844 and 1950536 have exited.
- Minimum measured duration 14.759773495985428 s; no sub-10-second observations.
- inputs_unchanged, runtime_payloads_unchanged and identities_unchanged are true.
  audit-formal.py independently recomputed all medians and paired ratios and
  rehashed all 151 identity files. formal-audit.json/log preserve the result.
  runtime-routes-after-formal.json confirms the unchanged selected cache routes.
- FastQC is descriptive: each of 8 paired checks has 9/11 equal data modules;
  GC and duplication differ. No speed ratio / whole-report equivalence claim.
- Full acceptance gates were executed before timing after all native/control
  edits; accept-base.status=0 and accept-oracles.status=0. Sealed FX104/104 and
  cutadapt119/119 passed. No native source edits since that acceptance.
- Qualifying calibration2 had all56 observations >=14.700760072999401 s;
  calibration1 and earlier calibration/instrumentation history remain separate.
- The README rewrite pause was between timed children, 13:41:34–14:10:26 UTC.
  readme-pause.json, readme-ci.json and commit dacff08 retain that history.

## Current reporting work

- Root English/Chinese READMEs now contain accepted performance data. Only their
  performance sections changed; preserve Argon High's other four sections.
- New bench/SCALE.md and bench/SCALE-REPRODUCTION.md contain methods, complete
  tables, resource observations, limitations and reproduction instructions.
- Main results: gzip trimming ratios1.12/1.83/1.28; lint1.23/1.11; UMI14.98/14.54
  for its frozen graph. Synthetic stats, flagstat and BAM sort remain slower.
  Plain-output trimming/conversion show wide variation and low effective CPU use.
- BAM sort4 median91.61s versus23.18s; native effectiveCPU1.21, maxRSS678.5MiB,
  sampledtemp1.966GiB. Do not claim complete sorting parallelism or an RSS cap.
- archive.py has not yet run. It excludes root README review candidates and
  includes executed snapshots plus support sources, raw logs and audit files.
- Remaining: finalize archive README, create exclusive bench/results/scale-7a7ba28,
  final documentation/Mermaid/link/matrix/integrity checks, regenerate SHA manifest,
  review/stage/commit/push the optimization + measurement batch, verify remote CI.
- Standing authorization to commit/push remains. No release/tag is requested.
  No rust-umitools source changes are in this batch; protected fixtures stay intact.

Native baseline: 7a7ba282acaa96f453200164cae14ab8c12b436c.
Current HEAD/origin before this delivery: dacff08c38860c5a3868c14e83d80fea1b71ef1d.
