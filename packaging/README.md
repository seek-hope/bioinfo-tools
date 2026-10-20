# Packaging preparation

Everything in this directory is **preparation**, not release machinery. Nothing here
publishes anything: no GitHub release, no tag, no crates.io upload, no conda channel
upload, no container registry push.

| File | What it is | Validated? |
| --- | --- | --- |
| `Containerfile` | Multi-stage container recipe for `rust-cutadapt` | **NO — Docker daemon unavailable.** The build command and `strip` were validated outside a container. |
| `conda/meta.yaml` | Draft conda recipe | **NO — `conda-build` not installed.** Structural checks only. |
| `conda/check_recipe.py` | Stdlib structural checker for the recipe | yes, with a negative control |
| `sbom.py` | Deterministic CycloneDX 1.5 generator from `Cargo.lock` | yes — 5 runs byte-identical, self-test, `--check` |
| `sbom.cdx.json` | The generated SBOM (committed so it can be diffed) | yes |
| `repro-check.sh` | Two-build reproducible-build proof | yes — both modes byte-identical |
| `cross-build-aarch64.sh` | aarch64 cross-build probe | yes — **reports BLOCKED, exit 3**, with the reason |

## Quick reference

```sh
# Reproducible build (proves identical SHA256 across two clean builds).
bash packaging/repro-check.sh

# aarch64 probe (exits 3 and names the missing sysroot).
bash packaging/cross-build-aarch64.sh

# SBOM.
python3 packaging/sbom.py            # regenerate
python3 packaging/sbom.py --check    # fail if the committed SBOM is stale
python3 packaging/sbom.py --self-test

# conda recipe structural checks (NOT conda-build lint).
python3 packaging/conda/check_recipe.py
```

## Toolchain

`../rust-toolchain.toml` pins **1.91.0**, the workspace MSRV. Every command above
uses it automatically. `--locked` is passed everywhere a build happens, so the
committed `Cargo.lock` is authoritative.

## What a release would still need

Recorded here so the gap is explicit rather than implied:

1. **Build the container at least once** on a host with a working Docker daemon, and
   run a smoke test inside it.
2. **`conda-build lint` and a real `conda-build` run** for each target platform.
   `check_recipe.py` only checks structure.
3. **An aarch64 sysroot** on the build host, then re-run
   `cross-build-aarch64.sh` — it will do the real build and a `qemu-aarch64` smoke
   test automatically once the toolchain is present.
4. **A decision on the conda `rust` package**, which must supply 1.91.0 or the
   recipe's toolchain pin is a fiction.
5. **Signing and provenance** (cosign / `--provenance`) if the artifacts are to be
   distributed; not attempted here.

See `../docs/EVIDENCE-PACKAGING.md` for the measured results and exact commands.
