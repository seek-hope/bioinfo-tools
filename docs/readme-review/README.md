# Bilingual README collaboration and review

The user selected local benchmarking and roadmap option C: prioritize completion
of existing tools, retaining interval/expression analysis and VCF/BCF processing
as future candidates without delivery dates. They authorized collaboration with
Gemini. The web model picker showed **4 Argon** / **4 Argon Complex tasks**.

Conversation: <https://gemini.google.com/app/ef630ce2ca87df02> (account access may
be required; this is not a publicly shared conversation link).

## Received contributions

- [Design review](argon-design.txt): critique of hybrid-core affinity, descendant
  processes, thermal effects of validation, cache/output scope and UMI workload
  structure; proposal for matched bilingual sections.
- [Bilingual draft](argon-draft.txt): English and Chinese project introduction,
  workflow/command examples, compatibility boundaries and roadmap. This is a raw
  UI text extraction; nested Markdown in the generated draft rendered poorly.
  It is retained as review material, not used verbatim as executable instructions.
- [Results-based review](argon-results.txt): checked wording around the measured
  ratios, FastQC scope, source patches and lint memory. Its speculative attribution
  of conversion/UMI differences primarily to startup was not adopted: these runs
  do not isolate startup or algorithm costs. Comparing 1M real with 10M synthetic
  data changes both size and input; the report also compares real/synthetic at 1M.

The implementation adopted CPU affinity for the command and descendants, explicit
compression, seeded paired order, warm-input preparation, pauses and telemetry,
nonuniform UMI bundles with mutation neighbors and known representatives, and
separate FastQC reporting. Tmpfs and the observed 44–100°C formal-run package
temperatures are explicit limitations. Fixed pauses do not establish thermal
control or isolated CPU resources.

The final READMEs retain the proposed matched structure, then use verified CLI
syntax and measured values. The draft's generic UMI single-core description and
placeholders for unmeasured `extract`/`view`/filter performance were removed.
The measured revision is the baseline plus the two saved source corrections,
not unchanged `aaf4ba4`. UMI extraction precedes adapter trimming in the connected
UMI example, and the external aligner must preserve the resulting read names.

## Evidence and implementation review

Argon supplied editorial/design advice and prose, not empirical validation.
All numbers come from the executed scripts and raw results linked in
[the benchmark report](../../bench/RESULTS-README.md). The bilingual performance
tables are generated from the same JSON values. Real records are public ENA data;
UMI performance uses synthetic data and does not establish real-library scaling.

A separate read-only code review found no observed invalidation of the completed
runs, and prompted these fixes before delivery:

- Document the source patch, tmpfs storage and nominal/final BAM bundle scales.
- Reject misspelled or inapplicable case selections and verify the planned matrix.
- Refuse pilot/incomplete-success data in five-repeat tables; retain failed QC
  warm-ups instead of crashing during rendering.
- Close/reap BAM verification subprocesses on success and exceptions.
- Describe lint as matching validation verdicts, and FastQC as checking displayed
  total bases rather than an exact unrounded total.

Ten benchmark evidence controls cover changed inputs/indexes, stale evidence,
changed outputs, case selection, pilot/failed summaries, raw QC differences,
missing BGZF EOF and coordinate-tie canonicalization. Production code changes
are limited to the independently reproduced Q20/Q30 and `fq2fa` defects. Historical
benchmark/oracle evidence was retained, and missing compatibility work was not
relabelled as complete. The [example execution](example-execution.json) records
13 successful CLI calls on four synthetic reads, ending with two independently
expected deduplicated records. A small SAM fixture stands in for external
alignment; no external aligner was tested.

The final workspace gate re-linked `rust-fx` with a different dependency feature
set. Source hashes were unchanged, and repeating the original per-package build
reproduced the measured binary hash exactly. Both the build distinction and the
correct reproduction command are recorded in the benchmark report.

[Final local validation](validation/README.md) links both complete quality-gate
logs, boundary checks for both build variants and the portable staging smoke run.
