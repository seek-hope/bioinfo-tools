# Targeted verification

This directory holds the targeted verification layer. It is deliberately
separate from the main Cargo workspace so that proof/test toolchains do not
change normal builds.

## Commands

```bash
bash tests/verify/check.sh --fast   # proptest + Miri on bioinfo-common
bash tests/verify/check.sh --full   # also Kani + TLA+
```

`--fast` fails if the pinned Miri toolchain/component is unavailable. `--full`
fails if `cargo-kani` or `tla2tools.jar` is absent; it never silently skips a
layer.

## Coverage matrix

| Layer | Implemented | What it establishes | Explicit gaps |
| --- | --- | --- | --- |
| proptest (9) | chunker reassembly; generated FASTQ parsing; arbitrary-byte parser no-panic (strict/lenient); histogram vs sorted N50/quartiles; `SortKey` total order; `PartPlan` range coverage; `parts_for_size` bound; QC limit rejection; adapter-total multiset counts | Algebraic/property invariants over generated inputs | Not a full parser grammar proof; no UMI/BAM index/FastQC module properties |
| Miri (39) | `bioinfo-common` library tests | No detected undefined behavior in the exercised shared-contract code | Only `bioinfo-common`; other crates are not under Miri |
| Kani (5) | chunker boundary; `parts_for_size` bound; `SortKey` antisymmetry; N50 bound; empty histogram | Bounded no-panic/boundary properties | Small finite domains; no parser/I/O/allocation proof; no UMI, BAM index, or FastQC semantics |
| TLA+ (2) | `CutadaptPipeline` (`NoLoss`, `BoundedQueue`); `BamSort` (`NoLoss`, `NoDuplicate`) | Design-level protocol/model invariants with small constants | Models omit compression, formats, I/O failures and biological correctness; not code proofs |

The differential oracles remain the compatibility authority. These checks are
targeted evidence, not a claim of whole-project formal verification.

## Toolchain pins

- Miri: `nightly-2026-05-28`
- Kani/CBMC: `0.68.0` / `6.11.0`
- TLA+ TLC: `2026.10.23` (`verification/tla/tla2tools.jar`)
- proptest: workspace verification manifest

See `kani/README.md`, `tla/README.md` and `docs/EVIDENCE-FINAL.md` for the
detailed scope and known limits.

## Runtime line coverage

`cargo llvm-cov --workspace --release --tests --summary-only` was run with
`cargo-llvm-cov 0.9.1` and `llvm-tools-preview`:

| Metric | Result |
| --- | --- |
| Regions | 72.74% |
| Functions | 71.44% |
| Lines | 73.47% |
| Branches | not collected |

That earlier run included only the Rust tests, not the Python oracle workloads.
Python itself does not prevent Rust coverage collection: invoke an instrumented
Rust binary and inherit `LLVM_PROFILE_FILE` into its subprocesses.

### Follow-up: branch coverage and instrumented oracles

A separate debug build with `nightly-2026-05-28`, `cargo-llvm-cov 0.9.1`,
`--branch --workspace --tests --offline --locked` measured:

| Metric | Rust tests only | Same build + UMI exact and M3 oracles |
| --- | --- | --- |
| Regions | 72.80% | 83.54% |
| Functions | 71.52% | 79.75% |
| Lines | 73.51% | 84.75% |
| Branches | 54.11% (1778/3286) | 67.80% (2228/3286) |
| UMI lines | 35.83% | 72.31% |
| UMI branches | 21.18% | 55.98% |

The instrumented binaries passed `77/77 byte-identical, 0 differ` and `M3 PASS`
(9/9 criteria). These percentages are not directly comparable to the earlier
stable/release measurement. Branch coverage is experimental, not MC/DC or a
proof of completeness; FFI C/C++ code and other Python suites were not included.
Reproduction commands and audit limitations are in
[the remediation evidence](../docs/EVIDENCE-AUDIT-REMEDIATION.md).
