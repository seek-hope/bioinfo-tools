# Packaging preparation evidence

Status: **all 6 items verified** on the primary **Rust 1.99.1** toolchain.
**Nothing was published** — no release, tag, version bump, crates.io upload, conda
channel upload or registry push.

Canonical machine-readable file: [EVIDENCE-PACKAGING.json](EVIDENCE-PACKAGING.json).

Repository: `https://github.com/seek-hope/bioinfo-tools` (private), commit
`3678b5e7e9626dc7a5e1d10148eaacbd231b2cd1`.

## 1. Toolchain model: one primary pin, one MSRV floor

Two different numbers, and conflating them causes real mistakes:

| Role | Version | Where declared | Meaning |
| --- | --- | --- | --- |
| **Primary build toolchain** | **1.99.1** | `rust-toolchain.toml` | what the repo builds and is verified with |
| **Declared MSRV** (floor) | **1.91** | `[workspace.package] rust-version` in `Cargo.toml` | the minimum a downstream user needs |

```text
$ rustc --version
rustc 1.99.1 (58cf8f9c3 2026-10-13)
$ cargo --version
cargo 1.99.1 (5f94df478 2026-08-27)
$ grep rust-version Cargo.toml
rust-version = "1.91"
```

The MSRV floor is set by `noodles` 0.118.0, which declares
`rust-version = "1.91.0"`. The primary pin tracks the latest stable. All artifact
hashes in this document are from **1.99.1** unless explicitly stated otherwise.

`--locked` is verified load-bearing rather than decorative: adding an unlisted
dependency makes cargo refuse with *"the lock file needs to be updated but --locked
was passed"*.

## 2. Item-by-item result

| # | Item | Status |
| --- | --- | --- |
| 1 | Reproducible build | **verified** on 1.99.1 — plain and portable modes both byte-identical |
| 2 | aarch64 cross-build | **verified** — real ARM ELF + qemu smoke test, output identical to host |
| 3 | Container | **verified** — `rust:1.99.1-bookworm` image built and smoke-tested |
| 4 | Conda recipe | **verified-with-limitation** — built, packaged binary executed; uses rust **1.99.0** |
| 5 | SBOM | **verified** — unchanged (`Cargo.lock` did not move) |
| 6 | Evidence | **verified** — this revision |

## 3. Reproducible build (item 1) — re-measured on 1.99.1

`bash packaging/repro-check.sh`:

```text
rustc     : rustc 1.99.1 (58cf8f9c3 2026-10-13)
cargo     : cargo 1.99.1 (5f94df478 2026-08-27)
commit    : 3678b5e7e9626dc7a5e1d10148eaacbd231b2cd1

build a: e65e5935e026144e3f2866df79d568fc952250e389fc19e799e75c8ad023a4a4
build b: e65e5935e026144e3f2866df79d568fc952250e389fc19e799e75c8ad023a4a4
RESULT : IDENTICAL

build c: 16d35e0a7e9eb6ae655a1315058c91b123e400758b5f9c283d73dfe668f7c997
build d: 16d35e0a7e9eb6ae655a1315058c91b123e400758b5f9c283d73dfe668f7c997
RESULT : IDENTICAL
absolute source paths embedded: 0 (want 0)

stripped sha256: 6697b029b765df0757f73cdebd4af08dc030790df9b13214b75263cb2bcc1603
```

| Mode | SHA-256 | Note |
| --- | --- | --- |
| plain (same checkout path) | `e65e5935e026144e3f2866df79d568fc952250e389fc19e799e75c8ad023a4a4` | identical twice |
| `--remap-path-prefix` (portable) | `16d35e0a7e9eb6ae655a1315058c91b123e400758b5f9c283d73dfe668f7c997` | identical twice, **0** embedded absolute paths |
| stripped (shipped form) | `6697b029b765df0757f73cdebd4af08dc030790df9b13214b75263cb2bcc1603` | — |

The plain build still embeds absolute source and registry paths from debug info, so
only the remapped mode is portable across machines. Both are reported.

> These hashes supersede the earlier 1.91.0 values (`b722e9e0…`, `ac470294…`,
> `07ee9abf…`), which are no longer current. A different compiler produces
> different code, so the change is expected.

## 4. aarch64 cross-build (item 2) — re-measured on 1.99.1

```text
$ bash packaging/cross-build-aarch64.sh
  file : ELF 64-bit LSB pie executable, ARM aarch64, dynamically linked,
         interpreter /lib/ld-linux-aarch64.so.1, for GNU/Linux 3.7.0
  sha256: 32ffc643d12b9b9a15fe0928664d2774ae11128d4a58b45720255b417ebdbd99
  absolute source paths: 0 (want 0)

  --version -> exit 0: rust-cutadapt 0.1.0
  trim (aarch64) -> exit 0
  trim (host)    -> exit 0
  trimmed output: IDENTICAL across architectures

RESULT: PASS (aarch64 build + qemu smoke test)
```

Artifact: `32ffc643d12b9b9a15fe0928664d2774ae11128d4a58b45720255b417ebdbd99`,
2,049,128 bytes, genuine ARM aarch64 ELF, `NEEDED libgcc_s.so.1, libm.so.6, libc.so.6`.

Cross toolchain: `aarch64-linux-gnu-gcc` (GCC 16.2.0), `aarch64-linux-gnu-g++`,
`aarch64-linux-gnu-ld` (binutils 2.47), sysroot `/usr/aarch64-linux-gnu`. Exported as:

```sh
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
```

The aarch64 binary **runs** under `qemu-aarch64 -L /usr/aarch64-linux-gnu`, and its
trimmed output is byte-identical to the x86_64 build's — the strongest statement
available without an aarch64 CI runner.

### The false-success paths, still guarded

`binfmt_misc` registers `qemu-aarch64` on this host, so an aarch64 ELF "runs"
directly and a **missing loader still yields shell exit status 0**. The script
therefore always invokes `qemu-aarch64 -L <sysroot>` explicitly and checks status;
every failure path is non-zero and `exit 0` is reachable only at the end.

| Control | Result |
| --- | --- |
| hide `aarch64-linux-gnu-gcc` from `PATH` | **exit 3** ("cross toolchain incomplete; no build attempted") |
| replace `qemu-aarch64` with an always-failing shim | **exit 1** ("--version failed", "failed to trim", "output differs") |
| genuine run | **exit 0** (`RESULT: PASS`) |

## 5. Container (item 3) — 1.99.1 image

`packaging/Containerfile` builds from **`rust:1.99.1-bookworm`** into
`debian:bookworm-slim`.

```text
$ sudo -n docker build -f packaging/Containerfile -t rust-cutadapt:1.99.1 .
```

| Property | Value |
| --- | --- |
| Tag | `rust-cutadapt:1.99.1` |
| Image ID (short) | **`f4b0196e99f2`** |
| Image ID (full) | `sha256:f4b0196e99f259bd1becf42b7b9064d261e600d7c2e483fac91efc90c0134710` |
| Size | 116,555,120 bytes |
| Layers | 4 |
| Architecture | amd64/linux |
| **Reproducible** | **yes** — a second `docker build` produced the identical image ID `f4b0196e99f2…` |

### Smoke tests actually run

| Test | Command | Result |
| --- | --- | --- |
| version | `docker run --rm rust-cutadapt:1.99.1 --version` | `rust-cutadapt 0.1.0`, exit 0 |
| non-root user | `docker run --rm --entrypoint id …` | `uid=10001(appuser) gid=10001(appuser)` |
| real trim | `docker run --rm --user "$(id -u):$(id -g)" -v /tmp/cs:/data … -a AGATCGGAAGAGC -o /data/out.fq /data/in.fq` | exit 0, wrote `out.fq` (8 bytes, adapter removed) |
| exit-code propagation | `--badflag` / missing file | **2** (usage) / **1** (I/O) |

The earlier `rust-cutadapt:local` image (`a9a6b630…`, built from
`rust:1.91.0-bookworm`) is still present locally but is **superseded**; the 1.99.1
tag is the one this document records.

### Deployment detail found by testing

The container runs as `appuser` (uid 10001), so a bind-mounted host directory must be
writable by that uid or output creation fails with
`Permission denied (os error 13)`. The `--user "$(id -u):$(id -g)"` form keeps host
file ownership and is documented in the `Containerfile` header.

## 6. Conda recipe (item 4) — built with rust 1.99.0

`conda-build` **26.9.1** at `tests/ci/env/conda-build/bin/conda-build`.

```text
$ conda-build packaging/conda --croot /var/tmp/cbr-latest --output-folder /var/tmp/cbld-latest-out --no-anaconda-upload
EXIT=0
```

| Property | Value |
| --- | --- |
| Package | `rust-cutadapt-0.1.0-hfb04f09_0.conda` |
| SHA-256 | `ef1b58a8c897e2e2bffef06f0297e0efb70371aeb666246ffd55a9a9194fe0f6` |
| Size | 790 KB |
| Runtime depends | `__glibc >=2.17`, `libgcc >=16` |
| **Rust resolved** | **`rust 1.99.0-h846490e_0`** |

### The packaged binary was extracted and executed

```text
$ ./bin/rust-cutadapt --version
rust-cutadapt 0.1.0                (exit 0)

$ ./bin/rust-cutadapt -a AGATCGGAAGAGC -o out.fq t.fq
exit 0, adapter trimmed
```

Packaged binary SHA-256: `9b7259236dac3dca9a83f20d82a42a074c2bc9b8ec45f9fc8553a07912fd9b74`.

### Why the conda binary is not the 1.99.1 artifact

conda-forge's newest `rust` is **1.99.0**, not 1.99.1 (verified:
`conda search -c conda-forge rust`). And `rust-toolchain.toml` is a **rustup**
feature, while conda-forge's `rust` installs rustc/cargo directly under
`$BUILD_PREFIX/bin` — so the pin is ignored by construction.

Measured proof, from the ELF's own embedded compiler hash:

| Binary | Embedded `/rustc/` commit |
| --- | --- |
| conda-packaged | `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4` |
| local 1.99.1 build | `58cf8f9c35b355199727627d07ae34a4e2846d67` |

Different commits, so the conda package is built by rust 1.99.0 and is **not**
byte-comparable with the 1.99.1 artifacts. That is expected for a different compiler
and is recorded in the recipe header rather than papered over. Building at exactly
1.99.1 would need a conda `rust` at that version or a vendored rustup toolchain.

### Conda package reproducibility: near-deterministic, not bit-identical

Two independent builds with the same recipe and the same rust 1.99.0 produced
**different package hashes** (`ef1b58a8…` vs `03276f8a…`), so conda packaging is
**not** bit-reproducible here. Measured cause and impact:

* the packaged **binaries** differ in **3 bytes of 2,148,288** (BuildID-related, at
  offsets 5445, 1877465, 2146465);
* both embed the same rustc (`/rustc/b940084d`), so this is not a compiler or
  toolchain difference;
* both binaries are **functionally identical**: the same trim produces identical
  output (`md5 5330749bbd4c3ae055f62a9a0331e9d9`).

So the claim is "the conda package builds and works", **not** "the conda package is
bit-reproducible". Only the Rust build (item 1) carries the reproducibility claim.

### `conda-build lint` does not exist in 26.9.1

The old `lint` subcommand was removed; `--check` is the nearest equivalent and is
**unreliable** — it exited 0 with an unsatisfiable dependency. The stdlib checker is
kept as a complement and passes, catching exactly the class of mistake `--check`
tolerated (wrong source path, a dependency that is not a real package, a build
script that lost `--locked`).

### Recipe bugs found earlier and still fixed

1. `path: ..` → **`../..`** (conda-build resolves `path:` relative to the recipe dir,
   so `..` pointed at `packaging/`, which has no `Cargo.toml`).
2. `- cargo` removed: no conda package named `cargo` exists; conda-forge's `rust`
   ships both rustc and cargo.
3. `--croot` must be **outside** the source tree, else
   *"Can't merge/copy source into subdirectory of itself"*.

## 7. SBOM (item 5) — unchanged

`Cargo.lock` did not move, so the SBOM is still current:

```text
$ python3 packaging/sbom.py --check
PASS: sbom.cdx.json is current (Cargo.lock sha256 f6a95b462b2bffa3...)
$ python3 packaging/sbom.py --self-test
SELFTEST PASS
```

| Property | Value |
| --- | --- |
| Format | CycloneDX 1.5 |
| Components | 162 (155 registry + 7 workspace), 155 with SHA-256 |
| `Cargo.lock` sha256 | `f6a95b462b2bffa326810e451788214d59d6c1dbaad5dd41de18044297edf8bc` |
| `sbom.cdx.json` sha256 | `bc5608019ad3440062593343f409aca564785d9ddf20c3bca9f5915ba3f50af2` |
| Determinism | 5 runs → 1 distinct hash; no timestamp; UUIDv5 serial from content |

## 8. Regression status on 1.99.1

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | **0 warnings** |
| `cargo test --workspace --locked --offline` | **685 passed** |
| `cargo test --workspace --release --locked --offline` | **685 passed** |
| cutadapt 4.6 differential | all visible conformance cases passed |
| UMI-tools exact | **77/77 byte-identical, 0 differ** |
| W1-3 (SeqKit + fq) | **50/50 cases passed** |
| W1-4 (samtools 1.24) | **20/20 cases passed** |
| QC (FastQC 0.13.0 + MultiQC 1.35) | **22/22 fixtures**, MultiQC parses clean |
| M3 byte-neutrality gate | **9/9 criteria, M3 PASS** |
| Reproducible build | both modes IDENTICAL |
| SBOM | `--check` PASS, self-test PASS |
| conda recipe structural check | PASS |

Two `clippy` lints specific to 1.99 were fixed in `rust-fastqc/src/stats.rs` and
`rust-fx/src/md5.rs`; behaviour is unchanged and the suites above confirm it.

## 9. Final artifact hashes

| Artifact | SHA-256 |
| --- | --- |
| x86_64 release, plain (1.99.1) | `e65e5935e026144e3f2866df79d568fc952250e389fc19e799e75c8ad023a4a4` |
| x86_64 remapped portable (1.99.1) | `16d35e0a7e9eb6ae655a1315058c91b123e400758b5f9c283d73dfe668f7c997` |
| x86_64 stripped (shipped form) | `6697b029b765df0757f73cdebd4af08dc030790df9b13214b75263cb2bcc1603` |
| aarch64 (1.99.1) | `32ffc643d12b9b9a15fe0928664d2774ae11128d4a58b45720255b417ebdbd99` |
| container image `rust-cutadapt:1.99.1` | `sha256:f4b0196e99f259bd1becf42b7b9064d261e600d7c2e483fac91efc90c0134710` |
| conda package (rust 1.99.0) | `ef1b58a8c897e2e2bffef06f0297e0efb70371aeb666246ffd55a9a9194fe0f6` |
| conda packaged binary | `9b7259236dac3dca9a83f20d82a42a074c2bc9b8ec45f9fc8553a07912fd9b74` |
| SBOM | `bc5608019ad3440062593343f409aca564785d9ddf20c3bca9f5915ba3f50af2` |

## 10. Remaining limitations

1. **The conda package uses rust 1.99.0, not the 1.99.1 primary pin.** conda-forge
   has no 1.99.1, and `rust-toolchain.toml` is a rustup feature a
   `$BUILD_PREFIX/bin/cargo` ignores. Fixing it needs a conda `rust` at 1.99.1 or a
   vendored toolchain.
2. **Conda packaging is not bit-reproducible** — two builds differ in 3 bytes of the
   packaged binary. Functionally identical. Only the Rust build carries the
   reproducibility claim.
3. **`conda-build --check` is not a usable gate**: it returned 0 with an
   unsatisfiable dependency.
4. **The container image is amd64 only** — no `linux/arm64` image, so the aarch64
   *binary* is verified but an aarch64 *image* is not.
5. **The SBOM is generated by a local stdlib script**, not an ecosystem tool:
   deterministic and checksummed, but not signed or attested.
6. **No signing or provenance** (cosign, SLSA) was attempted.
7. **The MSRV (1.91) is declared, not CI-tested on this machine.** The suites above
   were run on 1.99.1 only; 1.91 was previously verified to build the workspace, and
   `--locked` guards the dependency set.

## 11. Environment state

Resolved by provisioning (previously blocking):

* aarch64 cross toolchain + sysroot → item 2 builds and smoke-tests.
* Docker daemon (via passwordless `sudo -n`) → item 3 builds and smoke-tests.
* `conda-build` 26.9.1 → item 4 builds and executes.

Optional next steps: a conda `rust` at 1.99.1; a `linux/arm64` image via
`docker buildx`; `--remap-path-prefix` standardised in CI; signing/provenance.

## 12. Revision history

| Revision | Change |
| --- | --- |
| 1 | Reproducible build + SBOM achieved; aarch64, container and conda recorded as blocked/not built. |
| 2 | Environment provisioned: aarch64, container and conda all built and smoke-tested; cross script rewritten with strict exit codes. |
| **3 (this)** | **Primary toolchain moved to 1.99.1** with the declared MSRV held at 1.91. All artifact hashes re-measured (plain `e65e5935…`, portable `16d35e0a…`, aarch64 `32ffc643…`). Container rebuilt as `rust-cutadapt:1.99.1` (`f4b0196e99f2`). Conda rebuilds with rust **1.99.0** and is documented as near-deterministic (3 bytes) rather than bit-reproducible. Two 1.99-specific clippy lints fixed in `rust-fastqc/src/stats.rs` and `rust-fx/src/md5.rs`. |

No release, tag, version bump, crates.io or conda publication.
