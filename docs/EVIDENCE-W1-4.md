# W1-4 evidence: `rust-bam`

> Historical record: commands, versions and offline flags describe the session at the recorded commit. They are not current project requirements; see [DOC-AUDIT.md](DOC-AUDIT.md).

Status: **VERIFIED** — the crate is implemented and the samtools 1.24 differential
was run: **20/20 cases pass**.

Canonical machine-readable file: [EVIDENCE-W1-4.json](EVIDENCE-W1-4.json).

Scope: `docs/DEVELOPMENT-PLAN.md` **W1-4**, "`rust-bam` with `view`, `flagstat`,
`idxstats`, `quickcheck`, `index`, `sort`", built on **noodles 0.118.0** and
verified against **samtools 1.24**.

**W1-1, W1-2 and W1-3 remain complete and uncommitted**; their changes are
preserved untouched. W1-5 and later were not started.

## 1. Acceptance matrix

| Acceptance criterion | Status |
| --- | --- |
| `rust-bam` crate exists with all six subcommands | **met** |
| Built on noodles 0.118.0 (plan decision D2) | **met** — no `rust-htslib` |
| `cargo build --offline` resolves noodles | **met** |
| MSRV resolved explicitly, not silently | **met** — raised to 1.91, DR-W1-4-2 |
| SAM text identical to `samtools view` | **met** — 8/8 view cases |
| BAM header+records identical after `samtools view -h` | **met** — `@PG` only, §4 |
| `flagstat` numbers identical | **met** — 2/2 cases |
| `idxstats` numbers identical | **met** — 2/2 cases |
| `quickcheck` agrees | **met** — 2/2 cases |
| Sort order documented and stable | **met** — 4/4 cases, §5 |
| Valid, reproducible BGZF/BAM output | **met** — `quickcheck` exit 0, byte-reproducible |
| `index` interoperable with htslib | **met** — BAI byte-identical; htslib uses it |
| Existing tests/oracles preserved | **met** — 639 tests, all oracles green |

## 2. Dependency and MSRV

### noodles resolves offline

```text
$ ls ~/.cargo/registry/cache/*/ | grep -iE "^noodles"
noodles-0.118.0.crate
noodles-bam-0.97.0.crate
noodles-bgzf-0.52.0.crate
noodles-core-0.21.0.crate
noodles-cram-0.101.0.crate
noodles-csi-0.62.0.crate
noodles-fasta-0.68.0.crate
noodles-sam-0.92.0.crate
```

Locked versions actually used:

```text
noodles      0.118.0
noodles-bam  0.97.0
noodles-bgzf 0.52.0
noodles-core 0.21.0
noodles-csi  0.62.0
noodles-sam  0.92.0
```

Plan decision **D2** is honoured: `rust-htslib` is absent from `Cargo.toml`, so the
differential below is a genuine independent-implementation comparison rather than
a wrapper over the same htslib `samtools` uses.

### DR-W1-4-2 — MSRV raised from 1.88 to 1.91

Every noodles crate declares `rust-version = "1.91.0"` (read from the manifests, not
assumed):

```text
noodles-0.118.0     rust-version = "1.91.0"
noodles-bam-0.97.0  rust-version = "1.91.0"
noodles-sam-0.92.0  rust-version = "1.91.0"
noodles-bgzf-0.52.0 rust-version = "1.91.0"
noodles-csi-0.62.0  rust-version = "1.91.0"
noodles-core-0.21.0 rust-version = "1.91.0"
```

The workspace declared 1.88 for all five members. Leaving that while `rust-bam`
depends on noodles would have been a **false claim**, so it was raised to 1.91 in
`[workspace.package]` and recorded at length in the root `Cargo.toml`.

Consequences, stated honestly:

* The other five crates are buildable on 1.88 today; a shared workspace declares
  one floor, so their effective minimum rises to 1.91 even though their own code
  does not need it. Accepted to keep the single workspace W1-1 established.
* A separate workspace for `rust-bam` was rejected for the same reason.

The claim is **testable**, and both directions were checked:

```text
$ rustup run 1.88.0 cargo build --offline -p rust-bam
error: rustc 1.88.0 is not supported by the following packages:
  noodles@0.118.0 requires rustc 1.91.0
  noodles-bam@0.97.0 requires rustc 1.91.0

$ rustup run 1.91.0 cargo test --workspace --offline
passed: 639
```

## 3. Oracle

| Component | Version | SHA-256 | Resolution |
| --- | --- | --- | --- |
| `samtools` | **1.24** (htslib 1.24) | `730fcc3c41bd851bfe4ff1cf8c4adf5cc48e9da01d40a1bb843fa598bcdec342` | `--samtools`, then `$SAMTOOLS_ORACLE`, then `$BIOINFO_ORACLE_BIN`, then `$CONDA_PREFIX/bin`, then `PATH` |

No personal path is hardcoded anywhere: `tests/oracle/w14_diff.py` resolves the
oracle through `resolve_samtools()`, and the harness **exits 3** when it cannot.

`rust-bam` binary under test:
`321092b29da8f06d7e4a489c52e9ed7e9879618bf01b78163bbd439086c3e48a`

## 4. Differential result

```text
$ python3 tests/oracle/w14_diff.py --rust target/release/rust-bam
rust-bam : /…/target/release/rust-bam
samtools : /…/envs/dedup/bin/samtools
samtools 1.24
cases    : 20

PASS  view       view_basic                 SAM text exactly matches samtools view
PASS  view       view_header                -h includes the header
PASS  view       view_header_only           -H prints only the header
PASS  view       view_count                 -c prints the record count
PASS  view       view_empty                 a header with no records
PASS  view       view_sample_text           SAM input is accepted as well as BAM
PASS  view       view_no_header_input       a headerless SAM record stream
PASS  view       view_bam_roundtrip         -b output re-read by samtools matches
PASS  flagstat   flagstat_basic             all 16 counters match
PASS  flagstat   flagstat_empty             N/A percentages on an empty file
PASS  idxstats   idxstats_basic             per-reference counts and the * row
PASS  idxstats   idxstats_empty             zeros with no records
PASS  quickcheck quickcheck_valid           a valid BAM passes
PASS  quickcheck quickcheck_empty_valid     a header-only BAM passes
PASS  sort       sort_unsorted              coordinates ordered as samtools does
PASS  sort       sort_ties                  same-start records keep forward before reverse
PASS  sort       sort_basic_sorted          an already-sorted file is unchanged
PASS  sort       sort_empty                 an empty file sorts to an empty file
PASS  index      index_sorted               BAI is byte-identical to samtools'
PASS  index      index_rejects_unsorted     an unsorted file is refused with an input-precondition failure

20/20 cases passed
```

Cases by command: `view` 8, `sort` 4, `flagstat` 2, `idxstats` 2, `quickcheck` 2,
`index` 2.

### The one normalisation applied, and why it is unavoidable

`samtools` appends its own `@PG` provenance line to any header it writes:

```text
@PG  ID:samtools.1  PN:samtools  PP:samtools  VN:1.24  CL:samtools view -h in.bam
```

That line names the tool and its command line, so it **cannot** match between two
implementations, and it carries no alignment content. The `pg_lines` rule drops
`@PG` lines from **both** sides. Everything else is compared byte for byte:
`@HD`, `@SQ`, `@RG` and any `@PG` line that came from the *input* are all still
strict.

Measured before the rule existed, to confirm it was the only difference:

```text
$ diff <(samtools view -h basic.bam) <(rust-bam view -h basic.bam)
7d6
< @PG	ID:samtools.1	PN:samtools	PP:samtools	VN:1.24	CL:samtools view -h in.bam
```

One line, provenance only.

### BGZF/BAM validity and reproducibility

```text
$ rust-bam view -b basic.bam -o ours.bam
$ samtools quickcheck ours.bam ; echo $?
0
$ python3 -c "…compare last 28 bytes…"
ends with htslib EOF marker: True
```

Reproducibility — the same command twice produces identical bytes:

```text
$ rust-bam view -b basic.bam -o det1.bam && rust-bam view -b basic.bam -o det2.bam
$ cmp det1.bam det2.bam && echo REPRODUCIBLE
REPRODUCIBLE
$ rust-bam sort unsorted.bam -o s1.bam && rust-bam sort unsorted.bam -o s2.bam
$ cmp s1.bam s2.bam && echo SORT REPRODUCIBLE
SORT REPRODUCIBLE
```

Our BAM is **not** byte-identical to samtools' BAM for the same records, and that is
correct: htslib and noodles split the stream into BGZF blocks differently. Block
boundaries are an encoding choice, not part of the format contract. What is
contractual — the decoded header and records — matches, and htslib accepts our
container.

### `index` interoperability, the strongest single result

The BAI `rust-bam` writes is **byte-identical** to the one `samtools index` writes
for the same BAM (176 bytes), and htslib can *use* our index to answer region
queries:

```text
$ rust-bam index sorted.bam
indexed 10 records into sorted.bam.bai
$ samtools view -c sorted.bam chr1     # answered via OUR .bai
8
$ samtools view -c samtools_indexed.bam chr1
8
```

## 5. Documented sort order (DR-W1-4-3)

`samtools sort` defines steps 1–4; steps 5–6 make the order **total**, which is what
"stable" requires (samtools leaves such ties unspecified):

1. Unmapped/unplaced records (0x4, or no `RNAME`, or no `POS`) sort **last**.
2. By reference sequence id in `@SQ` header order (not alphabetical).
3. By leftmost alignment start, ascending.
4. Forward strand (0x10 clear) before reverse at the same position.
5. By record name.
6. By original input index — so two records identical in every compared field
   still come out in input order.

There is no parallelism and no hash iteration in the sort path, so output cannot
vary run to run. Verified: `sort_ties` passes, which is the case that would expose a
missing strand tie-break, and repeated runs are byte-identical (§4).

## 6. Real bugs found and fixed during W1-4

Recorded because each was found by measurement, not by inspection.

| # | Bug | Evidence | Fix |
| --- | --- | --- | --- |
| 1 | `flagstat` "with mate mapped to a different chr" wrongly required flag **0x2** (properly paired) | samtools counted 2 where we counted 0 for records flagged `1` with `RNEXT≠RNAME`; an isolated 2-record fixture confirmed 0x1 alone suffices, and 0x8 must exclude | `stats::FlagStat::add` now keys on 0x1, not 0x8, and drops the 0x2 requirement |
| 2 | `view -h` collided with clap's auto `-h` for help, panicking at CLI build time | `clap_builder` debug assert: "Short option names must be unique … '-h' is in use by both 'with_header' and 'help'" | `disable_help_flag(true)` on `view`, explicit `--help` that does not require `<FILE>` |
| 3 | Malformed input exited **3** where samtools exits **1** | `samtools view nh.sam` (headerless SAM) → exit 1; `rust-bam` → exit 3 | unparseable/corrupt input is now `ExitClass::Runtime` (1); 3 is reserved for actionable preconditions like indexing an unsorted BAM |
| 4 | Format sniffing consumed 4 bytes then handed the already-advanced reader to the gzip decoder | self-test `gzip_that_is_not_bam_is_rejected` failed with exit 4 instead of 3 | re-prepend the consumed prefix via `Cursor::chain` |
| 5 | `index`'s sortedness check compared a `Position` to a `usize` | compile error | convert with `usize::from(start)` |

Fixture bugs (mine, not the implementation's) were also found and fixed: the first
`BASIC_SAM` was not coordinate-sorted, so *samtools itself* refused to index it
("Chromosome blocks not continuous"); and a headerless SAM cannot be converted to
BAM (`no SQ lines present in the header`).

## 7. Harness defects fixed

The W1-3 lesson was applied up front, and the self-test guards it:

* `build_fixtures` owns `work.mkdir(parents=True, exist_ok=True)` — the exact
  directory-creation bug that crashed the W1-3 harness before case 1.
* Both tool paths are resolved to absolute paths, because every case runs with
  `cwd=<work>`; a relative `target/release/rust-bam` would not resolve.
* `--self-test` validates every case's `compare`/`argv`/name/category/guard, checks
  for duplicate case names, and asserts the `@PG` rule neither under- nor
  over-matches (`@PGx…` is *not* a header line).
* The self-test caught a real over-match: the first `@PG` regex left a blank line
  behind, which would itself have been a spurious difference.

**Negative control:** mutating the `flagstat` percentage format from `{:.2}` to
`{:.1}` drops the differential from **20/20 to 19/20**; restoring it returns
20/20. The comparison has teeth.

## 8. Verification results

All commands run by me, offline, in this session.

| Check | Command | Result |
| --- | --- | --- |
| W1-4 differential | `python3 tests/oracle/w14_diff.py --rust target/release/rust-bam` | **20/20 cases passed**, exit 0 |
| Harness self-test | `python3 tests/oracle/w14_diff.py --self-test` | **SELFTEST PASS** |
| Formatting | `cargo fmt --all -- --check` | clean |
| Lints | `cargo clippy --workspace --all-targets --offline -- -D warnings` | **0 warnings** |
| Workspace tests (debug) | `cargo test --workspace --offline` | **639 passed, 0 failed** |
| Workspace tests (release) | `cargo test --workspace --release --offline` | **639 passed, 0 failed** |
| MSRV 1.91 check | `rustup run 1.91.0 cargo test --workspace --offline` | **639 passed** |
| MSRV 1.88 refusal | `rustup run 1.88.0 cargo build --offline -p rust-bam` | refuses, naming noodles (as intended) |
| cutadapt 4.6 | `python3 tests/check.py` (in `rust-cutadapt`) | all visible conformance cases passed |
| UMI-tools exact | `bash tests/oracle/run_exact.sh` | **77/77 byte-identical, 0 differ** |
| UMI-tools semantic | `bash tests/oracle/run_determinism.sh` | **deterministic: 0 mismatches** |
| W1-3 differential | `w13_diff.py` with both oracles | **50/50 cases passed** |
| M3 byte-neutrality gate | `bash tests/gate/m3.sh` | **9/9 criteria, M3 PASS** |

### Test counts

| Crate | Tests |
| --- | --- |
| `bioinfo-common` | 39 |
| `bioinfo-fastx` | 96 |
| **`rust-bam`** | **52** |
| `rust-cutadapt` | 201 |
| `rust-fx` | 178 |
| `rust-umitools` | 73 |
| **Total** | **639** |

639 = the W1-3 total of 587 **plus 52 new `rust-bam` tests**. Per-crate counts sum
exactly to the workspace total.

`rust-bam`'s 52 tests: `format` 8, `stats` 15, `sort_order` 11, `commands` 8,
`error` 4, `index_util` 1, binaries 6 — the counting and ordering logic is tested
without touching the filesystem, which is why a formatting or comparator mistake
fails a unit test rather than only the oracle.

## 9. Limitations — what is not claimed

* **CRAM is not supported.** Detected and rejected with a clear message rather
  than misparsed. `samtools` supports it; `rust-bam` does not.
* **CSI indexes are not written.** Only BAI (`samtools index -c` is unmatched).
  noodles can build CSI; the option is deliberately absent rather than advertised
  and unimplemented.
* **Region queries are not implemented on our side.** `rust-bam view` has no
  `chr1:100-200` argument. htslib *can* use our index, which is what the index case
  proves; we do not yet consume our own.
* **`sort` buffers the whole file in memory.** Sorting needs random access; memory
  is proportional to input size, not bounded. W1-4 does not claim a streaming sort.
* **No `-t`/`--threads`, no `-f`/`-F`/`-q` filters, no `-L` BED input, no `-s`
  sampling.** The six commands cover the plan's matrix, not samtools' full CLI.
* **Byte-identical BAM to samtools is not claimed and is not achievable** — BGZF
  block boundaries legitimately differ. Decoded content and container validity are
  what is claimed.
* **`samtools sort`'s tie order for records identical in name *and* position is
  unspecified**; `rust-bam` adds name and input index to make it total. This is a
  documented deviation (DR-W1-4-3), not a mismatch — the differential passes because
  the cases exercise positions that samtools orders deterministically.
* **Only linux x86_64 measured.**

## 10. Repository state

* No release, no tag, no version bump, no publication.
* Versions: `bioinfo-common` 0.1.0, `bioinfo-fastx` 0.1.0, **`rust-bam` 0.1.0**,
  `rust-cutadapt` 0.1.0, `rust-fx` 0.1.0, `rust-umitools` 0.2.0.
* Workspace members now six: the five previous plus `rust-bam`.
* `rust-cutadapt` @ `4609a0c6eb835c9117915c6da41267712ef38773`, 0 tags.
* `rust-umitools` @ `40b5b709247e84e00d81ca05c3b37c107a9f014d`, 1 pre-existing tag,
  7 pre-existing modified paths — **not touched by W1-4**.
* New runtime dependencies in `rust-bam`: `noodles`, `bioinfo-common`, `clap`,
  `flate2`. `clap` and `flate2` were already workspace dependencies; no new crate
  was introduced for W1-4.
* The root `.gitignore` excludes `target/`, `tests/ci/env/bin/` and `.work/`, so no
  downloaded oracle binary can be committed.

## 11. Reproducing this evidence

```sh
cd /path/to/bioinfo-tools

# Build (offline, as required).
cargo build --release --offline -p rust-bam

# The differential needs samtools 1.24. Point the resolver at it if it is not on
# PATH; never hardcode a path.
export SAMTOOLS_ORACLE=/path/to/envs/dedup/bin/samtools
python3 tests/oracle/w14_diff.py --self-test
python3 tests/oracle/w14_diff.py --rust target/release/rust-bam

# The MSRV claim, both directions.
rustup run 1.91.0 cargo test --workspace --offline     # passes
rustup run 1.88.0 cargo build --offline -p rust-bam    # refuses, naming noodles

# The rest of the gates.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo test --workspace --release --offline
bash tests/gate/m3.sh
```

## 12. Files created for W1-4

* `rust-bam/Cargo.toml` — noodles 0.118.0 (bam, bgzf, core, csi, sam), bioinfo-common, clap, flate2.
* `rust-bam/src/lib.rs` — crate documentation and the `COMMANDS` list.
* `rust-bam/src/error.rs` — `Error`, the 1/3/4 split, and the shared exit classes.
* `rust-bam/src/format.rs` — SAM/BAM/CRAM content sniffing.
* `rust-bam/src/io_util.rs` — one reader over SAM and BAM; SAM and BAM writers.
* `rust-bam/src/stats.rs` — `FlagStat` and `IdxStats` counters.
* `rust-bam/src/sort_order.rs` — the documented total order.
* `rust-bam/src/index_util.rs` — BAI construction plus the sortedness precondition.
* `rust-bam/src/commands.rs` — the six subcommands and their exact output formats.
* `rust-bam/src/main.rs` — clap CLI, provenance block, exit-code mapping.
* `tests/oracle/w14_diff.py` — the 20-case differential and its self-test.
* `.gitignore` — excludes build artefacts, oracle binaries and scratch space.
* `Cargo.toml` (root) — `rust-bam` added to members; MSRV raised to 1.91 with the
  reasoning recorded inline.

No release, tag, version bump or publication.
