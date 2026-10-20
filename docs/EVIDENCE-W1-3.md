# W1-3 evidence: `rust-fx`

Status: **VERIFIED** — the plan's oracle differential was run against the exact
pinned oracle versions and **all 50 cases pass**.

Canonical machine-readable file: [EVIDENCE-W1-3.json](EVIDENCE-W1-3.json).

Scope: `docs/DEVELOPMENT-PLAN.md` **W1-3**, "`rust-fx` with `stats`, `seq`,
`fq2fa`, `fx2tab`, `sample --seed`, `split`, `lint`."

Its acceptance check is:

> Differential vs SeqKit 2.14.0 and fq 0.12.0 on the agreed subcommand matrix;
> decompressed bytes identical; `sample` reproducible for a fixed seed

**W1-1 and W1-2 remain complete and uncommitted**; their changes are preserved
untouched. W1-4 was not started (it is separately blocked on noodles; see
[EVIDENCE-W1-4.md](EVIDENCE-W1-4.md)). W1-5 and later were not started.

## 1. The one thing to read

| Acceptance criterion | Status |
| --- | --- |
| `rust-fx` implements the seven subcommands | **met** |
| In-repo tests for every subcommand | **met** — 178 tests |
| `sample --seed` reproducible for a fixed seed | **met**, verified in-repo *and* by property case |
| `split` naming/ordering deterministic | **met**, verified in-repo *and* against SeqKit |
| `lint` diagnostics per fq's published validator table | **met**, verified against `fq lint` |
| **Differential vs SeqKit 2.14.0, decompressed bytes identical** | **MET — 50/50 cases** |
| **Differential vs fq 0.12.0** | **MET — 50/50 cases** |

`tests/oracle/run_w13_oracle.sh` exits 0. It still **fails closed with exit 3** if
either oracle is missing, so this cannot silently regress into a false pass.

## 2. Result

```text
$ bash tests/oracle/run_w13_oracle.sh
rust-fx : target/release/rust-fx
seqkit  : /home/rimuru/Projects/Code/for_fun/bioinfo-tools/tests/ci/env/bin/seqkit
fq      : /home/rimuru/Projects/Code/for_fun/bioinfo-tools/tests/ci/env/bin/fq
seqkit v2.14.0
fq v0.12.0 (51779f50e 2024-07-08)
cases  : 50

...
50/50 cases passed
```

| Oracle | Version | SHA-256 of the binary used |
| --- | --- | --- |
| SeqKit | **2.14.0** | `88aa8a539b1e9097220a47811367b7ef826eff8637e5284fdd32a372770d6f7d` |
| fq | **0.12.0** (`51779f50e`) | `600e8b204086bc0d3e225b0021fd234fc452bf72414af80ad8a115a327b5fcb2` |

`rust-fx` binary under test: `c1552e841717f5d0f674d6dc533996feca84949aa654bc5b63a0a0662c25d466`

Pins, checksums, install instructions and the resolution order are recorded in
[`tests/ci/PINS.md`](../tests/ci/PINS.md). **The oracle binaries are not
committed**; `.gitignore` excludes `tests/ci/env/bin/`.

Cases by category: `stats` 8, `seq` 12, `fq2fa` 3, `fx2tab` 9, `sample` 4,
`split` 3, `lint` 11.

## 3. The harness defect that had to be fixed first

The harness crashed **before running a single case**:

```text
FileNotFoundError: [Errno 2] No such file or directory:
  '/tmp/w13-diff-.../stats_default_fastq-oracle/in.fq'
```

`run_case` wrote each fixture into a per-case work directory that no caller ever
created. Three genuine harness defects were found and fixed:

| # | Defect | Fix |
| --- | --- | --- |
| 1 | `run_case` wrote fixtures into a directory it never created, so the whole run died on case 1 | `run_case` now owns `work.mkdir(parents=True, exist_ok=True)` |
| 2 | The `--rust` path was relative (`target/release/rust-fx`) but `run_case` runs tools with `cwd=<work>`, so subprocess raised `FileNotFoundError` | `_absolute_binary()` resolves every tool path up front |
| 3 | Eight cases had a guard string split across two literals with a comma instead of concatenation, pushing `"seqkit"` into the `oracle` field. The matrix still built and `--list` still printed, but the run died with `KeyError: 'reverse then complement'` | guards joined; a structural self-test now validates every case's `oracle`/`argv` |

Two further **comparison** bugs — not crashes — were also wrong and are recorded
in §5 because they changed verdicts.

The self-test previously only exercised pure functions, so it could not see any of
these. It now additionally:

* asserts every case has a valid `oracle`, non-empty name/category/guard, and a
  string-only `argv`;
* calls `run_case` against `/bin/cat` in a never-created nested directory and
  asserts the directory and its fixtures exist.

Negative-tested: deleting the `mkdir` makes the self-test fail with
`run_case could not create its work dir`.

## 4. Real implementation bugs found by the oracle

These were **`rust-fx` defects**, fixed in the implementation because the oracle is
authoritative. Each quotes the measured difference.

### 4.1 `stats` human layout invented a table rule and used 3-space separators

```text
oracle: file   format  type  num_seqs  sum_len  min_len  avg_len  max_len
        in.fq  FASTQ   DNA          4       62        4     15.5       34
rust  : file    format   type   num_seqs   sum_len   min_len   avg_len   max_len
        -----   ------   ----   --------   -------   -------   -------   -------
        in.fq   FASTQ    DNA           4        62         4       15.5        34
```

SeqKit prints **no dashed rule line** and separates columns with **two** spaces.
The apparent three-space gap after `file` is a two-space separator plus the one
padding space of a left-aligned column. Fixed in `render`/`pad_row`. Fixed 4 cases.

### 4.2 `stats -a` GC used the wrong denominator

`stats` GC(%) is `(G+C)/length`; the implementation used `(G+C)/(G+C+A+T)`.

```text
in.fq (contains 4 Ns)   oracle GC=48.39   rust GC=51.72
GCGCAAAANN              oracle GC=40.00   (4/10)
                        fx2tab -g 50.00   (4/8)
```

So `stats` and `fx2tab -g` use **different** denominators — a real asymmetry, not
a bug in either. Both are now implemented and pinned:
`SeqStats::gc_percent()` (length) and `SeqStats::gc_percent_unambiguous()`
(GC+AT). Edge cases measured and matched:

| input | `stats` | `fx2tab -g` |
| --- | --- | --- |
| `GCGCAAAANN` | `40.00` | `50.00` |
| `NNNN` | `0.00` | `NaN` |
| `----` | `0.00` | `NaN` |
| empty | `NaN` | `NaN` |

### 4.3 Percentages were trimmed; SeqKit never trims

```text
fx2tab -l -g    oracle: ...\t12\t50.00      rust: ...\t12\t50
fx2tab -B AT    oracle: ...\t35.00          rust: ...\t35
```

The plan's quoted examples showed `47.6` and `0`, which is where the trimming came
from; the oracle disagrees and the oracle wins. `percent()` now always prints two
decimals.

### 4.4 `Q20(%)`/`Q30(%)`/`AvgQual` have **different** precision per layout

Measured across both layouts of the same input:

| fraction | human `Q20(%)` | tabular `Q20(%)` | human `AvgQual` | tabular `AvgQual` |
| --- | --- | --- | --- | --- |
| 58/62 | `93.55` | `94` | `11.9` | `11.90` |
| 2/3 | `66.67` | `67` | `4.77` | `4.77` |
| 5/8 | `62.5` | `62` | `4.26` | `4.26` |
| 1/2 | `50` | `50` | `3.01` | `3.01` |

The human layout keeps up to two decimals and trims; the tabular layout rounds
`Q20`/`Q30` to whole numbers and pads `AvgQual` to two decimals. Implemented as
layout-aware renderers in `table::cells`.

### 4.5 Quartiles were nearest-rank; SeqKit uses Tukey hinges

Undocumented upstream, so reverse-engineered from 11 synthetic inputs plus both
real fixtures:

```text
n=4  lens 1..4   oracle Q1=2 Q2=2 Q3=4
n=8  lens 1..8   oracle Q1=2 Q2=4 Q3=6      (type-7 linear gives 3)
n=20 lens 1..20  oracle Q1=6 Q2=10 Q3=16
```

The matching method is **Tukey's hinges** (median shared with both halves when the
count is odd) with each half-median **rounded half to even**. Nearest-rank and
type-7 both disagree on some `n`. A second, separate bug surfaced during the fix:
the upper hinge was sliced as `lengths[n/2+1..]` instead of `lengths[n/2..]`, which
gave `Q3=20` instead of `14` for lengths `[4, 7, 20]`.

### 4.6 `fx2tab -s` hashed the upper-cased sequence

```text
seqkit fx2tab -s  for ACGTACGT  -> db819856e873893888e8ac8f72cae1f8 = md5("acgtacgt")
rust                            -> cc0af3a4fedb18378b4b57b98068e69f = md5("ACGTACGT")
```

SeqKit hashes the **lower-cased** sequence; upper, lower and mixed-case inputs all
hash their lower-cased form.

### 4.7 `fx2tab -H` named the first column `name`; `-C`/`-B` order and labels were wrong

```text
-H -l -g      oracle: #name  seq  qual  length  GC
-B AT -C GC   oracle: #name  seq  qual  GC  AT      (counts BEFORE contents)
```

Every `-C` count column precedes every `-B` content column regardless of flag
order, and each is labelled with the bare spec (`GC`, `AT`) — no `(%)`/`(count)`.

### 4.8 `split` wrote its `[INFO]` diagnostics to stdout

SeqKit writes them to **stderr**, leaving stdout empty:

```text
$ seqkit split -s 1 in.fq > out 2> err
out is empty; err has "[INFO] write 1 sequences to file: in.fq.split/in.part_001.fq"
```

Fixed: both split `[INFO]` sites use `eprintln!`. Part naming
(`in.fq.split/in.part_001.fq`) and ordering already matched.

### 4.9 `lint` exited 3 and logged to stderr; fq exits 1 and logs to stdout

```text
$ fq lint bad.fq ; echo $?
1
```

`lint` returned `ExitClass::Input` (3). Every lint case failed on the exit code
alone. Now returns `ExitClass::Runtime` (1) via a new `CommandError::runtime`.

The log stream and shape also had to match. fq brackets a run with `INFO` lines on
**stdout**, using two targets:

```text
<ts>  INFO fq::commands::lint: fq-lint start
<ts>  INFO fq::commands::lint: validating single end read
<ts>  INFO fq::validators: disabled validators: []
<ts>  INFO fq::validators: enabled single read validators: ["[S003] NameValidator", ...]
<ts>  INFO fq::validators: enabled paired read validators: []
<ts>  INFO fq::commands::lint: starting validation
<ts>  INFO fq::commands::lint: read 1 records
<ts>  INFO fq::commands::lint: fq-lint end
```

Paired runs differ: `starting validation (pass 1)`, `read N * M records`,
`starting validation (pass 2)`, `read N records`, plus an
`enabled special validators: ["[S007] DuplicateNameValidator"]` line.
`fq-lint end` is printed **even when validation fails**.

### 4.10 S007 is a registered code that fq never enables

```text
$ fq lint --disable-validator S007 ok.fq ; echo $?     # accepted -> code is known
0
$ fq lint --lint-mode log dup.fq ; echo $?              # two identical @d records
0                                                       # no S007 error
enabled single read validators: [... S003, S004, S002, S001, S005, S006]   # no S007
```

`S007` is known but is a *special* validator, never part of the single-read set at
any level. Listing it in `Validator::SINGLE` made duplicate names exit 3 where fq
exits 0. It is removed from `SINGLE` but still round-trips through
`Validator::from_code`, so `--disable-validator S007` remains accepted.
`S008`/`S009` are likewise known-but-never-enabled.

### 4.11 P001 messages lacked the `@` and were attributed to the wrong file

```text
oracle: r1.fq:5:1: [P001] NamesValidator: names mismatch: expected '@other', got '@p2'
rust  : r2.fq:5:1: [P001] NamesValidator: expected "other", got "p2"
```

fq reports a pair mismatch **against read 1's file** and quotes the raw headers
(with `@`). The driver now passes raw headers and honours a
`report_against_read1` flag.

Error messages were also matched verbatim per code:

| code | fq wording |
| --- | --- |
| S001 | `missing + prefix` |
| S002 | `invalid sequence character: 'Z'` |
| S003 | `missing @ prefix` |
| S004 | `empty quality` / `expected 4, got 0` |
| S005 | `sequence-quality scores lengths mismatch: expected 8, got 3` |
| S006 | `invalid quality score character: '\xff'` |
| P001 | `names mismatch: expected '@other', got '@p2'` |

## 5. Harness comparison bugs (fixed in the harness, not the implementation)

Two cases were **wrongly modelled** and would have failed indefinitely against a
correct binary. Both fixes are justified, and neither weakens a real comparison.

### 5.1 `sample_proportion_bytes` byte-compared an uncomparable command

The case's own guard read *"listed for completeness: NOT compared byte-wise"* but
it was declared with `oracle="seqkit"`, so the harness byte-compared it and failed
every run. `sample`'s selection algorithm is unspecified (SeqKit uses Go's
`math/rand`); no independent implementation can reproduce it from a seed — this is
DR-4, recorded before the oracle existed. Re-declared as a `property` case: output
must be a subset of the input and exit 0. **This is not a weakened comparison** —
byte comparison was never possible, and the case says so itself.

### 5.2 `sample_seed_changes_selection` ran the same seed twice

The property is "a *different* seed changes the selection", but the harness ran the
case's single `argv` twice — seed 12 against seed 12 — and then asserted the outputs
differ. It could only ever fail. Added `_swap_seed()` so the second run actually
uses a different seed.

### 5.3 One legitimate normalisation rule added

`fq_log_prefix` strips ANSI SGR sequences and canonicalises
`<timestamp> <LEVEL>` to `<TS> <LEVEL>`.

This is required, not convenient: fq's log lines carry a **wall-clock timestamp**
that differs on every run (verified: two consecutive runs produce different first
lines), and ANSI colour that depends on the ambient environment. Byte-comparing
them is impossible for any implementation. Everything after the prefix — level,
target, message — is still compared byte for byte.

### 5.4 The trap avoided

When fixing §4.3 I briefly over-corrected `tabular_float` and `percent` while
reverting a deliberate negative-control mutation. The suite dropped to **41/50**,
which is how the mistake was caught. That accident is itself evidence the
comparison has teeth. Separately, a deliberate mutation of `percent()` (`.2` → `.1`)
drops the suite from 50/50 to **43/50**. A harness that cannot fail proves nothing.

## 6. Verification results

All commands run by me, offline, in this session.

| Check | Command | Result |
| --- | --- | --- |
| Differential | `bash tests/oracle/run_w13_oracle.sh` | **50/50 cases passed**, exit 0 |
| Harness self-test | `python3 tests/oracle/w13_diff.py --self-test` | **SELFTEST PASS** |
| Formatting | `cargo fmt --all -- --check` | clean |
| Lints | `cargo clippy --workspace --all-targets --offline -- -D warnings` | **0 warnings** |
| Workspace tests (debug) | `cargo test --workspace --offline` | **587 passed, 0 failed** |
| Workspace tests (release) | `cargo test --workspace --release --offline` | **587 passed, 0 failed** |
| cutadapt 4.6 | `python3 tests/check.py` (in `rust-cutadapt`) | all visible conformance cases passed |
| UMI-tools exact | `bash tests/oracle/run_exact.sh` | **77/77 byte-identical, 0 differ** |
| UMI-tools semantic | `bash tests/oracle/run_determinism.sh` | **deterministic: 0 mismatches** |
| UMI-tools formats | `python3 tests/oracle/formats.py` | **formats ok: 0 failures** |
| M3 byte-neutrality gate | `bash tests/gate/m3.sh` | **9/9 criteria, M3 PASS** |

### Test counts

| Crate | Tests |
| --- | --- |
| `bioinfo-common` | 39 |
| `bioinfo-fastx` | 96 |
| `rust-cutadapt` | 201 |
| `rust-fx` | **178** |
| `rust-umitools` | 73 |
| **Total** | **587** |

`rust-fx` went 177 → 178: one lint test renamed and repointed (`s007_rejects…` →
`s007_does_not_run_by_default`), plus the harness self-test additions which live in
Python, not Rust. The per-crate counts sum exactly to the 587 the workspace reports.

### Oracle environment note

The UMI-tools oracle needs `python3.12` with `numpy 2.5.3` and `pysam 0.24.1`. On
the default `PATH` it reports `python: expected 3.12.14, found 3.14.8` and refuses
to run. With the pinned environment prepended it passes as above. Environment
selection, not a regression.

## 7. Status change: partial → verified

Earlier revisions of this file said **PARTIAL** because both oracles were absent
and the differential had never run. That is no longer true: both oracles are
installed at the pinned versions, the harness defects are fixed, and all 50 cases
pass. Status is therefore **VERIFIED**, and the change is justified by a run, not
by an intention.

What *specifically* would invalidate it:

* a different SeqKit or fq version (§ `tests/ci/PINS.md` lists the behavioural
  dependencies on version);
* removing `tests/ci/env/bin/` — the harness then exits 3 and claims nothing.

## 8. What is still explicitly not claimed

* **Cross-platform byte-identity.** Only linux x86_64 was measured.
* **Subcommands outside the 50-case matrix.** The matrix is the agreed one; it is
  not exhaustive over every SeqKit/fq flag.
* **`sample` selection equality with SeqKit.** Impossible by construction (DR-4);
  reproducibility and subset-hood are what is claimed.
* **W1-4 or later.** Not started. W1-4 is separately blocked on noodles; see
  [EVIDENCE-W1-4.md](EVIDENCE-W1-4.md).

## 9. Reproducing this evidence

```sh
# 1. Pin the oracles (see tests/ci/PINS.md for downloads and checksums).
cd tests/ci/env/bin && sha256sum -c <<'EOF'
88aa8a539b1e9097220a47811367b7ef826eff8637e5284fdd32a372770d6f7d  seqkit
600e8b204086bc0d3e225b0021fd234fc452bf72414af80ad8a115a327b5fcb2  fq
EOF

# 2. Build and run the differential.
cd - && cargo build --release --offline -p rust-fx
python3 tests/oracle/w13_diff.py --self-test
bash tests/oracle/run_w13_oracle.sh

# 3. The workspace gates.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo test --workspace --release --offline
bash tests/gate/m3.sh
```

## 10. Files changed for W1-3

Implementation (behaviour fixes, oracle-verified):

* `rust-fx/src/table.rs` — human layout (no rule line, 2-space separators),
  `percent` two decimals, Tukey-hinge quartiles with half-even rounding,
  layout-aware `Q20`/`Q30`/`AvgQual`, `Cell` type alias.
* `rust-fx/src/stats.rs` — `gc_percent` (length denominator) and
  `gc_percent_unambiguous` (GC+AT), with the asymmetry documented.
* `rust-fx/src/commands.rs` — `fx2tab` `#name`/ordering/lower-case MD5,
  `stats` GC denominator, `split` INFO → stderr, `lint` stdout logging + exit 1,
  fq-format logger (`fq_log_message`, `fq_log_from`, `log_timestamp`,
  `civil_from_days`, `render_validators`), paired/single log flows.
* `rust-fx/src/lint.rs` — `LintError` gains `line`/`column`/`oracle_message`/
  `report_against_read1`; `render_at`; `S007` removed from `SINGLE` but retained
  in `from_code`.
* `rust-fx/src/io.rs` — `CommandError::runtime` (exit 1).

Harness:

* `tests/oracle/w13_diff.py` — 3 crash fixes, 2 comparison fixes, structural +
  `run_case` self-tests, `fq_log_prefix` rule, `_swap_seed`.
* `tests/oracle/run_w13_oracle.sh` — unchanged; still resolves oracles and
  fail-closes with exit 3.
* `tests/ci/PINS.md` — new: versions, SHA-256 pins, install, resolution order.
* `.gitignore` — new: excludes `tests/ci/env/bin/` so binaries are never committed.

No release, tag, version bump or publication. Versions unchanged:
`bioinfo-common` 0.1.0, `bioinfo-fastx` 0.1.0, `rust-cutadapt` 0.1.0,
`rust-fx` 0.1.0, `rust-umitools` 0.2.0.
