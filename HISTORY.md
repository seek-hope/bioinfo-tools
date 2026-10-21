# Repository history

The workspace originally contained two independent Git repositories:

| Archived history | Original remote |
| --- | --- |
| `rust-cutadapt.bundle` | `https://github.com/seek-hope/rust-cutadapt.git` |
| `rust-umitools.bundle` | `https://github.com/seek-hope/rust-umitools.git` |

Both bundles are release assets of this repository under tag
[`archive-pre-monorepo-2026-10-21`](https://github.com/seek-hope/bioinfo-tools/releases/tag/archive-pre-monorepo-2026-10-21).
The former local `../bioinfo-tools-historical/` directory was removed after the
upload; the release is now the canonical archive.

They were archived when the workspace became a monorepo. The archives are
**mirror clones**, so they contain every branch and tag, not just the imported
tree. Their final refs at consolidation time were:

```text
rust-cutadapt  main        4609a0c
rust-cutadapt  full-compat c337fc8
rust-umitools  main        40b5b70
rust-umitools  v0.2.0      b787498
```

The original `rust-cutadapt` commit imported into the monorepo was `2301bb2`
(`refactor: adopt shared contracts and FASTX layer`); the equivalent
`rust-umitools` commit was `6b89b0d`. Portable bundles are stored next to the
mirrors as `rust-cutadapt.bundle` and `rust-umitools.bundle`.

## GitHub consolidation (2026-10-21)

Five repositories were consolidated into two:

| Deleted repository | Visibility | Replaced by |
| --- | --- | --- |
| `seek-hope/rust-cutadapt` | private | `seek-hope/bioinfo-tools` (this monorepo) |
| `seek-hope/rust-umitools` | private | `seek-hope/bioinfo-tools` (this monorepo) |
| `seek-hope/rust-cutadapt-dist` | public | `seek-hope/bioinfo-tools-dist` |
| `seek-hope/rust-umitools-dist` | public | `seek-hope/bioinfo-tools-dist` |

`bioinfo-tools` stays private and holds the source. `bioinfo-tools-dist` is
public and holds only prebuilt binaries, `install.sh`, checksums and a README.
The deleted distribution releases are archived in the same release as
`dist-assets.tar.gz` (`rust-cutadapt-v0.1.0/` and `rust-umitools-v0.2.0/`).

Each release now contains **both** tools; the release tag applies to the
distribution set, while `rust-cutadapt --version` and `rust-umitools --version`
report the component versions.

The archived Git directories are read-only historical records. Build and test
from this monorepo, not from the archives.
