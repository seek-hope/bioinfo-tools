# bioinfo-tools workspace rules

This is the current monorepo policy. The original October 2026 cutadapt trial
brief is historical: see `TASK.md` and `docs/DEVELOPMENT-PLAN-LEGACY.md`. Its
single-end subset, no-network rule, `clap`+`flate2`-only dependency rule and
12-hour trial window do not describe the current workspace.

## Network and dependencies

- Network access, downloads and package installation are allowed when needed.
  Prefer existing dependencies. A new direct dependency needs a recorded reason,
  version, source and build/runtime impact (an ADR or a plan note).
- Build with `--locked`; `Cargo.lock` is authoritative for reproducible builds.
  Use `--offline` only when intentionally testing an isolated/cached environment;
  it is not a prerequisite for normal development or CI.
- Do not silently replace an unavailable dependency or format with an approximate
  implementation. Reject the capability or record the deviation.

## Scope and protected artifacts

- Workspace members are `bioinfo-common`, `bioinfo-fastx`, `rust-bam`,
  `rust-cutadapt`, `rust-fastqc`, `rust-fx` and `rust-umitools`. Each CLI keeps
  its own upstream argument, exit-code, report and determinism contract.
- Do not modify `rust-umitools/` without explicit maintainer authorization. Its
  component documentation may contain stale statements; record them in
  `docs/DOC-AUDIT.md` instead of editing that crate.
- Do not edit oracle fixtures, expected output files or comparison harnesses to
  make a test pass. Fix the implementation or document a deliberate deviation.
- Preserve pre-existing uncommitted packaging changes unless the task explicitly
  owns them.

## Quality and resources

- Keep FASTX processing streaming where the algorithm permits. State the actual
  memory bound, including per-position and per-distinct-sequence structures;
  streaming input does not imply bounded total memory.
- Default to strict input handling. Unsupported capabilities fail before output
  with the documented exit code. A stream that fails after writing records exits
  non-zero and documents its partial-output policy.
- Before claiming completion, run `bash tests/ci/check.sh` and
  `bash tests/ci/check.sh --with-fastx-qc-oracles` when the pinned references are
  installed, plus the component-specific oracle suites that apply. Quote real
  command output; a previous evidence file is not a rerun.
- Output must not depend on chunk size or worker count unless the upstream
  contract says otherwise. Performance claims need a correctness result on the
  same input, hardware, thread count and compression level.
- Respect an explicit task deadline when one is set. Do not publish, tag or
  commit unless explicitly asked; releases require maintainer approval and a
  complete evidence set.
