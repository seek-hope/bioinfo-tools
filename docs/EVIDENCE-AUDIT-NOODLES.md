# 独立只读审计：rust-umitools noodles migration

- Verdict: **PASS WITH FINDINGS** (the HIGH empty-input finding was resolved in the authorized remediation; medium limitations remain)
- Audit date: 2026-10-24
- Audit scope: current working tree, commit `49953ec`, dirty worktree
- Mode: initial read-only audit; separate authorized remediation for the HIGH finding; no commit/tag/release
- Auditor: goal-directed independent review in this session

## 1. Findings

| Severity | Location | Evidence | Required action |
| --- | --- | --- | --- |
| **HIGH — RESOLVED** | `rust-umitools/src/io/bam.rs:615-618`, `rust-umitools/src/commands/dedup.rs:65-69` | Historical finding: empty input with both `dedup --no-sort-output` and default sorted output returned **exit 0** but wrote a **0-byte invalid BAM**. The W1-1 htslib baseline wrote a valid 195-byte empty BAM. | Fixed: `Writer::finish_inner` calls `ensure_open()` before taking the inner writer; a `finished` guard prevents the Drop that runs after consuming `finish()` from reopening and truncating the output. `Sink::Direct` and sorted/group/count writers call fallible `finish()` before commit. `tests/empty_input.rs` verifies both paths, header parsing and BGZF EOF. |
| MEDIUM — RESOLVED | `rust-umitools/src/io/bam.rs:638-641`, `rust-umitools/src/commands/dedup.rs:68` | Historical finding: `Drop` ignored finish errors and `Sink::Direct` dropped the writer. | Fixed: direct and sorted/group/count paths use fallible `finish()` before commit; Drop remains only a best-effort fallback. |
| MEDIUM | `rust-umitools/src/io/bam.rs:453-466` | CRAM input is buffered into a `Vec` before iteration because the current noodles CRAM iterator borrows reader+header. BAM/SAM remain streaming, but CRAM memory grows with file size. | Document as a CRAM-only limitation (already in ADR/evidence); prefer a stored-iterator/self-referential adapter when noodles exposes one. |
| MEDIUM | `rust-umitools/src/io/bam.rs:431,550` | `Reader::set_threads` and `Writer::set_threads` are no-ops; the noodles BGZF adapter does not honor the old BGZF worker budget. Rayon work remains, but I/O threading behavior changed. | Document the performance/threading limitation; add a larger I/O benchmark if BGZF threading is part of the contract. |
| MEDIUM | `rust-umitools/src/main.rs:85,109`, `bioinfo-common/src/provenance.rs:170,188` | `--version --verbose` still prints `htslib : via rust-htslib...` while the dependency is gone. It is retained for the recorded golden contract, but it is a false dependency claim. | Keep only as an explicitly documented compatibility label until a separately approved golden/contract update replaces it. |
| LOW | `rust-umitools/src/io/bam.rs:136` | `Value::Array` is converted to an empty `Aux::String`; an UMI tag supplied as a BAM array would be silently treated as an empty string rather than rejected as a non-string/non-numeric tag. UMI paths do not use array tags today. | Reject unsupported array values with the documented format/tag error, or implement array handling. |

## 2. Remediation (authorized after the audit)

The HIGH and related finish-error findings were fixed in the authorized remediation:

- `rust-umitools/src/io/bam.rs`: `finish_inner` calls `ensure_open()` so zero-record writers emit a header/EOF even if never written; a `finished` flag makes the post-finish Drop a no-op instead of reopening/truncating the file.
- `rust-umitools/src/commands/dedup.rs`: `Sink::Direct` calls `finish()` and propagates the error before `TempOutput::commit`.
- `rust-umitools/src/io/sort.rs`: empty, spill-run and merge writers call fallible `finish()`.
- `rust-umitools/src/commands/group.rs`, `count.rs`: output/partition writers call fallible `finish()` instead of dropping.
- `rust-umitools/tests/empty_input.rs`: verifies direct and sorted empty BAM output, parsable header, and canonical BGZF EOF; a unit test covers finish error propagation.
- `rust-cutadapt/tests/cli_e2e.rs`: test-only `run_in` helper with `.current_dir(dir)` for the option-matrix test; keeps relative outputs (i.txt/r.txt/ts.fastq/tl.fastq/u.fastq/r.json) in the TempDir instead of the repository. This scope extension was explicitly authorized in generation 28; assertions, golden, fixtures and production behavior are unchanged.

The audit report is now a PASS WITH FINDINGS; the remaining medium/low limitations are the CRAM input buffer, BGZF `set_threads` no-op, the legacy provenance label, and `Aux::Array` handling.

## 3. Verified

- K1: `cargo fmt --all -- --check` exit 0.
- K3: workspace release **720 passed**.
- K5: `QUALITY PASS`; FASTX 50/50, QC 22/22, MultiQC/SELFTEST PASS.
- K2/K4/K5/K6/K7/K8 in this audit:
  - `! grep rust-htslib rust-umitools/Cargo.toml` → 0
  - `! grep custom_derive/newtype_derive Cargo.lock` → 0
  - `tests/ci/check_docs.py` → PASS
  - `git diff --check` → 0
  - `sha256sum -c tests/gate/golden.sha256` → 0
  - `json.tool docs/EVIDENCE-FINAL.json` → 0
  - `tests/gate/test_acceptance.py` → 9 tests, OK
- `rust-htslib`, `custom_derive`, `newtype_derive` are absent from `Cargo.toml`/`Cargo.lock` and the dependency tree.
- UMI exact **77/77**; formats/spill/determinism/random/extract/oracle and M3 10/10 evidence is consistent with the migration documentation.
- `tests/gate/golden` and `rust-cutadapt/tests/fixtures` hashes are unchanged; no commit/tag/release.

## 4. Not verified / boundaries

- Remote GitHub CI was not executed.
- No signing, tag, upload or release was performed.
- This audit did not modify source code; the empty-input defect was reproduced with temporary files only.
- Scope extension (generation 28): the test-only `rust-cutadapt/tests/cli_e2e.rs` TempDir cwd fix is explicitly authorized and documented; production changes remain limited to rust-umitools writer/lifecycle code/tests/evidence. K17 records the full change set; K18 checks golden/fixtures unchanged.
- The audit did not rerun the full 200k performance matrix; the existing evidence is reviewed, not independently remeasured.
- The audit did not perform fault injection for the `Drop`/finish error path; the finding is code inspection plus normal-path reproduction.

## 5. Output manifest

| Artifact | Path | Role |
| --- | --- | --- |
| This audit report | `docs/EVIDENCE-AUDIT-NOODLES.md` | verdict, findings, verified/not verified |
| Migration evidence | `docs/EVIDENCE-NOODLES.md` | functional and performance evidence |
| Migration ADR | `rust-umitools/docs/adr/0001-noodles-migration.md` | dependency/design decision |
| Golden integrity manifest | `tests/gate/golden.sha256` | 40 golden SHA-256 entries |
| Existing final evidence | `docs/EVIDENCE-FINAL.md`, `docs/EVIDENCE-FINAL.json` | historical/current evidence status |
| Code reviewed | `rust-umitools/src/io/bam.rs` | noodles adapter |
| Code reviewed | `rust-umitools/src/commands/dedup.rs` | direct/sorted sink lifecycle |
| Regression test | `rust-umitools/tests/empty_input.rs` | empty direct/sorted BAM + finish error |

## 6. Provenance

- Base commit: `49953ec50ae04b343123a214b77030ff31f39a4b`
- Working tree: dirty (migration and evidence changes are uncommitted)
- Rust: `rustc 1.99.1`; workspace MSRV 1.91
- noodles: `0.118.0`; noodles-cram `0.101.0`; noodles-fasta `0.68.0`
- UMI-tools oracle: 1.1.6; samtools 1.24; Python 3.12.14; numpy 2.5.3; pysam 0.24.1
- No external citations or fabricated data are used.
- Environment: local Linux host; no remote CI.

## 7. Reproducible commands

```bash
# Dependency checks
! grep -q "rust-htslib" rust-umitools/Cargo.toml
! grep -q 'name = "custom_derive"' Cargo.lock
! grep -q 'name = "newtype_derive"' Cargo.lock

# Documentation, integrity and safety gates
python3 tests/ci/check_docs.py
git diff --check
sha256sum -c tests/gate/golden.sha256
python3 -m json.tool docs/EVIDENCE-FINAL.json >/dev/null
python3 tests/gate/test_acceptance.py

# Functional evidence (existing logs / reproducible)
bash tests/ci/check.sh --with-fastx-qc-oracles
bash tests/gate/acceptance.sh --verify
bash rust-umitools/tests/oracle/run_exact.sh
CARGO_DENY_OFFLINE=1 bash tests/security/check.sh --licenses

# Reproduce the high finding with an empty BAM
samtools view -b -o empty.bam empty.sam   # header only, no records
target/release/rust-umitools dedup -I empty.bam -S out.bam --no-sort-output --quiet
stat -c '%s' out.bam
samtools quickcheck out.bam
```

Pre-fix historical observation (before the remediation): `exit=0`, output size `0`, `samtools quickcheck` exit `4` (not sequence data).

Post-fix current result (K13/K14, generation 28): both `--no-sort-output` and default sorted output are exit 0, and `samtools quickcheck -v` exits `0` on both. The recorded run produced `257` bytes on both paths. The exact BAM byte count can vary with the encoded `@PG CL` header (for example, the extra `--no-sort-output` argument); it is not part of the validity contract.

## 8. Conclusion

The HIGH empty-input defect has been fixed by the authorized remediation and is covered by a regression test. The audit is **PASS WITH FINDINGS**; the remaining medium/low limitations are documented above. Independent re-audit is the final completion gate.

The original audit was read-only; remediation was separately authorized and did not commit, tag, upload or release.

## 9. Post-fix C3 oracle receipts

Added after the independent audit requested current formats/secondary evidence. These runs use the remediated workspace, including the writer `ensure_open`/`finish` changes.

- K15 `bash rust-umitools/tests/oracle/run_formats.sh`: exit 0; `formats ok: 0 failures`. Covered PASS: stdin to file, file to stdout, closed stdout pipe, chunked stdin backpressure, CRAM output with reference, CRAM decodes, CRAM input, CRAM output without reference refused, summary JSON schema.
- K16 `run_spill.sh`: `bounded memory ok: 0 failures` (9 PASS), including disk bundle cluster/directional/adjacency and resource-error case.
- K16 `run_determinism.sh`: `deterministic: 0 mismatches` (10 PASS across 1/2/8/16 threads and spill/sort/group variants).
- K16 `run_random_diff.sh`: `random differential ok: 0 failures over 12 trials`.
- K16 `run_extract.sh`: 11 passed + 9 passed, 0 failed.
- K16 `run_oracle.sh`: 39 passed, 0 failed.

These are current post-fix results; no golden or fixture was modified. The formal approved checks are K15 and K16.

- K17 `set -e; git status --porcelain; git diff --name-only; git diff --cached --name-only; git rev-parse HEAD; git tag --points-at HEAD`: records the complete change set, HEAD and tags; no commit/tag/release.
- K18 `git diff --exit-code -- tests/gate/golden rust-cutadapt/tests/fixtures; git status --porcelain -- tests/gate/golden rust-cutadapt/tests/fixtures`: fixture/golden integrity evidence.
