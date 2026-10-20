# Packaging preparation evidence

Status: **all 6 items verified.** aarch64 cross-build, container image and conda
package now build and smoke-test for real. **Nothing was published** — no release,
tag, version bump, crates.io upload, conda channel upload or registry push.

Canonical machine-readable file: [EVIDENCE-PACKAGING.json](EVIDENCE-PACKAGING.json).

Repository: `https://github.com/seek-hope/bioinfo-tools` (private), commit
`47646ad1f03df8acaa5ad677b9fabad9de8adcb5`.

> **Revision note.** An earlier revision of this file recorded the aarch64 build as
> BLOCKED, the container as unbuilt and conda as unlinted. The environment was then
> provisioned and all three were re-run. The resolved blockers are marked as such
> below rather than deleted, so the record shows what was actually fixed.

## 1. Item-by-item result

| # | Item | Previous | **Now** |
| --- | --- | --- | --- |
| 1 | Reproducible build | achieved | **achieved** — re-verified, same hashes |
| 2 | aarch64 cross-build | BLOCKED | **PASS** — real ARM ELF + qemu smoke test |
| 3 | Container | recipe only | **PASS** — built, smoke-tested, reproducible image ID |
| 4 | Conda recipe | draft only | **PASS** — real `conda-build` package, executed |
| 5 | SBOM | achieved | **achieved** — unchanged, `--check` passes |
| 6 | Evidence | achieved | **achieved** — this file |

## 2. Toolchain pin (unchanged)

`rust-toolchain.toml` pins **1.91.0**; `rustc 1.91.0 (f8297e351 2025-10-28)`,
`cargo 1.91.0 (ea2d97820 2025-10-10)`. `--locked` is verified load-bearing: adding an
unlisted dependency makes cargo refuse with *"the lock file needs to be updated but
--locked was passed"*.

## 3. Reproducible build (item 1) — re-verified

`bash packaging/repro-check.sh`, unchanged hashes from the previous run:

| Mode | Hash (two builds each) | Result |
| --- | --- | --- |
| plain (same path) | `b722e9e01792d20b653e31f74c23eadf523b32090a337d3ac1ac64509f44b730` | IDENTICAL |
| `--remap-path-prefix` (portable) | `ac47029428b2d4b782ef8fbb9d2915f25053000e0078af7ad99b19a875247c2d` | IDENTICAL |
| stripped (shipped form) | `07ee9abfdb1dcdcd65629bd1d2d3850a2ecd73758245483e9b0177025cfeff02` | — |

Remapped build embeds **0** absolute host paths. The plain build still embeds them
from debug info, so only the remapped mode is portable — that distinction stands.

## 4. aarch64 cross-build (item 2) — RESOLVED, now PASS

```text
$ bash packaging/cross-build-aarch64.sh
== cross toolchain ==
  aarch64-linux-gnu-gcc: /usr/bin/aarch64-linux-gnu-gcc
  aarch64-linux-gnu-g++: /usr/bin/aarch64-linux-gnu-g++
  aarch64-linux-gnu-ld:  /usr/bin/aarch64-linux-gnu-ld
  sysroot: /usr/aarch64-linux-gnu
  rust target aarch64-unknown-linux-gnu: installed

== artifact ==
  size : 2049128 bytes
  file : ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV),
         dynamically linked, interpreter /lib/ld-linux-aarch64.so.1, for GNU/Linux 3.7.0
  sha256: dcf58efe232e926a1fef3d7199ac19b7f438db68b668b0341cd6e98d9f6a7265
  absolute source paths: 0 (want 0)

== smoke test (qemu-aarch64) ==
  --version -> exit 0: rust-cutadapt 0.1.0
  trim (aarch64) -> exit 0
  trim (host)    -> exit 0
  trimmed output: IDENTICAL across architectures

RESULT: PASS (aarch64 build + qemu smoke test)
```

Cross toolchain used, as provisioned:

```sh
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
```

The artifact is a genuine ARM aarch64 ELF with `NEEDED libgcc_s.so.1, libm.so.6,
libc.so.6`. It **runs**: the trimmed output is byte-identical to the native x86_64
build's, which is the strongest statement available without an aarch64 CI runner.

### The false-success bug, fixed and proven fixed

The old script could print `BLOCKED` and still be read as success, and more
importantly `binfmt_misc` has `qemu-aarch64` registered on this host — so an aarch64
ELF "runs" directly and a **missing loader still yields shell exit status 0**. Both
paths are now guarded:

* the smoke test never relies on `binfmt_misc`; it always invokes
  `qemu-aarch64 -L <sysroot>` explicitly and checks the status;
* every failure path returns non-zero and `exit 0` is reachable only at the end.

Three negative controls, all executed:

| Control | Result |
| --- | --- |
| hide `aarch64-linux-gnu-gcc` from `PATH` | **exit 3** ("cross toolchain incomplete; no build attempted") |
| replace `qemu-aarch64` with a shim that always fails | **exit 1** ("--version failed", "failed to trim", "output differs") |
| genuine run | **exit 0** (`RESULT: PASS`) |

## 5. Container (item 3) — RESOLVED, built and smoke-tested

Docker is reachable via passwordless `sudo -n` (server 29.9.1, overlayfs driver).

```text
$ sudo -n docker build -f packaging/Containerfile -t rust-cutadapt:local .
Successfully built a9a6b630427d
Successfully tagged rust-cutadapt:local
```

| Property | Value |
| --- | --- |
| Image ID | `sha256:a9a6b630427db101301d2ea26eed32a61f9e6f73ab6e59e6c5ba0204ef8a378c` |
| Digest | `rust-cutadapt@sha256:a9a6b630427db101301d2ea26eed32a61f9e6f73ab6e59e6c5ba0204ef8a378c` |
| Architecture | `amd64/linux` |
| Size | 116,652,538 bytes (117 MB), 4 layers |
| **Reproducible** | **yes** — a second `docker build` produced the **same image ID** |

### Smoke tests actually run

| Test | Command | Real output |
| --- | --- | --- |
| version | `docker run --rm rust-cutadapt:local --version` | `rust-cutadapt 0.1.0` (exit 0) |
| default CMD | `docker run --rm rust-cutadapt:local` | prints `--help` usage (exit 0) |
| real trim | `docker run --rm -v /tmp/smoke2:/data rust-cutadapt:local -a AGATCGGAAGAGC -o /data/out.fq /data/in.fq` | exit 0, wrote `out.fq` with the adapter removed |
| non-root | `docker run --rm --entrypoint id …` | `uid=10001(appuser) gid=10001(appuser)` |
| exit-code propagation | `--badflag` / missing file | **exit 2** (usage) / **exit 1** (I/O) — propagated correctly |

### A real deployment detail found by testing

The container runs as `appuser` (uid 10001), so a bind-mounted host directory must be
writable by that uid. The first trim attempt failed:

```text
ERROR: cannot create output "/data/out.fq": Permission denied (os error 13)
```

Both workarounds were tested; the `--user` form is now documented in the
`Containerfile` header because it keeps host file ownership:

```sh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/data" rust-cutadapt:local …
```

Verified: wrote `out.fq` owned by `rimuru rimuru` on a **non-world-writable**
directory. Running as root in the container would also work but would leave
root-owned files on the host, which is why the image does not do that.

## 6. Conda recipe (item 4) — RESOLVED, built and executed

`conda-build` **26.9.1** is at `tests/ci/env/conda-build/bin/conda-build`.

### Two real recipe bugs found and fixed

Building surfaced both, and neither was guessable:

1. **`path: ..` was wrong.** The recipe lives at `packaging/conda/meta.yaml`, so
   `..` resolved to `packaging/`, which contains no `Cargo.toml`. conda-build
   resolves `path:` relative to the recipe directory, so it must be `../..`.
2. **`cargo` is not a conda package.** The draft listed `- cargo` alongside
   `{{ compiler('rust') }}`, which made the dependency solve unsatisfiable
   (`cargo =* * does not exist`). conda-forge's `rust` ships **both** rustc and
   cargo, so the separate requirement was removed.

A third issue was `--croot` placement: conda-build's default work directory lives
**inside** the source tree, so copying the source into it fails with
*"Can't merge/copy source into subdirectory of itself"*. Building with
`--croot /var/tmp/cbroot` fixes it. (The first attempt also filled the 16 GB
`/tmp` tmpfs; the build now runs on the larger filesystem.)

### Real build result

```text
$ conda-build packaging/conda --croot /var/tmp/cbroot --output-folder /var/tmp/cbld --no-anaconda-upload
BUILD START: ['rust-cutadapt-0.1.0-hfb04f09_0.conda']
... Compiling rust-cutadapt v0.1.0
    Finished `release` profile [optimized] target(s) in 40.35s
EXIT=0
```

| Property | Value |
| --- | --- |
| Package | `rust-cutadapt-0.1.0-hfb04f09_0.conda` |
| Size | 841 KB |
| SHA-256 | `1d2d817dfec47d2dcc35acef0d964c2ac2eb98474b9b8eb08c9faedbce3ae582` |
| Runtime depends | `__glibc >=2.17`, `libgcc >=16` |
| Installed file | `bin/rust-cutadapt` |

The **packaged binary was extracted and executed**:

```text
$ ./bin/rust-cutadapt --version
rust-cutadapt 0.1.0                     (exit 0)
$ ./bin/rust-cutadapt -a AGATCGGAAGAGC -o out.fq t.fq
exit 0, adapter trimmed
```

### The toolchain mismatch, measured rather than assumed

`conda-build` resolved **`rust 1.98.1-hc89c8c8_2`** from conda-forge, and a probe
executed *inside the build environment* confirmed:

```text
rustc 1.98.1 (48a229cea 2026-09-01)
cargo 1.98.1 (797e8a9bc 2026-08-05)
$BUILD_PREFIX/bin/rustc          <- not rustup
```

Two consequences, both real and now documented in the recipe itself:

* **`rust-toolchain.toml` is ignored by this recipe.** That file is a rustup
  feature; conda-forge's `rust` installs rustc/cargo directly under
  `$BUILD_PREFIX/bin`. So the recipe **cannot** honour the 1.91.0 pin by
  construction.
* **The packaged binary is therefore not the reproducible one.** Measured:
  conda binary `305c78e79e192b5f77b42284078c78ab6f7cac37098c0d9818e076036049e4b6`
  versus the pinned 1.91.0 build `b722e9e01792d20b653e31f74c23eadf523b32090a337d3ac1ac64509f44b730`.
  Expected: a different compiler produces different code.

This is stated as a limitation, not glossed. Building at exactly 1.91.0 needs either
a conda `rust` package at that version or a vendored rustup toolchain in the sdist.

### `conda-build lint` does not exist in 26.9.1

The old `conda-build lint` subcommand has been removed. `--check` is the nearest
equivalent, and it is **not trustworthy on its own**: it exited **0** even when the
recipe had an unsatisfiable dependency (`cargo does not exist`). That is why the
stdlib checker is kept as a complement:

```text
$ python3 packaging/conda/check_recipe.py
PASS  meta.yaml (structural checks; a real conda-build run is recorded separately)
```

It catches exactly the class of mistake `--check` tolerated — wrong source path, a
dependency that is not a real package, a build script that lost `--locked` — and it
has a negative control (removing `--locked` → FAIL).

## 7. SBOM (item 5) — re-verified, unchanged

```text
$ python3 packaging/sbom.py --check
PASS: sbom.cdx.json is current (Cargo.lock sha256 f6a95b462b2bffa3...)
$ python3 packaging/sbom.py --self-test
SELFTEST PASS
```

| Property | Value |
| --- | --- |
| `Cargo.lock` sha256 | `f6a95b462b2bffa326810e451788214d59d6c1dbaad5dd41de18044297edf8bc` |
| `sbom.cdx.json` sha256 | `bc5608019ad3440062593343f409aca564785d9ddf20c3bca9f5915ba3f50af2` |
| Components | 162 (155 registry + 7 workspace), 155 with SHA-256 |
| Determinism | 5 runs → 1 distinct hash; no timestamp; UUIDv5 serial from content |

## 8. Regression status

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

Nothing regressed.

## 9. Remaining limitations

1. **The conda package is not built at the pinned toolchain.** It uses rustc 1.98.1
   because that is what conda-forge provides and `rust-toolchain.toml` is a rustup
   feature this recipe cannot use. Fixing it needs a conda `rust` at 1.91.0 or a
   vendored toolchain.
2. **`conda-build --check` cannot be used as a gate.** It returned 0 with an
   unsatisfiable dependency. The stdlib checker covers the gap.
3. **Container architecture is amd64 only.** No `linux/arm64` image was built, so
   the aarch64 *binary* is verified but an aarch64 *image* is not.
4. **SBOM is generated by a local stdlib script**, not an ecosystem tool. It is
   deterministic and checksummed, but it is not a signed or attested document.
5. **No signing or provenance** (cosign, SLSA) — not attempted.
6. **Not published:** no release, tag, dist upload, crates.io upload, conda channel
   upload or registry push. All artifacts are local.

## 10. Environment owner: what changed and what is still worth doing

Resolved by the provisioning (previously blocking):

* aarch64 cross toolchain + sysroot → item 2 now builds and smoke-tests.
* Docker daemon (via `sudo -n`) → item 3 now builds and smoke-tests.
* `conda-build` 26.9.1 → item 4 now builds and executes.

Optional next steps:

* Add a conda-forge `rust` at 1.91.0 (or vendor rustup) so the conda package matches
  the pinned toolchain.
* Build a `linux/arm64` image with `docker buildx` so the container matches the
  verified aarch64 binary.
* If releases should be byte-comparable across machines, standardise
  `--remap-path-prefix` in CI.

## 11. Files

Added or changed for this revision:

| File | Change |
| --- | --- |
| `packaging/cross-build-aarch64.sh` | **rewritten**: real build, explicit qemu smoke test, strict exit codes (0/1/3/4), cross-architecture output comparison |
| `packaging/Containerfile` | header now records the built image ID and the non-root volume requirement |
| `packaging/conda/meta.yaml` | `path: ../..`; `cargo` dependency removed; measured toolchain mismatch documented |
| `packaging/conda/check_recipe.py` | docstring corrected: `lint` does not exist in 26.9.1 and `--check` is unreliable |
| `docs/EVIDENCE-PACKAGING.md` / `.json` | this revision |

Unchanged: `rust-toolchain.toml`, `packaging/repro-check.sh`,
`packaging/sbom.py`, `packaging/sbom.cdx.json`, `packaging/README.md`, and the
`rust-umitools` clippy allowance from the previous revision.

No release, tag, version bump, crates.io or conda publication.
