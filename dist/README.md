# bioinfo-tools-dist

Public distribution repository for **bioinfo-tools**. It contains only
prebuilt Linux binaries, the install script, checksums and this README. The
source code lives in the private `seek-hope/bioinfo-tools` monorepo.

Every release contains **both** tools:

| Tool | What it does |
| --- | --- |
| `rust-cutadapt` | Adapter trimming for FASTQ/FASTA reads, cutadapt 4.6-compatible CLI |
| `rust-umitools` | UMI-tools 1.1.6-compatible deduplication, grouping and counting |

Both binaries target `x86_64-unknown-linux-gnu` (glibc). No aarch64 or macOS
artifact is currently published.

## Install

Install both tools into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash
```

Install only one of them:

```bash
curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash -s rust-cutadapt
curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash -s rust-umitools
```

The script downloads the requested tarballs, verifies each SHA256 checksum,
and installs the binaries. Choose a version or destination directory with
environment variables:

```bash
BIOINFO_VERSION=v0.2.0 INSTALL_DIR=/opt/bin bash install.sh
```

| Variable | Default |
| --- | --- |
| `BIOINFO_DIST_REPO` | `seek-hope/bioinfo-tools-dist` |
| `BIOINFO_VERSION` | `latest` |
| `INSTALL_DIR` | `$HOME/.local/bin` |

## Release assets

A release tagged `vX.Y.Z` contains:

```text
rust-cutadapt-x86_64-unknown-linux-gnu.tar.gz
rust-cutadapt-x86_64-unknown-linux-gnu.tar.gz.sha256
rust-umitools-x86_64-unknown-linux-gnu.tar.gz
rust-umitools-x86_64-unknown-linux-gnu.tar.gz.sha256
SHA256SUMS
install.sh
```

Manual download and verification:

```bash
version=v0.2.0
base="https://github.com/seek-hope/bioinfo-tools-dist/releases/download/${version}"
curl -fLO "$base/rust-cutadapt-x86_64-unknown-linux-gnu.tar.gz"
curl -fLO "$base/rust-cutadapt-x86_64-unknown-linux-gnu.tar.gz.sha256"
sha256sum -c rust-cutadapt-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf rust-cutadapt-x86_64-unknown-linux-gnu.tar.gz
install -m 755 rust-cutadapt-x86_64-unknown-linux-gnu/rust-cutadapt ~/.local/bin/
```

## Versioning

The tag applies to the distribution set, not to either tool individually.
`rust-cutadapt --version` and `rust-umitools --version` report the component
versions that were built into that release; the release notes record both.

## Source, issues and license

The source repository is private. This repository intentionally contains no
source code. Both tools are MIT-licensed; see [LICENSE](LICENSE).
