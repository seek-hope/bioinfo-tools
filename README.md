# bioinfo-tools monorepo

This directory is a Cargo workspace and the canonical Git repository for all
workspace members.

## Repository boundary

`rust-cutadapt/` and `rust-umitools/` were originally independent Git
repositories. Their full histories are archived as release assets in this
repository under tag
[`archive-pre-monorepo-2026-10-21`](https://github.com/seek-hope/bioinfo-tools/releases/tag/archive-pre-monorepo-2026-10-21):
`rust-cutadapt.bundle`, `rust-umitools.bundle` and `dist-assets.tar.gz`.

They are **historical archives**, not the build source. On GitHub the separate
`rust-cutadapt`, `rust-umitools`, `rust-cutadapt-dist` and `rust-umitools-dist`
repositories were consolidated on 2026-10-21; only two repositories remain:

| Repository | Visibility | Contents |
| --- | --- | --- |
| `seek-hope/bioinfo-tools` | private | this monorepo (source of truth) |
| `seek-hope/bioinfo-tools-dist` | public | prebuilt binaries, `install.sh`, checksums |

The monorepo contains the current source for every crate:

| Path | Contents |
| --- | --- |
| `bioinfo-common/` | shared exit codes, diagnostics, summary JSON and provenance |
| `bioinfo-fastx/` | shared streaming FASTX/compression/BGZF layer |
| `rust-cutadapt/` | cutadapt-compatible crate (workspace member) |
| `rust-umitools/` | UMI-tools-compatible crate (workspace member) |
| `dist/` | public distribution payload: unified `install.sh`, README, LICENSE |
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
- `rust-cutadapt` — cutadapt-compatible adapter trimmer
- `rust-umitools` — UMI-tools-compatible UMI workflows

## Build and test

```bash
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Network access and package installation are allowed. Use `--offline` only when
you intentionally need a cache-only/isolated run. The unified quality gate is
`bash tests/ci/check.sh`; add `--with-fastx-qc-oracles` to require installed
FASTX/QC references, and `--offline` for an isolated run. See
[tests/ci/README.md](tests/ci/README.md) for scope, negative-control tests and the
GitHub-hosted workflow. The workflow has passed on GitHub
([quality run 40151959375](https://github.com/seek-hope/bioinfo-tools/actions/runs/40151959375)).

The oracle binaries for the FASTX comparisons are pinned in
[tests/ci/PINS.md](tests/ci/PINS.md). They are downloaded locally, not committed.

The workspace MSRV is Rust 1.91 (set by `rust-bam`/noodles); the primary
`rust-toolchain.toml` pins the latest stable, currently 1.99.1.

## Distribution

`seek-hope/bioinfo-tools-dist` is the only public repository. It contains no
source. Each release contains both binaries:

```bash
curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash
```

The release workflow is [.github/workflows/release.yml](.github/workflows/release.yml):
pushing a `v*` tag builds `rust-cutadapt` and `rust-umitools` and publishes one
release with both tarballs, checksums and `install.sh`. See
[dist/README.md](dist/README.md).

## Evidence

Per-step evidence and known gaps are in `docs/EVIDENCE-*.md` and the matching
JSON files. The current reliability, scale and performance roadmap is
[docs/DEVELOPMENT-PLAN.md](docs/DEVELOPMENT-PLAN.md); the previous plan is retained
as [historical context](docs/DEVELOPMENT-PLAN-LEGACY.md).
The first implementation batch (FASTX input integrity and streaming compressed
QC input) is recorded in [docs/EVIDENCE-P0-START.md](docs/EVIDENCE-P0-START.md).
Local validation of the quality gate is in
[docs/EVIDENCE-P0-CI.md](docs/EVIDENCE-P0-CI.md). The documentation-constraint
audit and the protected-component list are in [docs/DOC-AUDIT.md](docs/DOC-AUDIT.md).
