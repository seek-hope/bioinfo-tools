# Current-state evidence: October 2026

> Historical record: commands, versions and offline flags describe the session at the recorded commit. They are not current project requirements; see [DOC-AUDIT.md](../../docs/DOC-AUDIT.md).

Status: **recorded and reproducible on this machine**.
Scope: W0-3 of the workspace development plan. This file records commands,
versions, commits and results; it does not introduce a new compatibility claim.

Canonical machine-readable file:
[EVIDENCE-2026-10.json](EVIDENCE-2026-10.json).

## Summary

| Area | Result |
| --- | --- |
| `rust-cutadapt` code | `9d9abdf` on `main` |
| `rust-cutadapt` Rust tests | 208 passed |
| `rust-cutadapt` differential | 119/119 passed against cutadapt 4.6 |
| `rust-umitools` code | `40b5b70` on `main` |
| `rust-umitools` byte-exact suite | 77/77 passed |
| `rust-umitools` semantic suite | 39 passed, 0 failed |
| `rust-cutadapt` benchmark | 0.672 s mean; 6.60x baseline; 5x target met |
| Hardcoded `/home/` paths in either `tests/` tree | none |
| Missing-oracle exit code | 3 in both oracle harnesses |
| CI | both successful at the recorded commits |

No tag and no release was created.

## Repositories and binaries

| Repository | Commit | Branch | Version | Binary SHA256 |
| --- | --- | --- | --- | --- |
| `rust-cutadapt` | `9d9abdf38fe0b46093df7166f38fa1bdc4279c65` | `main` | 0.1.0 | `c98c28990e9439f9d9cd1f75d4001dec0f8de7fa31baafe52f22db54ee4099de` |
| `rust-umitools` | `40b5b709247e84e00d81ca05c3b37c107a9f014d` | `main` | 0.2.0 | `df563faa8d27abfa8055f972f2ed7a8a2765a2da59dbdc22ce435c9dc19f6f83` |

The `rust-cutadapt` binary is `target/release/rust-cutadapt`. The
`rust-umitools` binary is resolved through `CARGO_TARGET_DIR` (the test
environment uses `/tmp/rust-umitools-build/target/release/rust-umitools`).

CI runs:

| Repository | Run | Commit | Result |
| --- | --- | --- | --- |
| `rust-cutadapt` | `39354372681` | `9d9abdf` | success |
| `rust-umitools` | `39354542940` | `40b5b70` | success |

## Oracle and dependency versions

| Tool | Version | How resolved |
| --- | --- | --- |
| cutadapt | 4.6 | `CUTADAPT_ORACLE`, then `tests/ci/env.yml` prefix, `CONDA_PREFIX`, `.work/oracle-venv`, `PATH` |
| UMI-tools | 1.1.6 | `UMI_TOOLS_ORACLE_PATH`, then `tests/ci/env.yml` prefix, `CONDA_PREFIX`, `PATH` |
| samtools | 1.24 | same oracle environment as UMI-tools |
| Python | 3.12.14 | oracle environment |
| NumPy | 2.5.3 | oracle environment |
| pysam | 0.24.1 | oracle environment |

No absolute oracle path is stored in either repository's `tests/` tree. The
commands below use `<pinned-oracle-bin>` as a placeholder for the environment
selected by the resolver.

## Commands and results

### rust-cutadapt

```text
$ cargo test --offline --release
test result: ok. 171 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
$ python3 tests/oracle/differential.py --matrix
119/119 cases passed
```

### rust-umitools

```text
$ UMI_TOOLS_ORACLE_PATH=<pinned-oracle-bin> bash tests/oracle/run_exact.sh
77/77 byte-identical, 0 differ
```

```text
$ UMI_TOOLS_ORACLE_PATH=<pinned-oracle-bin> bash tests/oracle/run_oracle.sh
39 passed, 0 failed
```

### Portability

```text
$ rg -n '/home/' tests
(no matches, both repositories)
```

```text
$ env -u UMI_TOOLS_ORACLE_PATH -u CONDA_PREFIX PATH=/usr/bin:/bin \
    bash tests/oracle/run_exact.sh
ERROR: umi_tools not found. Set UMI_TOOLS_ORACLE_PATH or add it to PATH.
exit=3
```

The cutadapt harness also exits 3 when the oracle is missing.

## Benchmark

The full-surface measurement is recorded in
[bench/cutadapt-c8b0249.json](bench/cutadapt-c8b0249.json). Summary:

| Item | Value |
| --- | --- |
| Code commit | `c8b0249` |
| Default workers | 4 |
| Input SHA256 | `b9e9ae7c967731fa728f589f9e45c97c059904ff8198656dc2fed1374408e5dc` |
| Binary SHA256 | `c98c28990e9439f9d9cd1f75d4001dec0f8de7fa31baafe52f22db54ee4099de` |
| Wall time | 0.62–0.72 s; mean 0.672 s |
| Peak RSS | 41,416 kB |
| Speedup vs recorded 4.4363 s baseline | 6.60x |
| Speedup vs same-session oracle | 14.36x |
| 5x target | met |
| Decompressed output | byte-identical to cutadapt 4.6 |

The command was:

```bash
./target/release/rust-cutadapt --quiet -a AGATCGGAAGAGC -m 10 \
  -o <out.fastq.gz> bench/workload.fastq.gz
```

Caveats:

- `hyperfine` was not installed. The evidence uses five independent
  `/usr/bin/time -v` runs, not hyperfine JSON. This is why G-M2's
  "raw hyperfine file" item is not claimed.
- The machine is shared; the same-session cutadapt run took 9.65 s while the
  recorded baseline is 4.4363 s. Both ratios are reported in the JSON.
- Four workers is a deliberate deviation from cutadapt's default of one; see
  [adr/0002-default-worker-count.md](adr/0002-default-worker-count.md).
  `-j 1` reproduces the 2.352 s single-thread result.

## What is explicitly not claimed

- A fresh-container rebuild was not performed. The evidence is from the
  current machine with the pinned oracle environments.
- `hyperfine` JSON and a hardware inventory are absent from this evidence.
- No compressed-byte equality is claimed; only decompressed bytes.
- No release, tag, dist upload or crates.io publication was made.

## Evidence schema (version 1)

The JSON file is valid when it has these top-level keys:

`schema_version`, `kind`, `generated_at_utc`, `workspace`, `repositories`,
`ci`, `oracles`, `suites`, `benchmark`, `portability_checks`, `notes`.

Each `suites` entry must have `name`, `command`, `result` and `exit_code`.
The `benchmark` entry must have `code_commit`, `input_sha256`,
`binary_sha256`, `wall_mean_s`, `speedup_vs_reference_baseline`,
`five_x_target_met` and `decompressed_output_matches_oracle`.
