# Argon High README rewrite — 2026-11-01

The English and Chinese READMEs were drafted in Gemini with **Argon** and
**Thinking level: High**. The user's requested section order is quoted verbatim
in [request.txt](request.txt) and [request-high.txt](request-high.txt).
The model picker was checked before sending the High request and again after
its response completed; [model-settings.json](model-settings.json) records the
visible selections.

The first response used Low, before the user specified High. It is retained in
[response-low.txt](response-low.txt). The final rewrite uses the subsequent
[High response](response-high.txt), with its original [English](draft.en.md) and
[Chinese](draft.zh-CN.md) file contents preserved separately. Links inside these
source drafts are relative to the repository root.

The review corrected the clone command's Markdown link formatting, separated
UMI grouping from deduplication, described the twelve QC modules as implemented
rather than always enabled, and separated expression analysis from analyses
that consume BAM files in the diagrams. It also shortened repetitive wording
and limited the pending benchmark table to the workloads actually being
measured. Input-size and runtime cells remain blank. CPU columns describe the
allocated core budget.

Validation passed for both Mermaid diagrams with mermaid 11.12.0 and jsdom
26.1.0, installed in a temporary directory. Documentation constraints, local
links, five-section order, matching bilingual shell examples, Bash syntax and
empty pending measurement cells were checked. The command smoke used the
retained binaries from `7a7ba282acaa96f453200164cae14ab8c12b436c`: 13 commands
passed, processing four reads and producing two UMI representatives in both
read-name and BAM-tag modes. The alignment stage used a supplied synthetic SAM
fixture. Results are in [smoke.json](smoke.json) and [validation.log](validation.log).
