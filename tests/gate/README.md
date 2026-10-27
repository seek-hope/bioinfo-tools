# W1-1 byte-neutrality gate

> **Existing oracle: 40 committed golden snapshots plus `golden.sha256`.**
> `bash tests/gate/acceptance.sh --verify` delegates to the read-only `m3.sh`
> gate. No arguments is a usage error (exit 2); `--help` prints usage (exit 0).
> `--record`, unknown arguments and `GOLDEN_DIR`/`GOLDEN_MANIFEST` overrides fail
> with exit 2 before any work. `m3.sh` verifies the golden digest manifest in
> addition to counting the files. The old automatic delete/record path has been
> removed. Golden updates require separate maintainer review and
> `snapshot.py --record --confirm-record`.
> Historical W1-1 evidence is in `docs/EVIDENCE-W1-1.md`.

This directory holds the instrument that proves **W1-1 is behaviour-neutral**.

W1-1 creates the Cargo workspace and the shared `bioinfo-common` crate, and
moves four things out of the two binaries into it:

| Moved | Where it landed |
| --- | --- |
| exit-code classes | `bioinfo-common/src/exit.rs` |
| diagnostic formatting | `bioinfo-common/src/diagnostic.rs` |
| the `--summary-json` contract | `bioinfo-common/src/summary.rs` |
| the provenance block | `bioinfo-common/src/provenance.rs` |

A refactor of that kind is only acceptable if **no stdout byte, stderr byte,
file byte or exit code changes**. `snapshot.py` records those observables from
the PRE-refactor binaries (`.work/w1-1-baseline/bin/*.before`) and compares any
other pair of binaries against that recording.

The plan's convention (`docs/DEVELOPMENT-PLAN.md`, "Milestone gates") is one
script per milestone that prints one PASS/FAIL line per criterion and exits
non-zero on any failure. That script is `m3.sh`.

---

## Quick start

```bash
# 1. Verify a candidate pair against the EXISTING oracle (no recording).
python3 tests/gate/snapshot.py --verify \
    --cutadapt target/release/rust-cutadapt \
    --umitools target/release/rust-umitools \
    --golden tests/gate/golden

# 2. Prove the harness can actually fail (mandatory).
python3 tests/gate/snapshot.py --self-test \
    --cutadapt target/release/rust-cutadapt \
    --umitools target/release/rust-umitools \
    --golden tests/gate/golden

# 3. The whole read-only gate.
bash tests/gate/acceptance.sh --verify

# 4. Entry-point safety regression tests (temporary workspace only).
python3 tests/gate/test_acceptance.py
```

Exit codes: `0` pass, `1` a case failed, `2` usage error, `3` a precondition
(missing binary, missing golden, unresolved oracle) was not met. A missing oracle is **never** a
skip: it is exit 3.

### `m3.sh` options

| Variable | Meaning | Default |
| --- | --- | --- |
| `CUTADAPT_BIN` | binary under test | `target/release/rust-cutadapt` |
| `UMITOOLS_BIN` | binary under test | `target/release/rust-umitools` |
| `BASELINE_CUTADAPT` | reference binary | `.work/w1-1-baseline/bin/rust-cutadapt.before` |
| `BASELINE_UMITOOLS` | reference binary | `.work/w1-1-baseline/bin/rust-umitools.before` |
| `GOLDEN_DIR` | golden directory | `tests/gate/golden` |
| `GOLDEN_MANIFEST` | reviewed SHA-256 list | `tests/gate/golden.sha256` |

`m3.sh` **never** builds. It fails the "binaries present" criterion instead, so
a reviewer cannot accidentally gate a stale artifact.

---

## Why the goldens are recorded from the `.before` binaries

`target/release/*` is rebuilt many times during the refactor. The `.before`
binaries are the only objects whose observable behaviour is known-good: they
were captured at the recorded commits in `docs/EVIDENCE-2026-10.md`
(`rust-cutadapt` `9d9abdf`, `rust-umitools` `40b5b70`).

`.work/w1-1-baseline/bin/*.before` are treated as **read-only**. Nothing here
writes to them, and neither `m3.sh` nor `snapshot.py` ever invokes cargo.

---

## Case matrix

Every case carries a `guard` string naming the refactor risk it protects, so a
failure reads as "the diagnostic layer broke", not just "case 12 failed".

### `rust-cutadapt` (fixtures `rust-cutadapt/tests/fixtures/simple.fastq` and
`.gz`, adapter `AGATCGGAAGAGC`, `-m 10`)

| Case | Level | Guards |
| --- | --- | --- |
| `cutadapt_version` | L2 | CLI version text |
| `cutadapt_help` | L2 | whole `--help` text (the 60-option grammar) |
| `cutadapt_unknown_option` | L0/L2 | exit class **2** + clap usage diagnostic on stderr |
| `cutadapt_missing_input` | L0/L2 | exit class **1** + stdout `ERROR:` line (cutadapt errors go to **stdout** once processing has begun) |
| `cutadapt_malformed_fastq` | L0/L2 | exit **1** + the exact malformed-FASTQ diagnostic |
| `cutadapt_plain_fastq` | L1/L2 | output file bytes + full text report |
| `cutadapt_gzip_fastq` | L1/L2 | **decompressed** output bytes from a `.gz` input |
| `cutadapt_quiet` | L1 | `--quiet`: no report, unchanged bytes |
| `cutadapt_json_report` | L1 | `--json` report bytes — this document has **no timing fields**, so zero normalisation |
| `cutadapt_report_minimal` | L1 | `--report minimal` stdout (no banner, no timing) |
| `cutadapt_debug` | L2 | `--debug` adapter/step diagnostics are stable |
| `cutadapt_empty_input` | L0/L2 | empty input → exit **0** + `No reads processed!` + report |
| `cutadapt_demultiplex` | L1/L2 | per-file bytes for `o_1.fastq` and `o_unknown.fastq` |
| `cutadapt_j1`, `cutadapt_j2` | L1/L2 | identical output bytes; only the core-count line may differ |

### `rust-umitools`

| Case | Level | Guards |
| --- | --- | --- |
| `umitools_version` | L2 | CLI version text |
| `umitools_version_verbose` | **L1** | **the provenance block — byte-exact, zero normalisation** |
| `umitools_help` | L2 | whole `--help` text |
| `umitools_{dedup,group,count,count_tab,extract,whitelist}_help` | L2 | each subcommand's option grammar + `after_help` block |
| `umitools_count_tab` | L1 | TSV output bytes + `--summary-json` contract |
| `umitools_count_tab_per_cell` | L1 | `--per-cell` output bytes |
| `umitools_count_tab_bad_table` | L0/L2 | 3-column table → `BadTable` diagnostic + its measured exit code |
| `umitools_dedup` | L1 | dedup records (via `samtools view -h`) + `--summary-json`, `--random-seed 1` |
| `umitools_group` | L1 | same, for `group` |
| `umitools_count` | L1 | `count --per-gene --gene-tag GX` TSV + `--summary-json` |
| `umitools_unknown_option` | L0/L2 | exit class **2** + clap diagnostic |
| `umitools_bad_progress` | L0/L2 | exit class **2** + `unknown --progress value 'sometimes'` **and its 7-space-indented hint** |
| `umitools_missing_input` | L0/L2 | exit class **4** + IO diagnostic |
| `umitools_unsorted_bam` | L0/L2 | exit class **3** + `NotSorted` diagnostic |
| `umitools_io_failure` | L0/L2 | exit class **4** when the output parent is not a directory |
| `umitools_logfile` | L1 | `-L` log bytes **byte-exact** + log/stderr mirroring |
| `umitools_logfile_dedup` | L2 | dedup `-L` log: timing/budget/host lines normalised, everything else exact |

The BAM fixtures are built in a temp directory with `samtools sort` +
`samtools index`, shaped like `rust-umitools/tests/oracle/exact.py`: a UMI
family with an exact tie, a one-edit chain, both strands at one position, a
9 nt family, and a trailing unmapped record.

The `mirror:` streams are a structural invariant, not a recorded text: the tool
records the literal verdict `equal` / `DIFFERENT` for "the `-L` log and stderr
carry the same diagnostics". That way a change that breaks mirroring fails even
though both rendered texts individually still match.

---

## Comparison levels used

Per `docs/DEVELOPMENT-PLAN.md`:

* **L0** — exit code equal. Applied to *every* case, always.
* **L1** — bytes equal. Output files (FASTQ, TSV, JSON, BAM records) and every
  stream that is meant to be byte-stable.
* **L2** — text equal after a named normalisation. Reports containing timings,
  core counts or absolute paths.
* **L3** — unused. Nothing here needs a numeric tolerance: the only floats are
  wall-clock timings, which are normalised or excluded rather than compared.
* **L4** — unused. Every observable here is contractual at L0–L2.

BAM output is compared as **header + records rendered by `samtools view -h`**,
not as raw BGZF bytes: that is the plan's documented BAM rule, and a BGZF
compression change is explicitly not a behaviour change. The rendered text is
then normalised with the same rule set as any other text stream, because a
header written by `samtools sort` carries a `@PG` line whose `CL` tag embeds this
run's temp directory.

The raw BGZF hash is **not** recorded in the golden. It cannot be: the temp
directory appears inside that `@PG` `CL` tag, so two recordings of a correct
output legitimately differ, which would make the golden non-reproducible. The
decompressed contract is the L1 rule (plan decision D4), and it is what the
comparison enforces.

gzip FASTQ output is compared on its **decompressed** bytes. The compressed hash
is recorded as a note only, because the repo already documents a deliberate
compression-level difference from cutadapt (level 3 vs 6).

---

## Normalisation: every rule and why

Normalisation is a small, explicit list of **named** regex rules attached
per case and per stream. There is no blanket scrub. A rule that stops matching
is a **failure**, not a pass — see "Anti-weakening" below.

| Rule | Applies to | Pattern (summary) | Why |
| --- | --- | --- | --- |
| `cutadapt_finished_line` | cutadapt report stdout | `^Finished in <t> s (<t> us/read; <t> M reads/minute).$` | wall-clock timing. The `--json` report has **no** timing field, so it needs no rule at all |
| `cutadapt_core_count` | cutadapt report stdout | `^(Processing (single\|paired)-end reads on )<n>( cores? ...)$` | resolved worker count depends on `-j` and on the host (`-j 0`). Only the line's shape is contractual |
| `umitools_summary_timings` | `--summary-json` | `"timings":{...}` | wall-clock seconds, `%.3f` |
| `umitools_summary_resources` | `--summary-json` | `"resources":{...}` | host properties — see below |
| `umitools_log_timing_lines` | umitools `-L` log | `^(wall time\|parse+cluster\|sort+commit)\s+:\s+<t>s$` | wall-clock timing |
| `umitools_log_threads` | umitools `-L` log | `threads=<n>` | from `available_parallelism`, capped by the cgroup CPU quota |
| `umitools_log_memory` | umitools `-L` log | `memory=<n>MiB` | from `/proc/meminfo` and the cgroup memory limit |
| `umitools_log_hardware_line` | umitools `-L` log | `^hardware\s+: .*$` | `HardwareProbe::summary()`: logical/physical CPUs, L3 size, available memory, free tmp space, chunk size |
| `umitools_log_container_line` | umitools `-L` log | `^container limits\s+: .*$` | cgroup limits |
| `umitools_temp_staging_name` | `umitools_io_failure` stderr | `.prefix.<pid>.<n>.tmp` | the I/O diagnostic names the staging file (`io/spill.rs`), whose name embeds the process ID |
| `program_name` | clap-generated text only | `<basename>\|<rust-cutadapt>\|<rust-umitools>` → `<BIN>` | see below |
| `absolute_temp_path` | every stream | this run's temp directory → `<TMP>` | the goldens must be reproducible across runs |

### Why `program_name` exists (constraint 1 from the baseline capture)

clap renders usage/help/error text from **argv[0]'s basename** unless a
`bin_name` is pinned. That produces two different divergences:

```
# rust-umitools does NOT pin a bin_name:
Usage: rust-umitools.before [OPTIONS] [INPUT] [OUTPUT] [THREADS]   # .before
Usage: rust-umitools [OPTIONS] [INPUT] [OUTPUT] [THREADS]          # real build

# rust-cutadapt DOES pin .bin_name("rust-cutadapt"), so both sides already agree:
Usage: rust-cutadapt.before ...   # prints "Usage: rust-cutadapt ..."
```

So the rule is an **alternation** of the resolved basename *and* both canonical
names, built with `re.escape` and ordered longest-first so
`rust-umitools.before` is never half-matched by `rust-umitools`. The rule name
is deliberately **stable** (`program_name`) even though the pattern depends on
the run: `rules_fired` is compared between the recorded and the verified
observation, so a per-run name would fail every clap case.

It is attached **only** to cases flagged `clap_text=True`. The provenance block
is deliberately **not** flagged: its first line contains the program name as
*data*, and it is compared byte-exactly.

### Why the provenance block has no rules (constraint 2 from the baseline capture)

`rust-umitools --version --verbose` prints:

```
rust-umitools 0.2.0
reference        : UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)
library          : noodles 0.118.0 (pure Rust; BAM/SAM/CRAM)
parallel         : rayon worker pool, single-threaded BGZF output
commands         : whitelist, extract, dedup, group, count, count_tab
exit codes       : 0 ok, 2 usage, 3 input, 4 io/resource
```

This block is assembled by `bioinfo-common/src/provenance.rs`, so every
character is contractual: the version, the reference hash, the column alignment,
the field order and the line count. The noodles/provenance refresh was approved
on 2026-10-26 and the corresponding contract was updated explicitly. The case is recorded at **L1** with an empty
rule list, and `rules_fired` is empty for it. The `mirror:` and `<TMP>` rules
cannot fire because none of that text contains a path.

The same text is asserted in-tree by
`bioinfo_common::provenance`'s own unit test, and this golden is the independent
check that the shared implementation reproduces it.

### `--summary-json`: what "byte-exact except timings" means

The contract is: everything except the **numeric values inside `"timings"`**.
That includes

* every key name and their order,
* the `"counters"` values (all of them),
* string escaping,
* the exact single trailing newline.

`"resources"` is a **separate, documented deviation** from the task's wording.
It must be normalised because it is not a property of the run at all:

```rust
// rust-umitools/src/runtime/budget.rs
threads     = available_parallelism().min(cgroup_cpu_limit).min(16)
memory_mib  = (/proc/meminfo MemAvailable, min cgroup limit) / 2, clamped
chunk_mib   = f(largest L3 cache, statvfs_free(tmpdir))
```

All three therefore change between machines and even between runs on a busy
host. Comparing them would make the gate machine-locked. This is reported
honestly rather than hidden: see "Could not be made deterministic".

---

## Anti-weakening

A harness that cannot fail is worthless, so:

1. **`--self-test` is mandatory and is part of `m3.sh`.** It
   * first verifies the **clean** golden (so a verifier that always fails cannot
     "catch" mutations), then
   * copies the goldens three times and applies one mutation each: one byte in a
     text stream, one exit code, one hash in a binary stream,
   * asserts `--verify` returns **non-zero** for every mutated copy,
   * prints `SELFTEST PASS` or `SELFTEST FAIL` and exits non-zero on failure.
2. **A rule that stops firing is a failure.** If
   `cutadapt_finished_line` no longer matches, the text it was hiding has changed
   shape, and silently passing would mean the gate no longer covers it.
3. **A missing golden is a failure**, not a skip.
4. **A missing oracle is exit 3**, never a silent skip of the BAM cases.
5. `expect_exit` is a *model* of the exit-code contract. A mismatch is reported
   loudly as `WARN` at record time and the **measured** code is what gets
   recorded — the gate never encodes a guess as if it were evidence.

---

## Could not be made deterministic (stated, not dropped)

| Thing | Why | What is done instead |
| --- | --- | --- |
| cutadapt `Finished in ...` | wall clock | normalised (`cutadapt_finished_line`); everything else in the report is byte-exact |
| cutadapt core count | `-j`, and the host CPU count for `-j 0` | normalised (`cutadapt_core_count`); `-j 1`/`-j 2` still prove the **data** is identical |
| `--summary-json` `timings` | wall clock | normalised (`umitools_summary_timings`) |
| `--summary-json` `resources` | CPU count, cgroup quota, `/proc/meminfo`, L3 size, free disk | normalised (`umitools_summary_resources`). **This is the one place the gate is weaker than "byte-exact"**, and it is a machine property, not a refactor property |
| umitools `-L` timings, `threads=`, `memory=`, hardware/container lines | same host facts, plus wall clock | normalised (five `umitools_log_*` rules); the counters, event summary and message text stay byte-exact |
| I/O diagnostic staging file name | embeds the PID (`.<prefix>.<pid>.<n>.tmp`) | normalised (`umitools_temp_staging_name`); the directory, prefix and error text stay exact |
| absolute temp paths | each run gets a fresh `mkdtemp` | normalised to `<TMP>`; recorded `argv` uses the same rule |
| gzip **compressed** bytes | deflate level/backend is explicitly not part of the contract | recorded as an informational note; the **decompressed** bytes are the L1 contract |
| BAM **BGZF** bytes | the `@PG` `CL` tag embeds the run's temp directory, so the container bytes differ between recordings | not recorded at all; header+records via `samtools view -h` (path-normalised) are the L1 contract, per plan decision D4 |
| `--version` / `--help` program name | argv[0]-dependent (`rust-umitools.before` vs `rust-umitools`) | `program_name` rule on clap-generated text only; the provenance block is excluded and stays byte-exact |

Nothing in the task's case matrix was dropped. The two additions beyond the task
wording are `umitools_count_tab_per_cell` (cheap, exercises the cell-barcode
branch) and `umitools_logfile_dedup` (the log file the task asks for, on the one
command whose log has volatile lines).

---

## Notes found while reading the sources (they changed the cases)

1. **`count_tab`'s TSV shape.** Column 1 is `read_id<sep>UMI`, column 2 is the
   **gene** (`rust-umitools/src/commands/count_tab.rs`); the UMI is the text
   after the last separator. So the fixture is
   `r1_AAAAAAAA<TAB>G1`, not `AAAAAAAA<TAB>3`.
2. **`count_tab` does not accept `--progress`** (it is not in `CountTabArgs`), so
   the invalid-`--progress` case uses `group`, whose `CommonArgs` has the flag.
3. **`count_tab`'s column diagnostic is `BadTable`**, which `error.rs` maps to
   `ExitClass::Input` → **3**, not 4. The case therefore records the measured
   code rather than asserting 4.
4. **`count` writes a TSV, not a BAM** (`outputs: vec![("tsv", ...)]`), so its
   output is compared as text while `dedup`/`group` output is compared through
   `samtools view -h`.
5. **`count_tab -L` is the only umitools log with no timing or hardware line**
   (`log.info("rust-umitools count_tab")` only), which is why it can be the
   byte-exact `-L` case and the dedup one cannot.
6. **`RUST_UMITOOLS_TRACE`** adds `[trace]` lines to stderr and
   **`RUST_UMITOOLS_QUIET`** changes `--progress auto`, so `build_env` strips
   both along with `RUST_UMITOOLS_MAX_PENDING_UMIS`, `RUST_UMITOOLS_MAX_MATES`
   and `RUST_UMITOOLS_IO_THREADS`. `COLUMNS=100` is pinned because clap wraps
   help text to the terminal width.

---

## Determinism of the recording itself

`--record` writes one JSON file per case, sorted by key, with no timestamp, no
hostname, no absolute path and no elapsed time. It is maintainer-only and now
requires `--confirm-record`; `--verify` is the normal validation path. Running
`--record --confirm-record` twice from the same binaries therefore produces
byte-identical files. The committed digest list can be checked with:

```bash
sha256sum -c tests/gate/golden.sha256
```

`m3.sh` checks this manifest before `--verify`. This is checked as criterion
**c** of the historical W1-1 acceptance list. Any future field that is not
reproducible must not be added to a golden; add it as a `note` instead.

---

## Fixes applied after the first recording attempt

The harness was written by an instance whose tool set had no shell, so it could
not execute anything. The recording and the debugging were done by the leader.
Three defects surfaced on the first real run and are fixed here; they are listed
so a reviewer knows what changed rather than assuming the first version was
correct.

| Symptom | Cause | Fix |
| --- | --- | --- |
| `umitools_dedup` and `umitools_group` failed on line 4 of `out.bam`, and a second `--record` produced different bytes for exactly those two cases | `render_bam` compared `samtools view -h` output **without** applying the case's rules, so the `@PG` `CL` tag kept the per-run temp directory | `render_bam` now takes the case's `Normalisation` and renders through `render_text`; the non-reproducible BGZF hash was dropped from the golden |
| `umitools_unsorted_bam` failed on the `hardware ...` line | `dedup` logs its budget and the hardware probe before reading input, so `stderr` carries host facts; the case attached no `log_rules` | the case now uses `normalise={"stderr": list(log_rules)}`; the `NotSorted` diagnostic and counters stay exact |
| `umitools_logfile` recorded `exit=4` instead of `0` | the case name is not `count_tab`-prefixed and is not in the BAM set, so `stage_umitools` staged no `reads.tsv` and the run failed to open its input | `stage_umitools` now stages the TSV fixtures for `umitools_logfile` too |

After these fixes: `--record` is byte-deterministic (verified over two runs),
`--verify` is 37/37 against both the pre-refactor pair and the post-refactor
workspace binaries, and the current `bash tests/gate/m3.sh` reports **10/10
PASS** (the additional criterion is the golden digest manifest) with
`SELFTEST PASS`.

### Fourth fix and one added case

A fourth defect was found while adding a case for `--add-pg`: `render_bam`
returned the result of `render_text` — itself a dict — wrapped in a second dict,
so the recorded `"text"` was an object rather than a string and the difference
printer crashed on any BAM mismatch. It now passes the rendered dict through.
That defect would have made the two BAM cases report a confusing crash instead
of a readable diff; it never produced a false pass.

`umitools_add_pg` was **added** after the initial matrix. W1-1 moved the `@PG`
tag set and its order into `bioinfo_common::provenance::ProgramRecord`, so the
provenance contract has two surfaces: `--version --verbose` (covered by
`umitools_version_verbose`, zero normalisation) and the `@PG` record. Without
this case the second surface would have been covered only by a unit test of the
shared crate. The case compares the full `samtools view -h` rendering of a
`--add-pg` BAM. The `CL` tag's binary path is normalised by
`binary_invocation_path` (a golden recorded from `.before` must verify against
the workspace build); tag names, tag order, `PP` linkage and the remainder of
the command line are byte-exact.

After these changes: 38 cases in that historical run, `--record`
byte-deterministic over two runs, `--verify` 38/38 against both pairs. The
current corpus has 40 cases and the current gate reports 10/10, including the
golden digest manifest.

---

## Recording from the upstream oracle (`record_from_oracle`)

Two cases — `cutadapt_read2_3prime` and `cutadapt_read2_5prime` — are recorded
from the **cutadapt 4.6 oracle**, not from `--cutadapt`.

Reason: the pre-refactor binary answered read-2 `-G`/`-B` incorrectly (it typed
them as 3' adapters and trimmed the wrong end of read 2; see
`docs/EVIDENCE-W1-1.md` DR-10). Recording those cases from that binary would
have frozen the defect into the golden as the expected answer — the gate would
then have *required* the bug.

`record_from_oracle=True` on a `Case` makes `--record` run the upstream tool
instead, and requires `$CUTADAPT_ORACLE` (or the documented resolution chain) to
be present; it exits 3 rather than silently falling back to the rust binary.

This is a narrow, auditable escape hatch. It applies to exactly two cases, it is
declared inline next to them with the reason, and it can only ever make the
expectation *stricter* (the upstream tool is the compatibility authority). It
must never be used to relax a comparison for a refactor regression.

Oracle-recorded cases need two extra rules, because the two implementations
legitimately differ in the banner and in the worker count:

| Rule | Why |
| --- | --- |
| `cutadapt_version_banner` | `This is cutadapt 4.6 with Python 3.14.8` vs `This is rust-cutadapt 0.1.0 with Python 3` |
| `cutadapt_core_count_any_plural` | `on 1 core ...` vs `on 4 cores ...` (the documented worker-count deviation) |

Proof the gate discriminates on this defect: with the current goldens the fixed
binary is **40/40**, while the pre-fix baseline binary is **39/40**, failing
exactly `cutadapt_read2_5prime`. `-A` passes either way, because `-A` was never
broken.

`-B` is deliberately **not** in the read-2 matrix: its trimming shares the 5'
path covered by `-G`, but its text report has a separate pre-existing cosmetic
difference (an omitted empty 5' histogram header, DR-11). Asserting it here
would make the W1-1 gate fail for a reason W1-1 did not cause.
