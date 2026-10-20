# bioinfo-tools workspace

This directory is a Cargo workspace and an **overlay Git repository**.

## Repository boundary

The workspace contains three repositories:

| Path | Owner | Contents |
| --- | --- | --- |
| `bioinfo-tools/` (this root) | overlay repository | root `Cargo.toml`/`Cargo.lock`, shared crates, tests, evidence and workspace documentation |
| `rust-cutadapt/` | independent repository | cutadapt-compatible crate and its own history/remote |
| `rust-umitools/` | independent repository | UMI-tools-compatible crate and its own history/remote |

The root `.gitignore` excludes `rust-cutadapt/` and `rust-umitools/` so their
commits stay in their own repositories. To build from a fresh checkout, clone
those two repositories beside this root; the workspace paths are relative.

## Workspace members

- `bioinfo-common` — shared exit codes, diagnostics, summary JSON and provenance
- `bioinfo-fastx` — shared streaming FASTX/compression/BGZF layer
- `rust-fx` — FASTX manipulation CLI
- `rust-bam` — SAM/BAM inspection and manipulation on noodles
- `rust-fastqc` — FastQC-compatible QC data report prototype
- `rust-cutadapt` and `rust-umitools` — independent repositories described above

## Build and test

```bash
cargo test --workspace --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
```

The oracle binaries for the FASTX comparisons are pinned in
[tests/ci/PINS.md](tests/ci/PINS.md). They are downloaded locally, not committed.

The workspace requires Rust 1.91 because `rust-bam` uses noodles 0.118.0.

## Evidence

Per-step evidence and known gaps are in `docs/EVIDENCE-*.md` and the matching
JSON files. The development plan remains in
[docs/DEVELOPMENT-PLAN.md](docs/DEVELOPMENT-PLAN.md).
