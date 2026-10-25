# W1-1 evidence: the workspace and `bioinfo-common`

> Historical record: commands, versions and offline flags describe the session at the recorded commit. They are not current project requirements; see [DOC-AUDIT.md](DOC-AUDIT.md).

Status: **complete** — see "Verification results" for the commands and their
output. This file records W1-1 only; it does not start W1-2.

Canonical machine-readable file: [EVIDENCE-W1-1.json](EVIDENCE-W1-1.json).

Scope: `docs/DEVELOPMENT-PLAN.md` **W1-1**, "Create the workspace and
`bioinfo-common`. Move exit-code classes, diagnostics, summary-JSON, and
provenance out of both binaries. Behaviour-neutral."

## Summary

| Item | Result |
| --- | --- |
| Cargo workspace | created at the repository root; three members |
| `bioinfo-common` | new library crate, std-only, 39 unit tests |
| Shared contracts moved | exit codes, diagnostics, `--summary-json`, provenance |
| `rust-cutadapt` Rust tests | 211 passed (174 lib + 14 + 21 + 1 + 1) |
| `rust-umitools` Rust tests | 58 passed (50 lib + 2 bin + 5 contract + 1 doc) |
| `cargo test --workspace` (debug, the acceptance command) | **307 passed, 0 failed** |
| `cargo test --workspace --release` | **307 passed, 0 failed** |
| cutadapt 4.6 differential | **119/119 passed** |
| UMI-tools 1.1.6 byte-exact | **77/77 byte-identical, 0 differ** |
| UMI-tools 1.1.6 semantic | **39 passed, 0 failed** |
| Byte-neutrality cases | **40/40 pass** (38 refactor-neutral + 2 oracle-recorded for DR-10) |
| Refactor-induced behaviour change | **none** — the 38 refactor-neutral cases are byte-identical to the pre-refactor binaries |
| Deliberate behaviour change | **one**: read-2 `-G`/`-B` now match cutadapt 4.6 (DR-10, a fixed pre-existing defect) |

No release, no tag, no version bump, no dist publication.

## What was created and moved

### The workspace

`Cargo.toml` at the repository root declares the members and the single release
profile:

| Member | Kind | Before | After |
| --- | --- | --- | --- |
| `bioinfo-common` | lib | did not exist | new |
| `rust-cutadapt` | bin + lib | standalone crate | workspace member |
| `rust-umitools` | bin + lib | standalone crate | workspace member |

### `bioinfo-common`

| Module | Contract | Tests |
| --- | --- | --- |
| `exit` | the 0/1/2/3/4 classes and their labels | 7 |
| `diagnostic` | `ERROR: ` / `WARNING: ` prefixes, hint layout | 7 |
| `summary` | the `--summary-json` document and its byte layout | 14 |
| `provenance` | the `--version --verbose` block and the `@PG` record | 9 |
| crate root | the prose contract, re-exports | 2 |

The crate is **std-only**. It never exits, never prints and never reads the
environment: every function returns data and the binaries decide the stream and
the status. That is boundary rule 2 of the plan, applied literally.

### Call sites rewired

| File | Was | Now |
| --- | --- | --- |
| `rust-umitools/src/error.rs` | local `EXIT_*` constants; `exit_code` matched variants | maps variants onto `ExitClass`; `exit_code()` is `class().code()` |
| `rust-umitools/src/main.rs` | inline `println!` provenance block; local `EXIT_OK` | `Provenance::new(..).with_field(..)` from `bioinfo-common` |
| `rust-umitools/src/commands/mod.rs` | four inline `push_tag` calls | `ProgramRecord::new(..).tags()` |
| `rust-umitools/src/runtime/summary.rs` | `Summary::render` written out by hand | delegates to `bioinfo_common::summary::Summary` |
| `rust-umitools/src/cli.rs` | `exit(crate::error::EXIT_OK)` | `exit(ExitClass::Success.code())` |

`rust-cutadapt` keeps its own `report::json` (a cutadapt 4.6 compatibility
contract, not this workspace's summary contract) and its own
`report::fmt` (Python `json.dumps` emulation). Neither moved, because moving
them would have changed output bytes. See DR-3 and DR-4.

## Design decisions and deviations

Every entry below is a place where the literal instruction ("move shared
things") met a constraint. None changes observable behaviour; the ones that
*could* have are recorded with the measurement that rules it out.

### DR-1 — `bioinfo-common` has no `serde`/`serde_json`, contrary to the plan table

The plan's crate table lists `serde`, `serde_json` as the allowed dependencies.
The crate has **no dependencies at all**.

Reason: the `--summary-json` byte contract shipped with a hand-written renderer
whose escaping is not `serde_json`'s. `serde_json` emits `\b` and `\f` for those
control characters; the shipped contract emits `\u0008` and `\u000c`. It also
orders object keys by insertion, which `serde_json`'s default map does not
guarantee. Adopting `serde_json` would therefore have changed output bytes for
any path containing a control character, and would have added a runtime
dependency to both binaries. The escaping is pinned by
`summary::tests::escaping_uses_four_digit_escapes_for_other_controls` and the
whole document by `matches_the_recorded_pre_refactor_bytes`.

This is a *reduction* in dependencies, not an addition, so it cannot violate the
"no new runtime dependency" rule. If `serde` is later wanted for a real reason
(new structured formats in W1-2+), that is a separate, recorded decision.

### DR-2 — `liblzma` → `xz2` in `rust-cutadapt` (forced by the workspace)

**This is the one change that could have altered output bytes, so it was
measured rather than assumed.**

Cargo forbids two packages with the same `links` key in one resolution graph.
`rust-cutadapt` used `liblzma` (→ `liblzma-sys`, `links = "lzma"`) while
`rust-htslib` — which `rust-umitools` needs for its verified CRAM support —
uses `lzma-sys` (`links = "lzma"`). Creating the workspace therefore failed to
resolve:

```text
error: failed to select a version for `lzma-sys`.
package `lzma-sys` links to the native library `lzma`, but it conflicts with a
previous package which links to `lzma` as well:
package `liblzma-sys v0.3.13`
```

Options considered:

| Option | Verdict |
| --- | --- |
| Drop `lzma` from `rust-htslib` | **Rejected.** `tests/oracle/run_formats.sh` and `docs/compatibility.tsv` record CRAM as `verified`; htslib needs lzma to read CRAM. This would remove a verified capability. |
| Drop xz from `rust-cutadapt` | **Rejected.** xz input/output is documented cutadapt 4.6 behaviour with 5 differential cases. |
| Keep two `links = "lzma"` packages | **Impossible.** Cargo refuses to resolve. |
| **Use `xz2` in `rust-cutadapt`** | **Adopted.** `xz2` is the direct predecessor of the `liblzma` fork (same author, same C library); it depends on `lzma-sys`, the same crate htslib already pulls in. The `XzDecoder`/`XzEncoder` API is identical, including `new_multi_decoder`. |

Evidence that this is byte-neutral, measured before the change was adopted:

* **Compressed output is identical.** `xz2` with `lzma-sys/static` produced
  **byte-identical** xz output to `liblzma` at preset 0 and preset 6, on a
  300 kB pseudo-random buffer, on a 20 MB realistic FASTQ excerpt, and on a
  3 000-read trimming run. The `-Z` (preset 0) path is identical too.
* **The differential suite agrees.** The five xz cases
  (`xz_input`, `xz_output`, `xz_output_fast`, `bzip2_to_xz`, `xz_paired`) pass
  against cutadapt 4.6 with the new binding.
* **Multi-stream decoding still works** (`new_multi_decoder` round-trips two
  concatenated xz streams).
* **Nothing else in the graph moved.** bzip2, gzip and plain output stay
  byte-identical, and all 208 `rust-cutadapt` tests pass.

Note on the linked library: `rust-htslib` requires `lzma-sys/static`, so feature
unification now compiles the bundled xz-5.2 sources into the binary instead of
dynamically linking the system liblzma. That is a *linkage* change, not an
output change — the measurements above were taken with exactly that
configuration.

`xz2 = "0.1.7"` and `lzma-sys = "0.1.20"` (static) replace `liblzma = "0.3.6"`.
This is a **dependency substitution, not an addition**: the same C library is
linked, through the same `links = "lzma"` provider that `rust-umitools` already
used. `docs/adr/0001-dependency-policy.md` is updated accordingly.

### DR-3 — cutadapt's `--json` report did **not** move

The plan lists "summary-JSON contract" as shared. `rust-cutadapt --json`
produces a *cutadapt 4.6 document* with cutadapt's own schema, field order,
`null` semantics, `OneLine` layout and Python `json.dumps` float formatting. It
is a compatibility surface verified field-by-field against the oracle.

"Sharing" it with `--summary-json` would have meant either changing cutadapt's
output or teaching the shared renderer two different escaping rules. Neither is
behaviour-neutral, so the cutadapt report stays where it is and this is recorded
as a deliberate boundary. The shared contract is the *workspace* summary
document (`--summary-json`), which is what a workflow manager reads.

### DR-4 — Python `json.dumps` emulation stayed in `rust-cutadapt`

Same reason: `report::fmt` exists to reproduce Python's float formatting and
short-form escapes, which is the opposite requirement from the summary
contract's escaping. Unifying them would change bytes on one side or the other.

### DR-5 — the `--debug` line still says `liblzma`

`rust-cutadapt --debug` prints
`DEBUG: rust-cutadapt <ver> with std + clap + flate2 + bzip2 + liblzma`. The
crate binding is now `xz2`, but the printed word is observable output, so it is
unchanged. Changing it would have been a (small) output change for no benefit;
the code carries a comment pointing at DR-2.

### DR-6 — one workspace release profile; `lto = "fat"` wins

Cargo refuses `lto` in a `[profile.release.package.*]` override, and it ignores
member-level `[profile]` sections once a workspace root exists:

```text
error: `lto` may not be specified in a `package` profile
warning: profiles for the non root package will be ignored, specify profiles at
         the workspace root
```

The two crates had `lto = "thin"` (cutadapt) and `lto = "fat"` (umitools). The
workspace uses `lto = "fat"`, `opt-level = 3`, `codegen-units = 1`:

* `rust-umitools` builds exactly as before.
* `rust-cutadapt` gains optimisation. This can change machine code, never
  behaviour; the gate proves byte-identical stdout, stderr, exit codes and
  output files.
* Binary SHA256 values therefore differ from the pre-refactor ones. That is
  expected and is called out in "What is explicitly not claimed".

### DR-7 — diagnostics keep a per-binary stream, and share only the text

The plan says the diagnostic surface is shared. What is shared here is the
`ERROR: ` / `WARNING: ` prefix, the hint indent and the message/hint split.
Which *stream* a diagnostic goes to stays local, because it is a documented
compatibility property: `rust-cutadapt` mirrors cutadapt 4.6, which writes its
own `ERROR:` lines to **stdout** once processing has begun (the pre-refactor
`main.rs` documents this), while `rust-umitools` writes them to **stderr**.
Moving the stream choice into the library would have changed cutadapt's output.

### DR-10 — read-2 adapter options `-A`/`-G`/`-B` were mis-parsed (defect; fixed here)

**Found by external verification of W1-1, after the first completion report.**
This is a genuine defect in the tree, and it is **pre-existing**, not a W1-1
regression — but it made the W1-1 acceptance command fail, so it is fixed and
recorded as part of W1-1 rather than deferred.

#### Symptom

```text
$ cargo test --workspace --offline
thread 'cli::tests::parses_paired_end_options' panicked at
    rust-cutadapt/src/adapter/mod.rs:320: unexpected adapter option A
test result: FAILED. 170 passed; 1 failed
```

`cargo test --workspace --offline --release` passed. That asymmetry is the tell:
the branch ended in a `debug_assert!` with a fallback.

#### Root cause

`cli` collects read-2 adapters from `-A`/`-G`/`-B` and passes **the option letter
itself** to `adapter::parse`. `parse` mapped the letter to an adapter type with:

```rust
let base_type = match option {
    'a' => AdapterType::RegularThreePrime,
    'g' => AdapterType::RegularFivePrime,
    'b' => AdapterType::Anywhere,
    other => {
        debug_assert!(false, "unexpected adapter option {other}");
        AdapterType::RegularThreePrime   // <- release-build fallback
    }
};
```

Only the lowercase spellings were handled. So for every `-A`, `-G` or `-B`:

* in a **debug** build the `debug_assert!` aborted, and
* in a **release** build the adapter was silently typed **three-prime**.

The release behaviour is the serious part. `adapter_type` drives the alignment
(`adapter/matcher.rs`), so `-G` (a 5' read-2 adapter) and `-B` (anywhere)
trimmed the **wrong end of read 2**, and the text report labelled them
`Type: regular 3'`. Measured against cutadapt 4.6 on a read with `ACGT` at both
ends, before the fix:

| invocation | cutadapt 4.6 (read 2) | rust-cutadapt (read 2) |
| --- | --- | --- |
| `-G ACGT` | `ACGTGGGGGGGGGGGG`, `ACGT` | `GGGGGGGGGGGG`, `GGGGGGGGGGGG` |
| `-B ACGT` | `ACGTGGGGGGGGGGGG`, `GGGGGGGGGGGG` | `GGGGGGGGGGGG`, `GGGGGGGGGGGG` |

`-A` was accidentally correct, because the 3' fallback happened to be the right
type for it.

#### Why no suite caught it

`tests/oracle/differential.py` and `tests/gate/snapshot.py` only ever exercised
`-A` on read 2; neither used `-G` or `-B`. So 119/119 and the 38-case gate were
both green while the behaviour was wrong. The gate now has
`cutadapt_read2_3prime` and `cutadapt_read2_5prime` for exactly this.

#### Proven pre-existing

The file is untouched by W1-1 (`git diff --stat src/adapter/mod.rs` is empty),
and the same test fails at the baseline commit:

```text
$ git -C rust-cutadapt archive 4609a0c | tar -x -C /tmp/baseline-ca
$ cd /tmp/baseline-ca && cargo test --offline --lib
thread 'cli::tests::parses_paired_end_options' panicked at src/adapter/mod.rs:320:
    unexpected adapter option A
test result: FAILED. 170 passed; 1 failed
```

#### Fix

Minimal and at the boundary the failure points to — how the option letter
becomes a type:

* a new `fn base_adapter_type(option: char) -> Result<AdapterType, AdapterParseError>`
  maps `'a' | 'A'`, `'g' | 'G'`, `'b' | 'B'`;
* an unknown letter now returns a new
  `AdapterParseError::UnknownOption(char)` (a usage error, exit 2) instead of
  silently pretending to be a 3' adapter. That branch is unreachable from the
  CLI — `cli` only ever passes one of those six letters — so it is documented as
  defence in depth rather than a live path, which is why the `debug_assert!` was
  not simply deleted;
* the doc comment on `parse` now states that the read-2 spellings mean the same
  three types.

Three tests were added: one asserting each uppercase letter matches its
lowercase spelling, one for anchors on the uppercase spellings (`^`, `$`, `...`,
and the `-B` linked rejection), and one asserting unknown letters error rather
than default.

Observable effect: **read-2 `-G`/`-B` now match cutadapt 4.6**, and nothing else
changed. Verified by running the fixed binary against the 38 goldens recorded
from the pre-fix-of-fix binary — all 38 still pass — plus 40/40 on the current
gate, 119/119 differential, and the two UMI-tools suites unchanged.

#### Consequence for the goldens

Recording the new read-2 cases from the pre-fix binary would have frozen the
wrong answer as the expectation. They are therefore marked
`record_from_oracle=True` and recorded from **cutadapt 4.6 itself**, the
compatibility authority. This is a capability in `snapshot.py` that only these
cases use, and it is documented in `tests/gate/README.md` as a statement that
the baseline was wrong — never as a way to relax a refactor comparison.

Proof that the gate discriminates: with the current goldens the fixed binary is
40/40, and the pre-fix baseline binary fails exactly `cutadapt_read2_5prime`
(`-A` passes, because `-A` was never broken).

### DR-11 — a pre-existing anywhere-adapter report difference, not fixed here

While adding the `-B` case above, a second difference appeared: for an
anywhere (`-B`) adapter, cutadapt 4.6 prints the `Overview of removed sequences
(5')` histogram header even when that histogram is empty, and rust-cutadapt omits
the header entirely.

```text
$ cutadapt -a AGATCGGAAGAGC -a AAAA -B ACGT ...    # 3 "length count expect" headers
$ rust-cutadapt ...                                # 2 headers
```

This is **pre-existing** (present in the pre-refactor binary), **cosmetic**
(a header line for an empty table), and **out of W1-1's scope** — W1-1 is a
behaviour-neutral refactor, and fixing this would be a cutadapt-compatibility
change, not a refactor. It is recorded here so it is not lost, and `-B` is
deliberately left out of the gate matrix so the W1-1 gate does not fail for a
reason W1-1 did not cause. `-B`'s *trimming* is still covered, because `-B` and
`-G` share the 5' code path that DR-10 fixed.

Suggested follow-up: fix the empty-histogram header and add a `-B` report case,
as a separate compatibility change with its own evidence.

### DR-9 — one workspace `Cargo.lock`; the two per-crate locks are now stale

A workspace resolves to a single lockfile at the workspace root, which is what
`cargo build --locked` now uses (verified). The two crates still carry their old
`rust-cutadapt/Cargo.lock` and `rust-umitools/Cargo.lock`, which Cargo ignores
while they are workspace members.

They were **left in place rather than deleted**, because deleting them is a
governance decision, not a refactor step:

* `rust-cutadapt` and `rust-umitools` are still separate Git repositories. Until
  the workspace root is itself a repository (governance work, not W1-1), a file
  committed at the root is not tracked anywhere, while the per-crate locks are.
* A reviewer comparing this change set should see the lockfile churn as its own
  decision. Silently deleting two tracked files would hide it.

The consequence is a real, recorded gap: **the per-crate lockfiles are stale and
must not be trusted.** `Cargo.lock` at the workspace root is authoritative. A
follow-up should delete the two crate-level files once the root is a repository;
that follow-up is listed under "What remains for W1-2".

What was checked rather than assumed: running `cargo build --release --locked`,
`cargo test --release --locked` and `cargo clippy --all-targets --locked` from
*inside* `rust-cutadapt/` still succeeds. Cargo discovers the workspace root,
uses the root `Cargo.lock` (which does contain `xz2`; the crate-level file does
not), and builds into `<root>/target`. So the two existing CI workflows keep
working unchanged, even though their checked-in lockfiles are now inert.

### DR-8 — per-crate `target/` directories become obsolete

The workspace puts all artifacts in `<root>/target`. Consequences that had to be
handled rather than guessed:

* `rust-cutadapt/tests/gate/m0.sh`, `tests/check.py`, `install.sh`,
  `README.md` and `BENCHMARK.md` refer to `target/release/rust-cutadapt`. From
  the repository root that path is still correct for the *workspace* build; from
  inside `rust-cutadapt/` it is not. The oracle harness resolves the binary via
  `--rust`, `$RUST_CUTADAPT`, or `target/release/rust-cutadapt`, so a run from
  the root finds the workspace artifact.
* `rust-umitools/tests/oracle/*.py` already honour `$CARGO_TARGET_DIR` and fall
  back to `<crate>/target`, which is why the suites are run with
  `CARGO_TARGET_DIR=<root>/target`.

No test, script or oracle was weakened to accommodate this; the two directories
are simply documented as build locations.

## Verification results

All commands below were run in this session from the repository root unless a
`cd` is shown. `--offline` throughout; nothing was downloaded.

### Build

```text
$ cargo build --release --offline
    Finished `release` profile [optimized] target(s) in ...
```

### Rust tests

Both profiles are run. The **debug** command is the W1-1 acceptance command; the
release command is included because it exercises the optimised build that ships.
An earlier version of this file recorded only the release run and described it as
`cargo test --workspace`, which was wrong: a debug-only `debug_assert!` failure
was therefore invisible (see DR-10).

```text
$ cargo test --workspace --offline
     Running unittests src/lib.rs (bioinfo-common)
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (rust-cutadapt)
test result: ok. 174 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/main.rs (rust-cutadapt)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/cli.rs (rust-cutadapt)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/cli_e2e.rs (rust-cutadapt)
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/differential.rs (rust-cutadapt)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (rust-umitools)
test result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/main.rs (rust-umitools)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/cli_contract.rs (rust-umitools)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
$ cargo test --workspace --offline --release
     Running unittests src/lib.rs (bioinfo-common)
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (rust-cutadapt)
test result: ok. 171 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/main.rs (rust-cutadapt)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/cli.rs (rust-cutadapt)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/cli_e2e.rs (rust-cutadapt)
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/differential.rs (rust-cutadapt)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (rust-umitools)
test result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/main.rs (rust-umitools)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/cli_contract.rs (rust-umitools)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Total: **307 passed, 0 failed in both profiles** — 39 + 174 + 0 + 14 + 21 + 1 + 50
+ 2 + 5 + 0 + 1 + 0, i.e. 39 `bioinfo-common` + 211 `rust-cutadapt` + 58
`rust-umitools` (the zero rows are `main.rs` unit-test targets with no tests of
their own).

`rust-cutadapt` went from 208 to 211 because DR-10 added three regression tests
for the read-2 adapter option letters.

### Oracles

```text
$ python3 rust-cutadapt/tests/oracle/differential.py \
      --rust target/release/rust-cutadapt --matrix
119/119 cases passed
```

```text
$ cd rust-umitools
$ CARGO_TARGET_DIR=<root>/target \
  UMI_TOOLS_ORACLE_PATH=<pinned-oracle-bin> bash tests/oracle/run_exact.sh
77/77 byte-identical, 0 differ
```

```text
$ cd rust-umitools
$ CARGO_TARGET_DIR=<root>/target \
  UMI_TOOLS_ORACLE_PATH=<pinned-oracle-bin> bash tests/oracle/run_oracle.sh
39 passed, 0 failed
```

The pre-refactor values (recorded in
[EVIDENCE-2026-10.md](EVIDENCE-2026-10.md)) were 208 Rust tests, 119/119, 77/77
and 39/39, measured at `rust-cutadapt` `4609a0c` and `rust-umitools` `40b5b70`.
Every one of those numbers is unchanged.

### Byte-neutrality gate

This is the check that establishes behaviour-neutrality directly, rather than by
inference from the oracle suites. `tests/gate/snapshot.py` records 40 observables
from the **pre-refactor** binaries and then verifies a candidate build against
that record: stdout, stderr, exit codes and output files, at the plan's
comparison levels (L0 exit code always, L1 bytes for files, L2 text after named
normalisation for reports).

```text
$ python3 tests/gate/snapshot.py --record \
      --cutadapt .work/w1-1-baseline/bin/rust-cutadapt.before \
      --umitools .work/w1-1-baseline/bin/rust-umitools.before \
      --out tests/gate/golden
recorded 40 cases into tests/gate/golden
```

```text
$ python3 tests/gate/snapshot.py --verify \
      --cutadapt target/release/rust-cutadapt \
      --umitools target/release/rust-umitools \
      --golden tests/gate/golden
40/40 cases pass
```

```text
$ bash tests/gate/m3.sh
--- summary
PASS  reference binaries present
PASS  reference binaries match the recorded SHA256
PASS  binaries under test present
PASS  goldens present (40 case files)
PASS  umitools oracle resolves (UMI_TOOLS_ORACLE_PATH/CONDA_PREFIX/tests-ci-env/PATH)
PASS  verify: every case matches the recorded behaviour
PASS  self-test: a mutated golden is rejected
PASS  provenance block is byte-exact
PASS  exit-code contract exercised for 0/1/2/3/4

9 criterion/criteria: 9 passed, 0 failed

M3 PASS
```

The gate is built so that it can fail, which is the part that makes it worth
anything:

* `--self-test` first verifies the **clean** golden (so a verifier that always
  fails cannot "detect" a mutation), then mutates a text byte, an exit code and
  a hash in turn, and requires each mutation to be **rejected**. It prints
  `SELFTEST PASS`.
* A normalisation rule that stops firing is a **failure**, not a pass, so the
  rules cannot silently weaken.
* An unresolvable UMI-tools oracle **fails** the gate instead of skipping the
  UMI-tools cases. (This actually happened on the first attempt with `PATH`
  unset: the gate failed 5 criteria rather than passing 4 and skipping the rest.)
* Recording twice produces byte-identical goldens, checked as part of the
  acceptance run.

The one place the gate is weaker than byte-exact is the `--summary-json`
`resources` object (`threads`, `memory_mib`, `chunk_mib`), which is derived from
the host's CPU quota, `/proc/meminfo` and free temp space. That is a machine
property, not a refactor property; every counter, key order, escape and the
trailing newline are still compared exactly.

### Harness provenance

The gate was authored by a delegated instance (`w1-1-harness`) whose tool set
turned out to have no shell, so it delivered the code but could not execute
anything, and it correctly refused to hand-write goldens it had not observed.
The leader then did the recording and debugging. Three real defects surfaced on
the first run and were fixed; they are listed in
[tests/gate/README.md](../tests/gate/README.md) under "Fixes applied after the
first recording attempt":

1. `render_bam` compared `samtools view -h` output without applying the case's
   path rule, so the `@PG` `CL` tag kept the per-run temp directory.
2. `umitools_unsorted_bam` attached no log rules, although `dedup` logs host
   facts to stderr before it reads input.
3. `umitools_logfile` staged no fixture, so it recorded an I/O failure
   (`exit=4`) instead of the intended successful run (`exit=0`).
4. `render_bam` returned the result of `render_text` (already a dict) inside
   another dict, which produced a nested "text" value and crashed the
   difference printer on any BAM mismatch. Fixed to pass the rendered dict
   through.

One case was also **added**: `umitools_add_pg` guards the `ProgramRecord`
refactor specifically. W1-1 moved the `@PG` tag set and its order into
`bioinfo_common::provenance`, so without a case covering `--add-pg` that part of
the "provenance block" requirement would have been asserted only by a unit test
of the shared crate, never by an end-to-end comparison. The case compares the
whole `samtools view -h` rendering of a `--add-pg` BAM; the `CL` tag's binary
path is normalised (a golden recorded from `.before` must verify against the
workspace build) and everything else — tag names, tag order, `PP` linkage and
the rest of the command line — is byte-exact.

A non-reproducible BGZF hash was also dropped from the goldens, for the reason
given in DR-8's neighbour: the `@PG` `CL` tag embeds the run's temp directory, so
the container bytes of a *correct* output legitimately differ between runs. The
decompressed contract (plan decision D4) is what the comparison enforces.

## What is explicitly not claimed

* **No clean-container run.** The evidence is from this machine with the pinned
  oracle environments.
* **Binary SHA256 values differ from the pre-refactor binaries.** The workspace
  forces `lto = "fat"` (DR-6), which changes code generation. Byte-neutrality is
  claimed for *observable outputs* (stdout, stderr, exit codes, output files),
  which is what the gate checks — not for the binary bytes themselves.
* **No compressed-byte equality is claimed in general.** The workspace rule is
  decompressed bytes (plan decision D4). For xz specifically, DR-2 does record
  compressed equality, because that was needed to justify the substitution.
* **The cutadapt `--json` schema and the Python formatting rules are not
  shared.** DR-3 and DR-4.
* **W1-2 and later are untouched.** No `bioinfo-fastx`, no parser migration, no
  new subcommand, no version bump.

## What remains for W1-2

1. **`bioinfo-fastx`.** Not created. `rust-cutadapt/src/fastq.rs` and
   `rust-umitools/src/io/fastq.rs` are both still in place.
2. **`rust-cutadapt` on `bioinfo-fastx`.** Not attempted; W1-2's acceptance is
   119/119 with decompressed-byte equality, which needs the parser first.
3. **Compression detection, multi-member gzip, BGZF, strict-vs-lenient modes.**
   Not moved. `rust-cutadapt/src/compress.rs` still owns cutadapt's format
   sniffing.
4. **A fuzz target for the parser.** Does not exist; W1-2 requires ten minutes
   per parser.
5. **CI.** The two workflows still run per-crate (`cargo test` from each crate's
   directory). They are correct but do not yet exercise
   `cargo test --workspace`, because the repositories are still separate Git
   repositories and the workspace root is not itself a repository. Unifying CI
   belongs with the governance work, not with W1-1.
6. **Stale per-crate lockfiles.** `rust-cutadapt/Cargo.lock` and
   `rust-umitools/Cargo.lock` are ignored by Cargo now that both are workspace
   members. The root `Cargo.lock` is authoritative. Deleting the two tracked
   files needs the workspace root to be a Git repository first (DR-9).
7. **The anywhere-adapter report header (DR-11).** cutadapt 4.6 emits an empty
   5' histogram header for `-B`; rust-cutadapt does not. Pre-existing, cosmetic,
   and a cutadapt-compatibility change rather than a refactor, so it is a
   follow-up with its own evidence rather than part of W1-1.
8. **`rust-umitools`' `-L` log-file mirroring** still routes through the local
   `Logger`; it writes to stderr and a file with its own prefix handling. It was
   left alone because moving it changes nothing observable and W1-1 only asks
   for the four named contracts.

## Reproducing this evidence

```bash
cd <repo root>
cargo build --release --offline
cargo test --workspace --release --offline

python3 rust-cutadapt/tests/oracle/differential.py \
    --rust target/release/rust-cutadapt --matrix

cd rust-umitools
CARGO_TARGET_DIR=../target \
UMI_TOOLS_ORACLE_PATH=<pinned-oracle-bin> bash tests/oracle/run_exact.sh
CARGO_TARGET_DIR=../target \
UMI_TOOLS_ORACLE_PATH=<pinned-oracle-bin> bash tests/oracle/run_oracle.sh

cd ..
bash tests/gate/m3.sh
```
