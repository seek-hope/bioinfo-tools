# Same-input optimization after e125519

This experiment compares the saved native executables from the first README
benchmark with optimized native executables and the pinned upstream programs.
All three baseline native hashes match the original measured binaries exactly.
The earlier tables showed commands that were slower than upstream; differences
between older benchmark generations, inputs and parameters did not establish a
version-to-version regression. This experiment supplies a matched before/after
comparison.

The changes cover `rust-fx stats`, `fq2fa`, `lint`, `rust-bam flagstat`, `sort`, and
`rust-fastqc`. Cutadapt and UMI runtime code are unchanged. No new dependency,
compression-level change, relaxed validation or approximation is introduced.
Compatibility statuses and existing approved exceptions are unchanged.

## What changed

| Path | Removed work | Preserved behavior |
| --- | --- | --- |
| `rust-fx/src/stats.rs` | Per-base `powf`, repeated gap-list searches and conditional base counters | All byte classifications; inclusive Q20/Q30; exactly the same floating-point additions |
| `rust-fx/src/io.rs` and `commands.rs` | Per-record FASTQ allocations/copies in stats, conversion and lint | Empty/long records, CRLF, EOF, malformed/compressed input, diagnostics and output order |
| `rust-fx/src/lint.rs` | An unread set containing every header | All validator decisions; the paired command retains its own required names |
| `rust-bam/src/commands.rs` | Per-record reference-name strings; eight-byte unbuffered sidecar writes | Full decoding/validation, flag counts, sort order, compression, and write-error diagnostics |
| `rust-fastqc/src/stats.rs` | Rebuilding a full GC model for every read | Existing model values and read-order additions; a single most-recent-length cache |

Single-file lint retains record buffers proportional to the longest record,
plus parser/codec buffers, instead of storing all names. Paired lint still keeps
read-one headers. Stats still retains its length histogram. Flagstat adds a
header-sized name table. The QC cache adds O(L) state; existing position-, tile-,
word- and distinct-sequence-dependent structures remain. These are not blanket
constant-memory claims.

The 100k sort syscall trace fell from **103,216 to 3,229 `write` calls**,
with byte-identical BAM output. A separate 100k stats counter check retained
2,939,286,781 versus 892,939,704 user-space instructions on the P-core counter,
again with identical output. These instrumented runs are excluded from the
formal timing table. `perf record` is blocked by host policy; no function-level
sampling profile is claimed.

An intermediate reuse implementation held the previous FASTA record during the
next owned-record assembly. That candidate was stopped and corrected before
acceptance. A two-record fixture with 32 MiB per record measured **38.6 MiB** peak
RSS for the original, **70.8 MiB** for the superseded candidate and **38.6 MiB**
for the final implementation. All three outputs match an independently generated
expected hash. This is a single-observation memory regression check, not a speed
measurement. A failing-stream unit test also verifies early release of the old
record.

## Measurements

<!-- OPTIMIZATION_TABLE_START -->
| Dataset | Command | Before s [min–max] | After s [min–max] | Upstream s [min–max] | Paired before/after | Max RSS MiB before/after/upstream |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| real-1000000 | stats -a -T | 1.972 [1.971–1.974] | 0.621 [0.620–0.626] | 0.501 [0.499–0.501] | 3.17× | 5.3 / 5.2 / 36.3 |
| real-1000000 | fq2fa | 0.200 [0.199–0.201] | 0.171 [0.167–0.172] | 0.251 [0.249–0.257] | 1.17× | 4.8 / 4.8 / 35.9 |
| real-1000000 | lint | 0.529 [0.528–0.532] | 0.236 [0.235–0.236] | 0.197 [0.197–0.198] | 2.24× | 149.7 / 4.8 / 4.4 |
| real-1000000 | FastQC* | 8.025 [8.018–8.046] | 4.479 [4.465–4.511] | 6.171 [6.166–6.175] | 1.79× | 92.0 / 92.1 / 307.8 |
| synthetic-1000000 | stats -a -T | 1.956 [1.955–1.959] | 0.615 [0.613–0.617] | 0.373 [0.370–0.375] | 3.18× | 5.2 / 5.2 / 36.6 |
| synthetic-1000000 | fq2fa | 0.185 [0.183–0.189] | 0.158 [0.154–0.159] | 0.133 [0.131–0.141] | 1.17× | 4.9 / 4.7 / 32.6 |
| synthetic-1000000 | lint | 0.483 [0.481–0.496] | 0.222 [0.222–0.223] | 0.189 [0.188–0.190] | 2.18× | 107.8 / 4.9 / 4.4 |
| synthetic-1000000 | FastQC* | 7.987 [7.978–8.001] | 4.401 [4.395–4.443] | 5.174 [5.166–6.175] | 1.81× | 110.0 / 110.1 / 296.3 |
| synthetic-bam-1000000 | flagstat | 0.083 [0.083–0.084] | 0.070 [0.070–0.070] | 0.037 [0.036–0.037] | 1.20× | 4.0 / 4.1 / 3.9 |
| synthetic-bam-1000000 | sort (32 MiB) | 1.406 [1.391–1.453] | 1.246 [1.231–1.300] | 0.533 [0.531–0.540] | 1.13× | 43.8 / 43.3 / 53.0 |
| synthetic-10000000 | fq2fa | 1.843 [1.771–1.864] | 1.470 [1.445–1.503] | 1.197 [1.164–1.263] | 1.25× | 4.9 / 4.8 / 36.7 |
| synthetic-10000000 | lint | 5.614 [5.583–5.632] | 2.140 [2.131–2.147] | 1.789 [1.782–1.793] | 2.62× | 829.0 / 4.9 / 4.4 |
| synthetic-bam-10000000 | flagstat | 0.815 [0.812–0.822] | 0.703 [0.702–0.718] | 0.319 [0.319–0.323] | 1.16× | 4.0 / 3.9 / 3.8 |

*FastQC before/after compares identical native report bundles. Its upstream values are descriptive: missing native plots and unresolved statistical differences prevent an upstream speed ratio or whole-report equivalence claim.

Ratios above one favor the optimized native executable. Ratios are medians within matched blocks, not ratios of separate medians. Min–max values are observed ranges, not confidence intervals. RSS is the maximum GNU time observation, not simultaneous process-tree total memory.
<!-- OPTIMIZATION_TABLE_END -->

The full population is 13 conditions: real and synthetic 1M FASTQ for stats,
conversion, lint and QC; synthetic 1M BAM for flagstat and sort; and 10M synthetic
records for conversion, lint and flagstat. Every condition has one warm-up and
five measured blocks, each running before, after and upstream: **195 measured
executions and 39 warm-ups**. All six execution permutations are used once across
those six blocks. Five timings of a dataset are not independent biological
replicates.

The accepted population joins ten complete core conditions with a separate
three-condition 10M run using the same frozen binaries, inputs, CPU, command and
comparison functions. The first 10M warm-up exhausted tmpfs because superseded
outputs were deliberately retained. Those outputs were copied to disk and
SHA-256-verified before their temporary originals were removed. All three 10M
conditions were then rerun. The merger rejects any attempt to discard a measured
sample or failed check; the incomplete warm-up and its error logs remain visible.
The retry preserves the block orders that those conditions had in the full plan.
A free-space check now accounts for three coexisting decoded FASTA outputs.
No measurements are taken while transferring retained files.
Sampled package temperatures across accepted warm-ups and measurements were
37–93°C; the observed one-minute load average ranged from 0.13 to 3.22.

This is the same Linux i7-13700H host with 31.1 GiB visible RAM. Commands and
descendants are pinned to logical P-core CPU 4. Input, output and temporary files
reside on **tmpfs**, not physical disk. Inputs are reread before each execution,
followed by two seconds idle. Frequency is not locked, CPU/SMT siblings are not
reserved, and pauses do not guarantee equal temperature. No build, oracle suite,
data generation or other benchmark overlaps formal timing. Output durability
fsync is not requested; the existing sort temporary-file syncs are unchanged.

Wall time includes command startup, GNU time and file I/O. RSS is GNU time's
maximum observation, not aggregate simultaneous descendant memory. Temporary
storage is sampled every 20 ms and may miss shorter peaks. Use these results for
the measured data and machine; do not infer an average suite-wide acceleration.
The [protocol](OPTIMIZATION-PROTOCOL.md) records the population, controls and
amendments before the final run.

## Correctness and acceptance

Both native variants use the unchanged README comparisons against upstream.
Stats/conversion/flagstat require exact output; sort requires quickcheck,
coordinate ordering, expected count and normalized records. Native sort bytes
must additionally match each other. Lint verifies valid input and malformed-tail
rejection, and requires identical native messages apart from timestamps.

FastQC requires **every native data, summary, HTML and ZIP byte to remain
identical**. Its comparison with upstream still checks only independently known
counts/lengths and retains raw module differences. Native graphics are still
incomplete and GC/duplication discrepancies remain unresolved. A before/after
native improvement does not establish upstream whole-report equivalence.

Additional checks cover all quality bytes/offsets, all sequence bytes and
custom-gap overlaps, cache changes across GC/read lengths, reusable buffers,
truncation and deferred sort-index write failures. The adversarial CLI check
compares **104/104** old/new cases, including 64 KiB boundaries and corrupt or
truncated gzip streams. Eight benchmark-evidence controls reject incomplete or
failed measurements and pilot summaries, recompute results from raw samples,
and forbid retries from discarding measured observations or changing artifacts.

<!-- OPTIMIZATION_ACCEPTANCE_START -->
Both complete local gates passed after the final native changes:

```text
QUALITY PASS (base only; external oracles NOT RUN)
README boundary oracle: 22/22
PASS SeqKit corrected semantics: 434 checks
PASS FastQC limits/kmer: 141 cases; {'exact': 111, 'proved-kmer-correction': 27, 'proved-config-rejection': 1, 'exact-preflight-rejection': 2}
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
```

The eight optimization-evidence controls and twelve gate-harness controls also
pass. Final native source hashes and all three local release binary hashes match
the accepted measurements. The archived [validation logs](results/optimization-e125519/validation/)
are fresh executions, separate from earlier phase checks and remote CI.
<!-- OPTIMIZATION_ACCEPTANCE_END -->

## Evidence and reproduction

The [evidence directory](results/optimization-e125519/) contains the raw final
report, all conditions and ranges in Markdown/TSV, executed scripts, build and
input provenance, validation logs, the pilot and the interrupted candidate.
The failed/incomplete population is not included in final summaries. Original
[README observations](results/readme-2026-10-31/) remain unchanged.

The baseline is `e1255197285329e0c184293ef36868de30694ee0`; the optimized native
source is that revision plus the archived `native-source.patch`. The commit
containing this report includes that same native implementation. Rust is 1.99.1.
Both artifacts use a workspace release build followed by a package-only
`rust-fx` build, retaining the original dependency-feature distinction:

```bash
git worktree add --detach /tmp/bioinfo-perf-before e125519
(
  cd /tmp/bioinfo-perf-before
  CARGO_TARGET_DIR="$PWD/target" cargo build --release --workspace --bins --locked
  CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked -p rust-fx
)
CARGO_TARGET_DIR="$PWD/target" cargo build --release --workspace --bins --locked
CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked -p rust-fx
```

Prepare the original inputs and pinned references with the commands in
[RESULTS-README.md](RESULTS-README.md#reproduce). The staging helper in the
[benchmark guide](README.md) accepts existing installations and downloads
nothing. Keep the toolchain pre-extracted. Then run from this optimization
checkout, using a new output directory:

```bash
python3 bench/optimization_benchmark.py \
  --inputs /tmp/bioinfo-readme-benchmark/inputs.json \
           /tmp/bioinfo-readme-benchmark/inputs-large.json \
  --baseline-dir /tmp/bioinfo-perf-before/target/release \
  --native-dir "$PWD/target/release" \
  --outdir /tmp/bioinfo-optimization-replay \
  --oracle-dir /path/to/oracle-bin \
  --toolchain-cache /path/to/pre-extracted-cache --cpu 4
python3 bench/summarize_optimization.py \
  /tmp/bioinfo-optimization-replay/report.json \
  --outdir /tmp/bioinfo-optimization-replay
python3 -W error::ResourceWarning bench/test_optimization_benchmark.py
```

Use an available CPU on another machine and record that change; it will not
reproduce this host's timing. Full gates require the pinned references described
in [tests/ci/README.md](../tests/ci/README.md). Push-triggered native/dependency CI
does not imply that those external oracles were provisioned remotely.
