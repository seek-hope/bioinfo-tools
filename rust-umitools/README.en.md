# rust-umitools

A Rust implementation of core UMI-tools 1.1.6 workflows. Supported inputs and
option combinations are listed in [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md);
experimental behavior and known differences are not drop-in compatibility claims.

**English** | [简体中文](README.md)

## Verification

- **Byte-exact comparison:** `run_exact.sh` contains **77 cases** (5 clustering methods x single/paired x read_id/tag x sort switches, duplicate pairs, MAPQ, NH/X0/XT, spliced, read-length, ignore-tlen, UMI lengths 1-40, thresholds 0/1/2, long soft clips/introns, large UG, existing UG/BX, per-gene/per-cell, subset, `--chrom`, `count --wide`). The decompressed header and record stream match UMI-tools exactly.
- **Semantic comparison:** `run_oracle.sh` covers 39 checks; additional suites cover randomized differential tests (generator + brute-force reference + upstream), determinism (threads x spill x multiple merge passes), formats/pipes and benchmark smoke.
- **RNG alignment:** reproduces CPython MT19937 for the verified dedup paths, including the extra draw after multimapping replacement. Gene-map representative ties are an exception: upstream contig order depends on `PYTHONHASHSEED`.
- **Shared implementation:** dedup/group/count use the same UMI encoding, neighbour lookup, clustering and disk-backed code.

## Installation

### Prebuilt binary (Linux x86_64, public download)

Public distribution repository: [seek-hope/bioinfo-tools-dist](https://github.com/seek-hope/bioinfo-tools-dist). Each release contains both `rust-umitools` and `rust-cutadapt`:

```bash
curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash -s rust-umitools
```

The installer downloads the latest release, verifies its SHA256 checksum and installs to `~/.local/bin`. You can also choose a version or directory:

```bash
INSTALL_DIR=/opt/bin BIOINFO_VERSION=v0.2.0 bash install.sh rust-umitools
```

Manual download:

```bash
base=https://github.com/seek-hope/bioinfo-tools-dist/releases/latest/download
curl -fLO "$base/rust-umitools-x86_64-unknown-linux-gnu.tar.gz"
curl -fLO "$base/rust-umitools-x86_64-unknown-linux-gnu.tar.gz.sha256"
sha256sum -c rust-umitools-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf rust-umitools-x86_64-unknown-linux-gnu.tar.gz
install -m 755 rust-umitools-x86_64-unknown-linux-gnu/rust-umitools ~/.local/bin/
```

The prebuilt package targets Linux x86_64 (glibc). An aarch64 prebuilt package is not currently provided.

### Build from source

The source repository is private:

```bash
cargo install --git https://github.com/seek-hope/bioinfo-tools --package rust-umitools --locked
```

Rust 1.91+ is required. For local development from a checkout:

```bash
cargo build --release --locked  # target/release/rust-umitools
cargo install --path . --locked # or install to ~/.cargo/bin
```

## Quick start (5 minutes)

```bash
# Deduplicate (coordinate-sorted output by default)
rust-umitools dedup -I sorted.bam -S deduped.bam
# Paired-end
rust-umitools dedup -I sorted.bam -S deduped.bam --paired
# Group / count
rust-umitools group -I sorted.bam   -S grouped.bam --output-bam --group-out groups.tsv
rust-umitools count -I assigned.bam -S counts.tsv  --per-gene --gene-tag GX
# Single-cell
rust-umitools dedup -I in.bam -S out.bam --per-gene --gene-tag GX --per-cell --cell-tag CB
rust-umitools count -I in.bam -S c.tsv --per-gene --gene-tag GX --per-cell --cell-tag CB --wide
# FASTQ
rust-umitools extract   -I reads.fastq.gz -S out.fastq.gz -p NNNN
rust-umitools whitelist -I reads.fastq.gz -S whitelist.tsv -p NNNNCCCC
# CRAM and pipes
rust-umitools dedup -I in.cram -S out.bam --reference genome.fa
cat in.bam | rust-umitools dedup -I - -S - | samtools view -c -
```

Every subcommand's `--help` includes copyable examples, defaults and exit-code information.

## Migrating from umi_tools

| umi_tools | rust-umitools |
|---|---|
| `umi_tools dedup -I in.bam -S out.bam` | Same option names |
| `--paired`, `--extract-umi-method=tag --umi-tag=RX` | Same option names |
| `--per-gene --gene-tag GX`, `--per-cell --cell-tag CB` | Same option names (per-cell requires `--per-gene`) |
| `umi_tools group --group-out g.tsv` | Same option names; writing BAM requires `--output-bam -S out.bam` |
| `umi_tools count --per-gene --gene-tag GX [--per-cell] [--wide]` | Same option names |
| `umi_tools count_tab -I r.tsv -S c.tsv [--per-cell]` | Same option names |
| `umi_tools extract -I r.fastq -S o.fastq -p NNNN` | Same option names (string and regex subset) |
| `umi_tools whitelist -I r.fastq -S wl.tsv -p NNNNCCCC` | Same option names (distance/density knee, correction map) |

Defaults match upstream: `--method=directional`, `--edit-distance-threshold=1`, `--umi-separator=_`, `--soft-clip-threshold=4`, unpaired/chimeric policy `use`, unmapped `discard`.

## Performance (same machine, input and parameters)

> The table below is the historical baseline for the 4,428,466-read `small.bam`; later mimalloc, compression-level and count-only changes were not re-measured on that input. See [docs/BENCHMARK.md](docs/BENCHMARK.md) and `docs/bench/*.json` for current synthetic/real benchmarks and commands.

| Tool | wall time | peak RSS |
|---|---:|---:|
| UMI-tools 1.1.6 | 143.08 s | 1.10 GiB |
| **rust-umitools** | **9.24 s** | **0.78 GiB** |
| umi-tools-rs 2.0.1 | 73.29 s | 0.94 GiB |

- Historical baseline vs UMI-tools: **15.49x**; vs umi-tools-rs: **7.93x**; all three reported the same molecule count (3,718,913).
- Neighbour-strategy thresholds, sort strategies and bounded-memory measurements are in [docs/BENCHMARK.md](docs/BENCHMARK.md).

## Bounded memory

- The bundle window ends at position boundaries; oversized mate requests spill and are re-scanned in blocks; gene aggregation partitions by gene hash when over budget; a single oversized bundle uses blocked exact neighbour search plus on-disk pair indexes/CSR.
- These paths are **byte-identical** to the in-memory path, enforced by `run_spill.sh` (K9).
- `--memory-limit` minimum is 64 MiB; `--sort-strategy auto|low-memory|high-throughput` changes buffering policy without changing output.

## Diagnostics and automation

| Option | Purpose |
|---|---|
| `--quiet` | Keep only errors and warnings; suitable for CI |
| `--progress auto\|on\|off` | Progress output; terminal-only by default |
| `--summary-json FILE` | Atomically write a machine-readable summary (counters, resources, stage timings) |
| `--version --verbose` | Reference version, compression backend, commands and exit codes |
| `--sort-strategy ...` | Sort/batch budget policy; decompressed output is unchanged |
| `--subset P` | Sample with probability P (shares RNG with tie-breaking, matching upstream) |
| `--chrom NAME` | Process one contig only (name validated before output) |
| `--reference FASTA` | CRAM read/write reference |
| `--partition-genes N` | Gene-partition fallback for count (0 = automatic) |

The workspace provides `.github/workflows/quality.yml` (x86_64 fmt/clippy/test/rustdoc, MSRV 1.91) and a `Containerfile`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 2 | Usage error, or a capability this build does not implement (rejected before output) |
| 3 | Input does not meet a precondition (mixed UMI lengths, unsorted input, etc.) |
| 4 | I/O or resource error |

## Known limitations

- Uses mimalloc as the global allocator (`purge_delay=0` at startup); count uses a compact `CountBundle`, reducing peak RSS on high-UMI input from about 444 MiB to about 255 MiB (an earlier sorted variant reached 203 MiB, but insertion order is retained in the final version).
- Default `--compresslevel 1` (faster, slightly larger); use `--compresslevel 6` for smaller output close to the upstream default.
- `--output-stats` writes observed per-position/per-UMI tables and a NumPy-compatible null model. The `unique` method's duplicate-dict-key overwrite is covered by exact TSV comparisons.
- `dedup --per-gene --per-contig --gene-transcript-map` writes the gene `MC` tag. Upstream representative ties depend on Python `set` order and `PYTHONHASHSEED`, so exact representative parity is not a stable target.
- `--representative=stable` is a project extension, not an upstream RNG mode.
- `--memory-limit` reserves 8-32 MiB for noodles, the allocator and thread stacks; the rest is the bundle/sort budget. Recorded 64/128/256 MiB budgets had about 57/103/187 MiB peak RSS; it is not a hard whole-process limit.
- dedup defaults to half of available memory; a high-duplication 2M-read input measured about 2.47 GiB RSS with auto. Use `--memory-limit 1GiB` for about 1.30 GiB at roughly +17% wall; `--sort-strategy low-memory` does not change that peak.
- A single oversized position or gene that exceeds the post-reservation budget exits 4 before writing output and suggests raising `--memory-limit` or splitting the input.
- CRAM decoding by noodles may add derived `MD/NM` tags; compare BAM/CRAM by core fields.
- Compressed BAM bytes may differ from upstream (compression level/BGZF layout); decompressed content is byte-identical.
- When `--random-seed` is not fixed, upstream uses `os.urandom` and is not reproducible; this implementation defaults to seed 0.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --locked
bash tests/oracle/run_oracle.sh      # 39 semantic checks
bash tests/oracle/run_exact.sh       # 77 byte-exact checks
bash tests/ci/check_manifest.sh      # compatibility matrix sync
bash tests/oracle/run_determinism.sh # threads x spill x merge passes
bash tests/oracle/run_spill.sh       # bounded-memory equivalence
bash tests/oracle/run_extract.sh     # extract + whitelist
bash tests/oracle/run_formats.sh     # stdin/stdout, CRAM, summary
bash tests/bench/run_bench_smoke.sh  # benchmark smoke
```

Compatibility details are in [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).
Current development priorities are in the
[workspace development plan](../docs/DEVELOPMENT-PLAN.md);
[docs/ROADMAP.md](docs/ROADMAP.md) is the historical component roadmap.
