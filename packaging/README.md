# Packaging preparation

Everything here is **preparation**, not release machinery. Nothing publishes
anything: no GitHub release, no tag, no crates.io upload, no conda channel upload,
no container registry push.

All items are **built and smoke-tested**. See
`../docs/EVIDENCE-PACKAGING.md` for commands and real output.

| File | What it is | Status |
| --- | --- | --- |
| `Containerfile` | Multi-stage container recipe for `rust-cutadapt` | **built + smoke-tested**; image `sha256:a9a6b630…`, reproducible across rebuilds |
| `conda/meta.yaml` | Conda recipe | **built** by conda-build 26.9.1; `.conda` package produced and its binary executed |
| `conda/check_recipe.py` | Stdlib structural checker for the recipe | passes, with a negative control |
| `sbom.py` | Deterministic CycloneDX 1.5 generator from `Cargo.lock` | 5 runs byte-identical, self-test, `--check` |
| `sbom.cdx.json` | The generated SBOM | committed so it can be diffed |
| `repro-check.sh` | Two-build reproducible-build proof | both modes byte-identical |
| `cross-build-aarch64.sh` | aarch64 cross build + qemu smoke test | **PASS**; real ARM ELF, trimmed output identical to the host build |

## Quick reference

```sh
# Reproducible build (proves identical SHA256 across two clean builds).
bash packaging/repro-check.sh

# aarch64 build + qemu smoke test. Exits 0 only when both succeed;
# 3 = toolchain incomplete, 4 = build failed, 1 = smoke test failed.
bash packaging/cross-build-aarch64.sh

# Container.
sudo -n docker build -f packaging/Containerfile -t rust-cutadapt:local .
sudo -n docker run --rm rust-cutadapt:local --version
# Bind mounts must be writable by uid 10001; the --user form keeps host ownership:
sudo -n docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/data" rust-cutadapt:local --version

# Conda. --croot must be outside the source tree or conda-build refuses to copy it.
tests/ci/env/conda-build/bin/conda-build packaging/conda \
    --croot /var/tmp/cbroot --output-folder /var/tmp/cbld --no-anaconda-upload
python3 packaging/conda/check_recipe.py

# SBOM.
python3 packaging/sbom.py            # regenerate
python3 packaging/sbom.py --check    # fail if the committed SBOM is stale
python3 packaging/sbom.py --self-test
```

## Toolchain

`../rust-toolchain.toml` pins **1.91.0**, the workspace MSRV. `--locked` is passed
everywhere a build happens.

## Two limitations worth knowing before trusting an artifact

1. **The conda package is NOT built at the pinned toolchain.** conda-forge's `rust`
   is 1.98.1, and `rust-toolchain.toml` is a rustup feature that a plain
   `$BUILD_PREFIX/bin/cargo` ignores. So the conda binary
   (`305c78e7…`) differs from the pinned 1.91.0 build (`b722e9e0…`). Measured, not
   assumed — see the recipe header for the probe output.
2. **The container image is amd64 only.** The aarch64 *binary* is verified under
   qemu, but no `linux/arm64` image was built.

## What a release would still need

1. A **conda `rust` at 1.91.0** (or a vendored rustup toolchain in the sdist) so the
   conda package matches the pinned build.
2. A **`linux/arm64` image** via `docker buildx`, so the container matches the
   verified aarch64 binary.
3. **`--remap-path-prefix` standardised in CI** if released artifacts should be
   byte-comparable across machines.
4. **Signing and provenance** (cosign / SLSA) if artifacts are distributed.
