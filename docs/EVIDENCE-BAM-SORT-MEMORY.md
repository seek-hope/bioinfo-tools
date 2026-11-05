# Explicit BAM sort memory strategy: feasibility and run-schedule evidence

This experiment changes the existing `--max-memory` argument from `512MiB` to
`2GiB` for a frozen input. It does not change source, defaults, the historical
512 MiB condition, the 64-run fan-in or the accepted resource envelope. There is
**no accepted all-budget elapsed-speed summary yet**: the first attempt failed
preflight, two later attempts were interrupted, and a subsequent shared-load screen
completed all 12 runs but only one of six pairs passed its background-comparability
checks. All six raw pairs favor 2 GiB; their interpretation is bounded below.

## Frozen workload and implementation

Both arms use `target/surpass-d3d8edc/quickcheck-sealed/rust-bam`, SHA256
`5b82922b4c8edc87a0e7566d4971ce2e4b92d2acfb044d94c4e578c9addd9855`.
All 129 current runtime hashes were restored to that seal after withdrawing the
histogram/direct-arena micro-optimizations. The native implementation still uses
full decoding, ordinary normalized encoding, compact chunks, the borrowed-quality
adapter, radix sorting and regular-file parallel initial BAM input.

The frozen 86M-record BAM is
`target/scale-benchmark/enlarged-inputs/synthetic-bam-1000000-x86/unsorted.bam`,
SHA256 `1bf69ba64fdb7ddfd8c4ff57cb0685a61f6aeb22f244a343f759482d53fb5bbf`.
Repeated source records increase volume, not biological diversity. Commands use
`sort --max-memory <512MiB|2GiB> --max-open-files 64 --threads 4 -o output.bam`.

## Successful controls and observed schedule

An uninstrumented 2 GiB feasibility invocation validated all output bytes, exit,
diagnostics, cleanup and observed resources. It ran alongside repository gates,
so its elapsed time is ineligible. See `sort-memory-control1/validation.json`.

Separate `strace -f -qq -e trace=openat` diagnostics captured actual run-file
creation and read opens. No traced elapsed time is used as speed evidence.
Both outputs matched SHA256
`fb9c67cd073449570a39ae7d659463008ee1eaebcb403e3032fef2b710623774`,
with empty stdout, exact `sorted 86000000 records\n` stderr and no temporary
leftovers. Final analysis is `sort-memory-trace3/validation.json`.

| Configured estimate | Initial runs | Intermediate runs | BAM read-open calls | Peak sampled aggregate RSS | Peak sampled logical files |
|---|---:|---:|---:|---:|---:|
| 512 MiB | 88 | 2 | 180 | 324886528 B | 2101116127 B |
| 2 GiB | 22 | 0 | 44 | 1224609792 B | 1193724005 B |

`open_run` calls `AlignmentReader::open`, which opens for format sniffing and then
reopens for decoding. Thus each logical run has exactly two read-open calls.
The analyzer checks this multiplicity rather than silently removing duplicates.
The 512 MiB path creates 88 runs before the first run-read, then creates two more
while reducing its 64/24 groups; the 2 GiB path creates all 22 before any run-read
and no intermediate output thereafter. Together with the unchanged `reduce_runs`
control flow, this confirms removal of one full intermediate merge level.
It does not measure compression time saved or guarantee lower overall elapsed time:
larger chunks change sorting locality, retained memory and final merge width.

The first analyzer accidentally matched the final `run-000000/output.bam` path.
The second assumed one read-open per run. Both failures and source are preserved;
source inspection and the third analyzer corrected those assumptions. The third
analysis reuses the successful original 512 MiB trace and runs a fresh 2 GiB
trace. No oracle fixture or expected output was edited. See
`sort-memory-trace-failures.md` for the provenance chain.

## Bounds and next timing comparison

All controls stayed inside the sampled 8 GiB aggregate RSS / 32 GiB file /
1800 second envelope, with at least 16 GiB filesystem headroom. The trace runs'
minimum observed headroom was 131982254080 bytes. These checks are not OS caps or
reservations. GNU-time process maximum RSS and sampled simultaneous aggregate RSS
are different observations. Inputs and earlier batch outputs are separately
retained outside each later invocation workspace.

`--max-memory` remains a record-memory estimate. Variable auxiliary/CIGAR heaps,
private encoding buffers, owned fallback records, arena capacity slack, radix
scratch and compression/merge buffers can increase actual RSS. The approximate
1.14 GiB observed RSS at `2GiB` applies only to this input. Different configured
budgets may also change which parse/encoding/output error occurs first; fixed-budget
error semantics are preserved, but cross-budget error identity is not assumed.

`target/surpass-d3d8edc/screen-sort-memory.py` prepares a separate native
512 MiB-versus-2 GiB comparison at all 1/2/4-core budgets, with two balanced paired
blocks and identical sealed executable/input. It is exploratory, without warmup
or calibration, and has no upstream arm. It checks competing verifier/build jobs
and a short process-CPU observation before and after measurements; these checks
still cannot guarantee an exclusive host or detect every transient workload.
The first attempt in `sort-memory-screen1/report.json` was rejected before any
measurement because other work was active. No accepted speed ratio exists for
this strategy. A useful native result would still need a fresh calibrated upstream
comparison within the same resource envelope, with upstream settings chosen fairly.

## Interrupted paired attempts, 2026-11-05

After authorization to start, `screen-sort-memory2.py` ran twice into distinct
exclusive destinations. Each attempt had a fresh ten-second load preflight and
pre-execution identity/output contracts in `sort-memory-preflight2/` or
`sort-memory-preflight3/`. The workload, seed, order, source, binary, affinities,
resource limits and predeclared load thresholds were unchanged. No build, test,
profile or other benchmark was launched by this agent during either attempt.

| Attempt | UTC start → rejecting postcheck | Completed / planned invocations | Rejection |
|---|---|---:|---|
| `sort-memory-screen2` | 06:20:00.702 → 06:27:54.707 | 6 / 12 | Observed background aggregate CPU 2.305 cores exceeded the 2-core threshold |
| `sort-memory-screen3` | 06:29:21.874 → 06:34:49.546 | 4 / 12 | One background `MainThread` process used 1.012 cores, exceeding the 1-core threshold |

The rejecting observations span about two seconds; they are not proof of sustained
load throughout a measurement. Conversely, the successful preflights do not prove
an exclusive host. The first interruption involved browser/desktop activity; the
second process is retained under its observed name, not attributed to an inferred
application. Neither threshold was relaxed, and no unrelated process was stopped.
After the second interruption, automatic retries stopped to avoid repeated
resource use and selective reporting.

All ten retained invocations independently passed `validate_byte_batch` against
the pre-frozen complete stdout/stderr/output contract, exit code, measurement
identities, typed raw samples and resource limits. The auditor regenerated the
entire seeded schedule and checked each actual invocation against its prefix,
checked globally unique PID/start identities, and retained the missing conditions
explicitly. It also reconciled each invocation's observed file peak with earlier
retained batches: approximately 2.52 GiB and 2.20 GiB for the two attempts,
respectively, below the 32 GiB envelope. Inputs and outputs from other attempts
remain outside that per-screen accounting.

`sort-memory-interrupted-audit1/report.json` records correctness success but
`performance_eligible: false`. Raw timings are retained, not pooled across attempts
or promoted into an accepted ratio. In particular, a complete single-core prefix
does not repair the incomplete 1/2/4-core population or the failed postcheck. No
benchmark remains running. A new attempt needs a new destination and another
quiet-host preflight under that original policy; the two interrupted populations
must remain visible. The user subsequently authorized a different shared-load
policy for a new experiment, documented below.

## Completed shared-load screen, 2026-11-05

The user explicitly permitted stable non-idle CPU load. The new experiment in
`target/surpass-d3d8edc/sort-memory-shared1/` completed from
**07:09:55.639 UTC to 07:24:13.251 UTC**, without a concurrent build/test/profile
launched by this agent. It retained all twelve invocations even when background
comparability failed. No earlier observation was reclassified or pooled.

The pre-execution `contract.json` freezes commands, input, runtime/measurement/
validation sources, expected byte contracts, environment hash and the policy in
`SHARED-LOAD-PROTOCOL.md`. All **12 complete BAM outputs**, stdout/stderr, exits,
raw events, resource observations and artifact populations passed the independent
byte audit. `sort-memory-shared1-audit/report.json` also regenerates the seeded
schedule, recomputes load summaries and checks invocation identities and signatures.

### Observed times — descriptive, not an accepted all-budget speed summary

| Physical cores | 512 MiB median seconds | 2 GiB median seconds | Two paired 512 MiB / 2 GiB ratios | Load-comparable pairs |
|---|---:|---:|---|---:|
| 1 | 85.166 | 68.160 | 1.2329, 1.2661 | 0 / 2 |
| 2 | 75.091 | 56.187 | 1.3493, 1.3237 | 1 / 2 |
| 4 | 71.401 | 51.478 | 1.3641, 1.4105 | 0 / 2 |

These are observed elapsed results from the same input/binary and paired core
budget, not stage-speed multiplications or historical upstream ratios. All six
pairs favor 2 GiB. However, the protocol requires both pairs to pass load checks
before producing an eligible per-budget median, so **all three eligible medians
remain null**. The one comparable two-core pair was 74.696 seconds versus
56.428 seconds (1.3237×); one pair is not a replicated acceptance result.

### Load policy and actual differences

The policy has no absolute background-CPU ceiling. It checks:

- paired mean CPU difference at most max(0.25 cores, 20% of the pair mean);
- paired p90 difference at most max(0.50 cores, 25% of the pair p90 mean);
- within-run half-window mean drift at most max(0.35 cores, 30% of the run mean);
- at least 95% timing-window coverage, bounded gaps and visible-process/host-CPU
  reconciliation;
- visible background read/write differences at most max(5 MiB/s, 25% of the pair
  mean), with substantial parent/child IO ambiguity marked ineligible.

These are predeclared operational tolerances, not statistical equivalence tests.
The observed background was not uniformly stable:

| Cores / block | Background mean cores, 512 MiB / 2 GiB | Main rejection or qualification |
|---|---|---|
| 1 / 0 | 0.771 / 1.126 | CPU difference/drift and ambiguous child-lifecycle IO |
| 1 / 1 | 0.964 / 1.358 | CPU difference/drift and process/host reconciliation |
| 2 / 0 | 0.940 / 1.342 | CPU difference and within-run drift |
| 2 / 1 | 1.835 / 1.720 | All declared checks passed |
| 4 / 0 | 2.025 / 1.387 | CPU difference and within-run drift |
| 4 / 1 | 1.428 / 0.862 | CPU mean/p90 difference and within-run drift |

The background census excludes the measured process/session descendants and
observer by PID/start identity. It uses duration-weighted rates and half-window
splits, not averages of unweighted samples. Census timing and process lifecycle
are imperfect: short-lived work can be missed; per-core placement, GPU/cache/IRQ
contention and inaccessible IO cannot be certified equal. In particular,
`io_accounting_complete` remains false. Parent-reaped IO is marked ambiguous, not
presented as independently measured physical disk traffic. Review corrected two
prototype defects before timing (child IO roll-up and birth timestamp boundaries);
13 controls and a fresh live-sampler byte-contract control passed. Prototype and
control evidence remain retained.

### Resource bounds and observer cost

Maximum sampled aggregate benchmark RSS was **1221791744 bytes**; maximum sampled
invocation files **2108760064 bytes**; maximum complete-screen files including
previous retained outputs **3631271936 bytes**. Minimum headroom was
**225227694080 bytes**. All stayed within the recorded resource envelope. These
remain observations rather than OS caps; inputs and other attempts are separately
retained. The extra observer consumed 1.314–2.348 CPU seconds per invocation and
ran inside timing on both arms. The maximum recorded sensor value was 105050
millidegrees Celsius; frequency and thermal conditions were not fixed.

The result supports pursuing the 2 GiB strategy, whose removal of an intermediate
merge was already independently demonstrated. It does **not** establish a fair
all-budget speed margin, universal superiority or performance relative to
samtools. Defaults and runtime source are unchanged; further comparisons must
retain this complete population and its failed comparability checks.

After timing and the raw audit finished, both full repository gates were rerun,
followed by the sealed BAM oracle. Actual output was `QUALITY PASS (base only;
external oracles NOT RUN)`, `QUALITY PASS (base + FASTX/QC oracles)` and
`20/20 cases passed`. See `shared-final-gates.json` and its three logs. Runtime,
measurement and validation identities remained unchanged; these checks ran after,
not concurrently with, the measured screen.
