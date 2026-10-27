# Quality gate

`tests/ci/check.sh` is the reusable native gate. Network access and installation
are allowed by default; `--offline` is an explicit isolation mode, not a project
prerequisite. The gate never edits golden files, publishes, or refreshes expected
output.

```bash
python3 tests/ci/test_check.py -v
python3 tests/ci/check_docs.py

bash tests/ci/check.sh
bash tests/ci/check.sh --offline
bash tests/ci/check.sh --with-fastx-qc-oracles
bash tests/ci/check.sh --with-fastx-qc-oracles --isolate-network
```

## What runs

| Mode | Checks |
| --- | --- |
| Base | `check_docs.py`; compatibility matrix drift check; workspace `fmt --check`; clippy with warnings denied; rustdoc with warnings denied; release tests; all native binaries; cutadapt visible plain/gzip fixture smoke |
| `--offline` | Base checks with `CARGO_NET_OFFLINE=true` and `--offline` passed to Cargo; a missing pinned toolchain fails instead of being installed |
| `--with-fastx-qc-oracles` | Base checks plus SeqKit 2.14.0/fq 0.12.0 and FastQC 0.13.0/MultiQC 1.35 differentials, with version probes first |
| `--isolate-network` | Runs oracle probes and oracle suites under `unshare -Urn`; requires permitted user/network namespaces |

The gate exits immediately on failure and does not print a PASS footer. Bad
arguments exit 2; missing prerequisites, missing oracles or wrong versions exit 3;
a failing check retains its command's status. Base mode prints that external
oracles were **not** run.

`cargo metadata` is used to locate the actual target directory, so
`CARGO_TARGET_DIR` and Cargo target configuration are respected. This is a native
gate, not a cross-build: `CARGO_BUILD_TARGET` is rejected and a configured
non-native target fails the binary check.

## Oracle provisioning and pins

The gate never silently downloads an oracle. With network access, install the
pinned versions manually, in CI, or with a maintainer script, then let the gate
verify version banners and resolved paths. Pins and checksums are in
[PINS.md](PINS.md) and [PINS-QC.md](PINS-QC.md). A matching banner is not an
authentication of the bytes; provisioning must also verify the documented
checksums where available.

Resolution order for each oracle: explicit `SEQKIT_ORACLE`, `FQ_ORACLE`,
`FASTQC_ORACLE`, `MULTIQC_ORACLE`; otherwise `tests/ci/env/bin`,
`CONDA_PREFIX/bin`, then `PATH`. A missing explicit override never falls back to a
different binary. The resolved paths are exported so the oracle scripts test the
same binaries whose versions were checked.

`MULTIQC_NO_VERSION_CHECK=true` is set for deterministic oracle runs. The QC
harness has its own self-test that needs no oracle.

## Safety and history

- `rust-umitools/`, oracle/expected files and pre-existing packaging changes are
  protected. See [docs/DOC-AUDIT.md](../../docs/DOC-AUDIT.md).
- `tests/ci/check_docs.py` prevents current-facing documentation from
  reintroducing retired constraints. Historical evidence may still mention the
  offline commands used in its recorded session.
- The gate does not publish, tag or commit.

## Continuous integration

[`.github/workflows/quality.yml`](../../.github/workflows/quality.yml) runs the base
gate on `ubuntu-latest` for main-branch pushes and pull requests. It installs the
pinned Rust toolchain and native build dependencies, then runs the negative
controls and `bash tests/ci/check.sh`.

A manual dispatch can request `fastx_qc_oracles`. That optional step expects the
pinned oracles to be pre-provisioned and fails clearly if they are absent; the
hosted workflow does not download or fabricate them. The workflow passed on
GitHub in quality run `40151959375`. Registering required checks and a release
gate remains a maintainer decision.

## Remaining integration

Full cutadapt differential, UMI exact/semantic/spill/determinism, BAM differential,
coverage-guided fuzz, MSRV, performance trends and packaged/cross-platform
artifacts are separate suites. They are not silently claimed by the base gate;
see [docs/DEVELOPMENT-PLAN.md](../../docs/DEVELOPMENT-PLAN.md).
