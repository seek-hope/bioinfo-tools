# Packaging preparation evidence

Status: **5 of 6 items achieved; container and conda builds NOT validated** because
the tooling is absent. Nothing was published.

Canonical machine-readable file: [EVIDENCE-PACKAGING.json](EVIDENCE-PACKAGING.json).

Repository: `https://github.com/seek-hope/bioinfo-tools` (private), commit
`47646ad1f03df8acaa5ad677b9fabad9de8adcb5`.

## 1. Item-by-item result

| # | Item | Status |
| --- | --- | --- |
| 1 | Reproducible build: pinned toolchain, `--locked`, two builds identical | **achieved** |
| 2 | aarch64 cross-build | **BLOCKED** — no aarch64 sysroot on the host; precise reason recorded |
| 3 | Container recipe | **recipe written, NOT built** — Docker daemon unavailable |
| 4 | Conda recipe draft | **draft written, NOT built or linted** — `conda-build` not installed |
| 5 | SBOM | **achieved** — deterministic CycloneDX 1.5 from `Cargo.lock` |
| 6 | Evidence | this file and its JSON |

No release, tag, dist upload, crates.io upload or conda publication was performed.

## 2. Toolchain pin

`rust-toolchain.toml` pins **1.91.0**, the workspace MSRV, so a build no longer
depends on whatever `stable` the machine happens to have:

```text
$ rustc --version
rustc 1.91.0 (f8297e351 2025-10-28)
$ cargo --version
cargo 1.91.0 (ea2d97820 2025-10-10)
```

Why 1.91.0 specifically: `noodles` 0.118.0 (used by `rust-bam`) declares
`rust-version = "1.91.0"`, so 1.91 is the workspace floor. Building **at** the floor
proves the MSRV claim rather than only asserting it. A `stable` channel pin would
silently change the compiler over time and break the reproducibility result.

### `--locked` is verified, not just passed

Passing `--locked` proves nothing unless the flag is load-bearing. Tested by adding
an unlisted dependency to a member manifest and confirming cargo refuses:

```text
$ cargo build --release --locked --offline -p rust-fx   # after adding byteorder
error: the lock file .../Cargo.lock needs to be updated but --locked was passed to
prevent this
```

The manifest was restored and `Cargo.lock` confirmed byte-unchanged afterwards.

## 3. Reproducible build (item 1)

`packaging/repro-check.sh` builds twice into separate `CARGO_TARGET_DIR` trees.

### Mode 1 — two clean builds, same checkout path

```text
rustc     : rustc 1.91.0 (f8297e351 2025-10-28)
cargo     : cargo 1.91.0 (ea2d97820 2025-10-10)
commit    : 47646ad1f03df8acaa5ad677b9fabad9de8adcb5
host      : Linux x86_64

build a: b722e9e01792d20b653e31f74c23eadf523b32090a337d3ac1ac64509f44b730
build b: b722e9e01792d20b653e31f74c23eadf523b32090a337d3ac1ac64509f44b730
RESULT : IDENTICAL
size   : 2266784 bytes
```

### Mode 2 — two clean builds with `--remap-path-prefix`

```text
build c: ac47029428b2d4b782ef8fbb9d2915f25053000e0078af7ad99b19a875247c2d
build d: ac47029428b2d4b782ef8fbb9d2915f25053000e0078af7ad99b19a875247c2d
RESULT : IDENTICAL
absolute source paths embedded: 0 (want 0)
size   : 2266400 bytes

stripped sha256: 07ee9abfdb1dcdcd65629bd1d2d3850a2ecd73758245483e9b0177025cfeff02
```

### The distinction that matters

The **plain** build embeds absolute source and registry paths from debug info, so it
is reproducible only at the same checkout and `CARGO_HOME` path. Measured directly:

```text
$ strings -a rust-cutadapt | grep -m2 /home/rimuru
/home/rimuru/.cargo/registry/src/index.crates.io-.../clap_builder-4.6.7/src/error/mod.rs
/home/rimuru/Projects/Code/for_fun/bioinfo-tools/rust-cutadapt/src/cli.rs
```

`--remap-path-prefix` removes them (0 occurrences in mode 2). **Mode 2 is the one a
packager should use**; mode 1 is what "reproducible on this machine" means. Both are
reported rather than claiming the stronger result from the weaker evidence.

### Exact command

```sh
CARGO_TARGET_DIR=/tmp/a RUSTFLAGS="--remap-path-prefix=$PWD=/build/source \
  --remap-path-prefix=$HOME/.cargo=/build/cargo" \
  cargo build --release --locked --offline -p rust-cutadapt
```

The binary links only `libgcc_s`, `libm`, `libc` and `ld-linux`; there is no
`RPATH`/`RUNPATH` and no other shared dependency.

## 4. aarch64 cross-build (item 2) — BLOCKED

The probe reports the blocker itself, with exit 3:

```text
$ bash packaging/cross-build-aarch64.sh
== target installed for the pinned toolchain? ==
aarch64-unknown-linux-gnu: installed

== native C toolchain for the target ==
aarch64-linux-gnu-gcc: MISSING
aarch64-linux-gnu-ld: MISSING
sysroot: MISSING (no aarch64 libc/libgcc for the target)

== BLOCKED ==
```

### What I established, and how

This was diagnosed rather than assumed, and the diagnosis separates three failures
that are easy to conflate:

1. **The Rust target works.** `rustup target add aarch64-unknown-linux-gnu
   --toolchain 1.91.0` succeeded, and a pure-Rust workspace member builds a real
   aarch64 artifact:

   ```text
   $ cargo build --release --locked --offline -p bioinfo-common --target aarch64-unknown-linux-gnu
   Finished `release` profile
   $ file …/libbioinfo_common.rlib
   current ar archive
   ```

2. **Native C dependencies cannot be compiled.** `rust-cutadapt` pulls
   `zstd-sys` and `lzma-sys` through `bioinfo-fastx`, and both compile vendored C.
   Without a cross `cc` they fail:

   ```text
   error occurred in cc-rs: failed to find tool "aarch64-linux-gnu-gcc":
   No such file or directory (os error 2)
   ```

   Pointing cc at `clang --target=aarch64-unknown-linux-gnu` also fails, and the
   reason is instructive: clang finds **x86_64** glibc headers and rejects
   `__float128` for the aarch64 target:

   ```text
   /usr/include/bits/floatn.h:97:9: error: __float128 is not supported on this target
   ```

3. **Nothing can be linked.** Even a pure-Rust binary fails, because the linker is
   x86_64 and there is no aarch64 startup or libc:

   ```text
   rust-lld: error: …/libstd-….rlib(lib.rmeta) is incompatible with elf64-x86-64
   ld.lld: error: cannot open crtbeginS.o: No such file or directory
   ld.lld: error: unable to find library -lgcc_s
   ```

   Tried and rejected: default linker, `clang`, `rust-lld` directly,
   `-fuse-ld=lld`, and explicit `--target`. All fail on the same missing sysroot.

### Verification that no sysroot exists

```text
$ find / -maxdepth 4 \( -name crtbeginS.o -o -name 'libgcc_s.so*' \) | grep -i aarch64
(no output)
$ ls /usr/aarch64-linux-gnu          -> absent
$ ls /usr/lib/gcc-cross              -> absent
```

**This is an environment blocker, not a recipe defect.** To unblock:

```sh
apt-get install gcc-aarch64-linux-gnu libc6-dev-arm64-cross
```

`cross-build-aarch64.sh` will then perform the real build and automatically run a
`qemu-aarch64` smoke test (`qemu-aarch64` is present) and print the artifact hash.
**No aarch64 artifact is claimed.**

## 5. Container (item 3) — recipe only, NOT built

`packaging/Containerfile` is a two-stage recipe (`rust:1.91.0-bookworm` builder →
`debian:bookworm-slim` runtime, non-root user, `--locked` build, `strip`ped binary).

The Docker CLI exists but the **daemon does not**:

```text
$ docker info
Server:
failed to connect to the docker API at unix:///var/run/docker.sock; check if the
path is correct and if the daemon is running: connect: no such file or directory
```

**No container build and no container smoke test were run. Nothing about the image
is claimed.**

### What was validated locally instead

Each step of the recipe that could be executed outside a container was:

| Recipe step | Local validation |
| --- | --- |
| `cargo build --release --locked -p rust-cutadapt` | **ran and succeeded** (identical to the reproducible-build command) |
| `strip target/release/rust-cutadapt` | **ran**: 2,266,784 → 2,046,224 bytes (9.7% smaller) |
| stripped binary still works | **verified**: `rust-cutadapt --version` → `rust-cutadapt 0.1.0` |
| runtime needs only glibc/libgcc/libm | **verified** via `ldd`: no other shared libraries, so `debian:bookworm-slim` is sufficient and the base image needs no extra packages |

What remains unvalidated: the base image pulls, `apt-get` layer, layer caching,
`useradd`, `COPY --from`, and any actual `docker run`.

## 6. Conda recipe (item 4) — draft only, NOT built or linted

`packaging/conda/meta.yaml` is a draft. `conda` itself is installed (26.7.2) but
**`conda-build` is not**:

```text
$ conda render packaging/conda
conda: error: argument COMMAND: invalid choice: 'render' (choose from ...)
$ python3 -c "import conda_build"
ModuleNotFoundError: No module named 'conda_build'
```

So **no `conda-build lint`, no `conda render` and no `conda-build` run happened.**
The recipe is not claimed to be buildable.

### What the draft says, and the two real problems it documents

The recipe is written deliberately rather than plausibly. Two structural facts
about this workspace force its shape:

1. **The sdist must be the whole workspace.** `rust-cutadapt` uses path
   dependencies on `bioinfo-common` and `bioinfo-fastx`, which are **not published
   to crates.io**. A recipe that fetched a single crate tarball could not resolve
   them.
2. **The `rust` package must supply 1.91.0.** `rust-toolchain.toml` pins it. If
   conda-forge's `rust` on a platform is older, the recipe's pin becomes a fiction.
   This is left as an explicit open decision rather than guessed, because guessing
   would produce a recipe that silently builds with the wrong compiler.

### Structural validation performed (standard library only)

`packaging/conda/check_recipe.py` checks what can be checked without `conda-build`:

* required sections present (`package`, `source`, `build`, `requirements`, `test`, `about`);
* no tab characters; no trailing whitespace; every content line is a mapping entry
  or list item (with block-scalar handling);
* `package.version` resolves to a literal, including through a `{% set %}`;
* the build script uses `cargo build --release --locked -p rust-cutadapt`;
* the test block actually executes the binary;
* the recipe declares itself a `DRAFT`;
* `requirements.build` requests a C compiler (needed for `zstd-sys`/`lzma-sys`).

Result, plus a negative control proving the checker can fail:

```text
$ python3 packaging/conda/check_recipe.py
PASS  meta.yaml (structural checks only — conda-build was NOT run)

$ # after removing --locked from the build script
FAIL  meta.yaml
        build script does not use `cargo build --release --locked`
```

`package.version` (0.1.0) matches `rust-cutadapt/Cargo.toml` (0.1.0).

## 7. SBOM (item 5)

No SBOM tooling is installed — `cargo-sbom`, `cargo-cyclonedx`, `cyclonedx-py`,
`syft`, `trivy` and `bom` are all absent, and `import cyclonedx` fails. Rather than
claim an SBOM from a tool that never ran, `packaging/sbom.py` generates one from
first principles using only the standard library, driven by
`cargo metadata --locked --offline`.

```text
$ python3 packaging/sbom.py
wrote packaging/sbom.cdx.json: 162 components, serial urn:uuid:ed6df66e-4f3d-578e-8b6e-d023f31bca02
Cargo.lock sha256: f6a95b462b2bffa326810e451788214d59d6c1dbaad5dd41de18044297edf8bc
```

| Property | Value |
| --- | --- |
| Format | CycloneDX **1.5** |
| Components | **162** (155 registry crates + 7 workspace members) |
| Dependency edges | **162** (full graph, not a flat list) |
| Components with SHA-256 | **155** — every registry crate; the 7 workspace members correctly have none |
| Components with a license | 162 (88 as SPDX *expressions*, 74 as single identifiers) |
| `Cargo.lock` sha256 | `f6a95b462b2bffa326810e451788214d59d6c1dbaad5dd41de18044297edf8bc` |
| `sbom.cdx.json` sha256 | `bc5608019ad3440062593343f409aca564785d9ddf20c3bca9f5915ba3f50af2` |

### Determinism, and how it is achieved

Five consecutive runs produce one distinct hash:

```text
run 1..5: 9517cffefa86119ca6a2c48a1e291717f10a6673a6b6d7df81fbdaaea6169996 (all identical)
```

Guarantees built into the generator:

* components sorted by `(name, version)`; JSON emitted with `sort_keys=True`;
* **no `timestamp`.** CycloneDX normally carries one in `metadata`; a clock value
  would make every run differ, so it is deliberately omitted and `metadata`
  carries the input hash instead, which is strictly more useful for verification;
* no hostname, no absolute paths, no random UUIDs;
* `serialNumber` is a **UUIDv5 derived from the content** (SHA-256 of `Cargo.lock`
  plus the sorted component list), so identical inputs give an identical serial and
  a changed lockfile changes it.

### Two real bugs found while writing it

1. **`cargo metadata` does not expose checksums** — the `checksum` field is `None`
   for all 162 packages. The first version therefore emitted **0** hashes. Fixed by
   parsing `checksum` out of `Cargo.lock` directly.
2. **SPDX expressions are not valid CycloneDX `id` values.** Nearly every crate
   declares `"MIT OR Apache-2.0"`. Forcing that into `license.id` would be invalid
   CycloneDX; expressions now go to `license.expression`.

Both were caught by reading the generated output rather than trusting the generator,
which is why the self-test asserts a minimum hash count and a minimum component
count.

### Self-test

```text
$ python3 packaging/sbom.py --self-test
SELFTEST PASS
```

It proves: two runs are byte-identical, no timestamp leaked, the component count is
not truncated, every component has `type`/`name`/`version`/`purl`, workspace
members are typed `application`, at least 100 components carry checksums, and
components are strictly sorted.

`--check` makes staleness a failure, so a changed `Cargo.lock` cannot silently leave
a stale SBOM:

```text
$ python3 packaging/sbom.py --check
PASS: sbom.cdx.json is current (Cargo.lock sha256 f6a95b462b2bffa3...)
```

A checksum was cross-checked against the lockfile by hand:

```text
sbom adler2 sha: 320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa
lock adler2 sha: 320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa
```

## 8. One source change, and why

Pinning the toolchain surfaced two `clippy` lints that 1.95.0 had not flagged, in
`rust-umitools/src/runtime/budget.rs`: `stats.f_bavail as u64 * stats.f_frsize as u64`
trips `clippy::unnecessary_cast` on x86_64, where those fields are already `u64`.

`u64::from(...)` was tried and is flagged as `useless_conversion` on x86_64 for the
same reason. The casts are **load-bearing on other targets** (`c_ulong` is 32-bit on
32-bit platforms), so removing them would silently truncate there. The change is
therefore an `#[allow(clippy::unnecessary_cast)]` with the reason written above it —
**no behaviour change**, and the casts remain.

Verified: `git diff` on that file shows only added comment lines and the attribute.
All 685 tests pass, debug and release.

## 9. Regression status

Everything re-run under the pinned 1.91.0 toolchain with `--locked`.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | **0 warnings** |
| `cargo test --workspace --locked --offline` | **685 passed** |
| `cargo test --workspace --release --locked --offline` | **685 passed** |
| cutadapt 4.6 differential | all visible conformance cases passed |
| UMI-tools exact | **77/77 byte-identical, 0 differ** |
| UMI-tools semantic | **deterministic: 0 mismatches** |
| W1-3 (SeqKit + fq) | **50/50 cases passed** |
| W1-4 (samtools 1.24) | **20/20 cases passed** |
| QC (FastQC 0.13.0 + MultiQC 1.35) | **22/22 fixtures**, MultiQC parses clean |
| M3 byte-neutrality gate | **9/9 criteria, M3 PASS** |

Nothing regressed.

## 10. What the environment owner must fix

1. **Install an aarch64 sysroot and cross C toolchain.** This is the only thing
   blocking item 2 entirely:
   `apt-get install gcc-aarch64-linux-gnu libc6-dev-arm64-cross`. Then
   `bash packaging/cross-build-aarch64.sh` completes the build and smoke test
   unaided.
2. **Start the Docker daemon** (or provide `podman`). Then
   `docker build -f packaging/Containerfile -t rust-cutadapt:local .` — and the
   smoke test in the recipe header — become runnable. Until then the Containerfile
   is unvalidated text.
3. **Install `conda-build`** into a conda env, then `conda-build lint packaging/conda`
   and a real build. This also forces the `rust` 1.91.0 question in §6.
4. **Decide the conda `rust` provisioning** (a conda package that provides 1.91.0,
   versus vendoring a toolchain). Left open deliberately; guessing would produce a
   recipe that builds with the wrong compiler.
5. If `--remap-path-prefix` is adopted for releases, **standardise it in CI** so
   published artifacts are byte-comparable across machines.

## 11. Explicitly not claimed

* Any container build, `docker run`, or image content. The daemon is absent.
* Any `conda-build lint`, render, or build. `conda-build` is absent.
* Any aarch64 binary. No sysroot exists, so nothing links.
* crates.io publishability. The workspace members use path dependencies, so they
  cannot be published as-is without publishing `bioinfo-common` and
  `bioinfo-fastx` first.
* Signing, provenance attestation, or registry push.
* Reproducibility **across different Rust versions** — only 1.91.0 was measured.
* Reproducibility across different glibc or host architectures.

## 12. Files added

| File | Purpose |
| --- | --- |
| `rust-toolchain.toml` | pins Rust 1.91.0 + rustfmt + clippy |
| `packaging/repro-check.sh` | two-build reproducibility proof (2 modes) |
| `packaging/cross-build-aarch64.sh` | aarch64 probe; exits 3 with the precise reason |
| `packaging/Containerfile` | multi-stage container recipe (not built) |
| `packaging/conda/meta.yaml` | conda recipe draft (not built or linted) |
| `packaging/conda/check_recipe.py` | stdlib structural checker for the draft |
| `packaging/sbom.py` | deterministic CycloneDX 1.5 generator |
| `packaging/sbom.cdx.json` | the generated SBOM |
| `packaging/README.md` | what is real, what is not, and what a release still needs |
| `docs/EVIDENCE-PACKAGING.md` / `.json` | this document and its machine-readable form |

Modified: `rust-umitools/src/runtime/budget.rs` (one `#[allow]` plus its
explanation; no behaviour change).

No release, tag, dist upload, crates.io or conda publication.
