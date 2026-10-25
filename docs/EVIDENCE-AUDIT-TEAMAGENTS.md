# TeamAgents independent audit (2026-10-23)

> Historical snapshot. Verdict: PASS WITH FINDINGS. Findings were used as
> remediation input; see [EVIDENCE-AUDIT-REMEDIATION.md](EVIDENCE-AUDIT-REMEDIATION.md).
> Both delegated reviewers were blocked by their sandboxes, so the leader
> performed the inspection directly; this is not an external certification.

# Independent Audit — bioinfo-tools snapshot (/tmp/bioinfo-audit-team)

VERDICT: PASS WITH FINDINGS

The three hard safety claims survived direct testing: the allow-list is exactly satisfied by
every crate in `Cargo.lock` with no entry unused; `check licenses` passes while
`check advisories` fails for `custom_derive`; and `acceptance.sh` has no path that deletes or
re-records `tests/gate/golden`. Findings concern **scope of proof** and **decision
provenance**: the gate is safe because the wrapper has no write verbs, while the goldens have
no integrity binding and the "approved" policy has no approval record.

## Findings

| Severity | file:line | Evidence | Required action |
|---|---|---|---|
| high | `tests/gate/golden/` (40 files); `tests/gate/m3.sh:106-114` | C3 only *counts* goldens (`find … \| wc -l`); every digest check in `m3.sh` targets the two baseline binaries (`:65-66`), never the goldens. No `goldens.sha256` exists. Re-recording stays directly reachable: `snapshot.py:1207-1210` `record()` does `mkdir` + unlink-each over `--out`, `:1427` keeps `--record`, and `tests/gate/README.md:343-352` documents it as routine. | Commit a `goldens.sha256` and verify it in C3; mark `--record` maintainer-only, forbid committing its output unreviewed. |
| high | `docs/EVIDENCE-AUDIT-REMEDIATION.md:46,90-91` | Doc says "根目录没有 `deny.toml`" and asks whether to approve and enable the draft as root `deny.toml`, "批准前不声称 license gate 已正式通过". A root `deny.toml` exists (mtime 20:20:58 vs doc 20:10:19), headed "Approved workspace dependency policy (2026-10-23)" (`deny.toml:1`). No approver or decision record anywhere. | Record the decision (who/when/what) and update section 2, or relabel `deny.toml:1` draft-pending-approval. |
| medium | `deny.toml` (untracked); `tests/ci/check.sh`; `.github/workflows/quality.yml` | `git status` → `?? deny.toml`, `?? tests/security/`. grep "deny" across `quality.yml`, `check.sh`, `tests/ci/README.md`, `README.md`, `AGENTS.md`: zero hits. The advisory failure is visible only to a human running cargo-deny by hand. | Add a CI step `cargo deny --offline --locked check licenses` (advisories explicitly allowed to fail) and track the file. |
| medium | `tests/gate/test_acceptance.py:29-34` | Suite installs a **stub** `m3.sh`, so it never exercises the real gate. On a copy, changing the delegate to `exec bash tests/gate/m3.sh --record` left 7/7 OK — the re-recording regression it exists to prevent is invisible. It did catch golden deletion (5 failures) and a re-accepted `--record` flag (4 failures). | Assert the exact delegated argv; add one integration case against real `m3.sh` with `GOLDEN_DIR` copied to a temp dir. |
| medium | `tests/gate/m3.sh:33` via `tests/gate/acceptance.sh:31` | `acceptance.sh` forwards the environment unscrubbed and `GOLDEN_DIR=${GOLDEN_DIR:-tests/gate/golden}`. Demonstrated: `GOLDEN_DIR=/tmp/a3/forged/g … --verify` → `m3 saw GOLDEN_DIR=/tmp/a3/forged/g`, rc=0. A `PATH` shim is likewise inherited — a forged `python3` executed inside the gate. These re-point *what* is compared; they cannot corrupt committed goldens but can green the gate against forged ones. | Pin `GOLDEN_DIR` in `acceptance.sh` or refuse the override; at minimum name active overrides in the PASS summary. |
| medium | `docs/EVIDENCE-FINAL.json:46` vs `:83`,`:41` | `"audit_status": "passed"` sits beside `"current_worktree_audit_status": "not independently re-audited"` and `"protected_paths_clean": false`. | Rename to `pre_self_check_audit_status` or require a scope sibling. |
| medium | `tests/ci/check_docs.py:150` | Docs gate *requires* `audit_status == "passed"` and never inspects the worktree status or protected-path flag, so it pins the optimistic field and cannot catch a future overclaim. | Validate scope fields; fail when the two statuses contradict. |
| medium | `docs/DEVELOPMENT-PLAN.md:220` | Row reads "…独立审计已通过" with no scope qualifier, unlike `EVIDENCE-FINAL.md:4`, `DEVELOPMENT-PLAN.md:4` and `DOC-AUDIT.md:6`, which all say "pre-self-check snapshot, not re-audited". | Add the qualifier. |
| low | `deny.toml:22` | `multiple-versions = "warn"` with 6 duplicate pairs (`derive-new`, `shlex`, `syn`, `synstructure`, `thiserror`, `thiserror-impl`). Setting `deny` makes `bans FAILED` (tested), so duplicates can never block. | Document the tolerance or tighten for `syn`/`thiserror`. |
| low | `tests/gate/test_acceptance.py:86-89` | `test_missing_verifier_fails_closed` asserts only `!= 0`; actual is 127, so any breakage satisfies it. | Assert the exact code. |
| low | `tests/gate/m3.sh:26` | `ROOT=$(cd "$(dirname "$0")/../.." …)`: via symlink ROOT follows the link's parent. Demonstrated — a symlinked `acc.sh` in a forged tree ran **that tree's** `m3.sh`, rc=0. Ordinary misuse fails closed (127). | Use `${BASH_SOURCE[0]}` + `readlink -f`. |
| low | `tests/gate/acceptance.sh:23-26` | `--help`/`-h`/no-arg exit **0**, the same code as a passing verification (measured: no-arg 0, `--help` 0, `--record` 2, `("")` 2, `--verify --record` 2, missing `m3.sh` 127). A CI wrapper keyed on exit status cannot tell "help printed" from "gate passed". | Exit 2/64 for help, or require `--verify`. |
| low | `Cargo.lock:795-798` | `newtype_derive` 0.1.6 is `custom_derive`'s sibling (same author/era) with **no** RustSec entry, so advisories give false comfort that removing `custom_derive` cleans the family. | Track both. |
| info | `deny.toml:10-18` | All seven entries are load-bearing — removing each individually turned `licenses` to FAILED. Header says the list "matches the current Cargo.lock inventory": fitted to today's lock, and blanket-allowing those licences for all future crates. | State it is a permanent policy choice, not a snapshot of the current lock. |

## Claims verified (independently)

1. **Allow-list exactness.** Parsed all 162 `Cargo.lock` entries (155 crates.io, 7 path members, 0 git), read each cached `Cargo.toml`, and expanded every license expression to disjunctive normal form: **162/162 have a fully-allowed OR-path; 0 unsatisfiable; 0 missing `license`**. Non-allow tokens appear only in OR branches (`0BSD` adler2; `Unlicense` aho-corasick/byteorder/memchr; `LGPL-2.1-or-later` r-efi) and are all satisfied by an allowed branch — no unreviewed licence is exercised. Not too broad: each entry is needed (removal test above).
2. **Deny behaviour.** On a copy, `cargo-deny 0.20.2 --offline --locked`: `check licenses` → `licenses ok`; `check advisories` → `advisories FAILED`, `RUSTSEC-2025-0058`, "No safe upgrade is available"; full `check` → `advisories FAILED, bans ok, licenses ok, sources ok`, exit 1; `check sources` → ok. `[advisories].ignore = []` at `deny.toml:7`.
3. **custom_derive is build-time only.** `rust-umitools/Cargo.toml:25` → `rust-htslib 0.49`, edge at `Cargo.lock:1054-1061`. `rust-htslib-0.49.0/src/lib.rs:112` `extern crate custom_derive;`; invocations at `src/bam/record.rs:1741`, `src/bcf/record.rs:1220`, `src/bcf/header.rs:49`. The crate is 462 lines of `macro_rules!` (`src/lib.rs:112`), `license = "MIT/Apache-2.0"`, **no `build.rs`, no `proc-macro`, no `unsafe` in `src/`**. So "compile-time macro helper for newtype trait impls, not runtime BAM/BCF processing" is **accurate**. Nuance: it generates the `NewtypeDeref/DerefMut/Index/IndexMut/From` impls used *by* BAM/BCF types (`bam/record.rs:1761-1765` for `CigarString`), so it shapes runtime behaviour through codegen without running then. Real risk: unmaintained codegen, no patched version, no unsafe/FFI/IO surface, no data-path exposure.
4. **Upgrade is not a fix.** `rust-htslib-1.0.1/Cargo.toml:79` still declares `custom_derive`.
5. **Advisory class.** Local `RUSTSEC-2025-0058`: `informational = "unmaintained"`, `patched = []`, DB commit `f7b2f6bc…`. Not a known vulnerability.
6. **Entry point cannot delete or re-record.** `acceptance.sh` has no `rm`/`mv`/`cp`/`mkdir`/`touch`/`chmod`/`tee`, redirects only to `/dev/null`, validates the whole argv (`:18-27`), then `exec bash tests/gate/m3.sh` (`:31`). `m3.sh` writes nothing and calls only `snapshot.py --verify` (`:143`) and `--self-test` (`:163`); `--record` appears nowhere in it. `verify()` (`snapshot.py:1259`) reads only; `self_test()` (`:1380`) mutates a `copytree` inside a `TemporaryDirectory` (`:1393-1398`). On a copy, `--record` without `--cutadapt/--umitools` errored out and the golden digest was unchanged; the only destructive writes live in `record()` (`:1208-1210`, `:1242`), unreachable from the wrapper.
7. **Argument matrix** (copy): no-arg 0, `""` 2, `--help` 0, `-h` 0, `--record` 2, `--unknown` 2, `--` 2, two args 2, `--verify` without `m3.sh` 127; `bash -n` clean. `${1---help}` correctly yields `--help` when `$1` is unset, and the literal empty argument exits 2 rather than 0.
8. **Coverage arithmetic and scoping.** All 12 fractions reproduce their quoted percentages exactly (72.80/71.52/73.51/54.11/35.83/21.18 and 83.54/79.75/84.75/67.80/72.31/55.98) with identical denominators across both columns — consistent with "same instrumented build plus extra workloads". Caveats present and correct: experimental nightly branch instrumentation, not MC/DC, no C/C++ FFI, not comparable to the older stable 73.47% line-only run (`EVIDENCE-AUDIT-REMEDIATION.md:148`, `verification/README.md:70-73`, `EVIDENCE-FINAL.md:49`). Repro commands are coherent: `exact.py:23` reads `<CARGO_TARGET_DIR>/release/rust-umitools` and the documented symlink matches; `CARGO_TARGET_DIR` stays exported for the `llvm-cov report` step; the symlink is honestly not called a release artefact.
9. **Protected paths.** `git status --porcelain rust-umitools/` → exactly the 3 claimed files (`tests/ci/check.sh`, `tests/ci/env.sh`, `tests/compat/matrix.py`), none runtime source; `tests/gate/golden` and `rust-cutadapt/tests/fixtures` clean. Docs say "该目录不是 clean" (`EVIDENCE-FINAL.md:30`, `DOC-AUDIT.md:4`, `EVIDENCE-AUDIT-REMEDIATION.md:162`) instead of claiming cleanliness — consistent.
10. **Static doc counts.** 40 golden JSONs; 7 `def test_` in `test_acceptance.py`; 12 in `test_check.py`; `check_docs.py` exits 0.

## Not verified

- **No build, test, or coverage run.** `target/` is absent and `.work/w1-1-baseline/` does not exist; I ran no cargo build/test/fmt/clippy and did not attempt coverage. The coverage figures are verified only for arithmetic and internal consistency.
- "718 passed", "50/50", "22/22", "77/77 byte-identical", "9/9 criteria", `SELFTEST PASS` — read from docs, not reproduced.
- `m3.sh` here reports `2 passed, 7 failed` because baselines, binaries and oracle are missing — infrastructure, not a verdict.
- GitHub workflow never executed; no required-checks policy.
- Whether the historical "independent audit" happened — only its self-consistent scoping is checkable.

## Team note

I delegated both scopes. `cs-review-2` returned **BLOCKED**; `ed-review-2` produced no
deliverable. Neither instance has a shell tool and both sandboxes reject `/tmp/...` with
`path escapes workspace`, so they opened zero snapshot files. No reviewer findings are
incorporated — every finding above is from my own inspection. The fix is environmental
(mount the snapshot in their workspace or grant exec), not more retries.

## Modifications

**No.** Nothing under `/tmp/bioinfo-audit-team` was modified, created, deleted, renamed or
chmod-ed; no `git` write command and no cargo run in the workspace. `git status --porcelain`
md5 `ada65e8d986b2df539bce0c4b6725235` at 70 entries both before and after. All mutations
(entry-point tampering, `deny.toml` edits, cargo-deny runs, symlink and PATH-shim probes)
were performed on copies in `/tmp/a2` and `/tmp/a3`.
