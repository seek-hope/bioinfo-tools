# bioinfo-tools monorepo

This directory is a Cargo workspace and the canonical Git repository for all
workspace members.

## Repository boundary

`rust-cutadapt/` and `rust-umitools/` were originally independent Git
repositories. Their histories are preserved outside the monorepo at:

```text
../bioinfo-tools-historical/rust-cutadapt.git
../bioinfo-tools-historical/rust-umitools.git
```

They are **historical archives**, not the build source. The monorepo contains
the current source for every crate:

| Path | Contents |
| --- | --- |
| `bioinfo-common/` | shared exit codes, diagnostics, summary JSON and provenance |
| `bioinfo-fastx/` | shared streaming FASTX/compression/BGZF layer |
| `rust-cutadapt/` | cutadapt-compatible crate |
| `rust-umitools/` | UMI-tools-compatible crate |
| `rust-fx/` | FASTX manipulation CLI |
| `rust-bam/` | SAM/BAM tooling on noodles |
| `rust-fastqc/` | FastQC-compatible QC prototype |

A fresh checkout of this repository is self-contained; no sibling crate
repositories are required.

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

The workspace MSRV is Rust 1.91 (set by `rust-bam`/noodles); the primary
`rust-toolchain.toml` pins the latest stable, currently 1.99.1.

## Evidence

Per-step evidence and known gaps are in `docs/EVIDENCE-*.md` and the matching
JSON files. The development plan remains in
[docs/DEVELOPMENT-PLAN.md](docs/DEVELOPMENT-PLAN.md).
