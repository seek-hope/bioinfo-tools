# bioinfo-tools

A Cargo workspace for five bioinformatics CLIs and two shared libraries. This
repository is the canonical source tree; no sibling checkouts are required.

## Components

| Path | Purpose |
| --- | --- |
| `bioinfo-common/` | Shared exit codes, diagnostics, summaries and provenance |
| `bioinfo-fastx/` | Streaming FASTX parsing, compression and BGZF support |
| `rust-cutadapt/` | Adapter trimming with a cutadapt 4.6 compatibility target |
| `rust-umitools/` | UMI extraction, whitelist, deduplication, grouping and counting |
| `rust-fx/` | FASTX manipulation with SeqKit/fq compatibility targets |
| `rust-bam/` | SAM/BAM tools built on noodles |
| `rust-fastqc/` | FastQC-compatible data, summary, HTML and ZIP reports |

Support is versioned and command-specific. See the
[compatibility matrix](compat/README.md) before treating a command as a drop-in
replacement for its upstream tool.

## Build and test

```bash
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run the full native gate with:

```bash
bash tests/ci/check.sh
```

Add `--with-fastx-qc-oracles` when the pinned FASTX/QC references are installed,
or `--offline` for an intentionally isolated run. Network access and package
installation are otherwise allowed. Builds use `--locked`; `Cargo.lock` is the
reproducibility baseline.

The workspace MSRV is Rust 1.91. `rust-toolchain.toml` selects the current
stable toolchain. See [tests/ci/README.md](tests/ci/README.md) for gate coverage,
oracle provisioning and CI limits.

## Documentation

- [Development plan](docs/DEVELOPMENT-PLAN.md): current priorities and batch boundaries.
- [Compatibility matrix](compat/README.md): verified, experimental and unsupported surfaces.
- [Benchmark runner](bench/README.md): measurement methods and recorded results.
- [Document audit](docs/DOC-AUDIT.md): current documents, historical records and protected artifacts.
- `docs/EVIDENCE-*.md`: historical execution records tied to specific builds; not current support claims.

## Repository and distribution

The original `rust-cutadapt` and `rust-umitools` repositories were consolidated
into this monorepo on 2026-10-21. Their histories are archived in the
[`archive-pre-monorepo-2026-10-21` release](https://github.com/seek-hope/bioinfo-tools/releases/tag/archive-pre-monorepo-2026-10-21).
See [HISTORY.md](HISTORY.md) for the archive mapping.

The public `seek-hope/bioinfo-tools-dist` repository contains prebuilt binaries,
checksums and `install.sh`, not source. Its release workflow currently packages
`rust-cutadapt` and `rust-umitools`; it does not publish all five CLIs. See
[dist/README.md](dist/README.md). Releases, tags and uploads require maintainer
approval.
