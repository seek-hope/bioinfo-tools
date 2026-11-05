# Expanded measurement primitives

`superiority_batch.py` executes a fixed-size workload repeatedly through fresh CLI
processes. `superiority_resources.py` observes its process tree and workspace.
They are execution primitives. `superiority_plan.py` now supplies pure checks for
complete calibration, frozen paired schedules and per-condition acceptance.
`superiority_evidence.py` verifies raw batches against caller-frozen deterministic
complete-byte output contracts. `superiority_inventory.py` reconciles the matrix
and family obligations, with concrete BAM short-command specifications. Broader
semantic family adapters, the remaining executable cases and the formal execution
driver remain unfinished. Existing scale results do not gain these controls
retroactively.

## Coverage inventory — not execution evidence

The inventory preserves all 11 historical conditions and declares all 16 protocol
families against an immutable required-mode baseline. All current compatibility
rows must be supported-and-mapped, explicitly unsupported/unachievable, or
explicit non-workload metadata/validation rows. Unknown supported commands fail.
Custom case populations cannot omit or rewrite historical conditions. Declaring a
mode is not proof that it has run; `complete` remains false while cases lack
commands, inputs, validators or equal-work evidence.

`bam_short(inputs, executables)` materializes 18 concrete cases with both arms
and all three budgets (108 specifications). It requires actual regular input
files, 100 distinct path/inode inputs where declared, no semantic-role aliases,
and matching exact-arm work. Header-only, errors, compressed headers and
regular-file stdin are included; live pipes and biological diversity are not.
The frozen inventory is `target/surpass-d3d8edc/inventory-short6.json`.

The 108 specifications link to 216 fully byte-validated invocations (204 retained
controls plus 12 new header-only invocations), with 18 stable complete signatures.
This is integration/correctness evidence only. The independent audit is
`inventory-short-validation3-audit4.json`; failed literal-header execution,
prototype sources, review findings and counting corrections remain retained.
The final 69-control run includes the other resource/batch/plan/evidence tests.
No old or new elapsed ratio becomes a formal result through this inventory.

## Fixed input, calibrated process count

A specification has exactly these fields:

```python
spec = {
    "schema_version": 1,
    "count": 100,
    "argv": ["/absolute/sealed/rust-bam", "flags", "99"],
    "stdin": None,
    "expected_exit": 0,
    "cores": 1,
}
```

Ordinary argument strings are passed literally, without a shell. An argument such
as `{"run_path": "output.bam"}` refers to a distinct output inside each invocation's
prepared directory. `{"run_path": "."}` selects that directory itself. The caller
must route every output, index, cache relevant to the comparison and temporary
file into the measured workspace. `TMPDIR` is set separately for each CLI; tools
that ignore it require explicit command options. Input paths remain fixed and
must be hashed and conditioned by the experiment driver before timing.

`run_batch(spec, fresh_directory, environment, Limits())` preserves every stdout,
stderr and generated artifact, plus commands, CLI PID/start-tick identities,
initial CPU affinity, exit codes, GNU-time metrics and resource samples. It never
validates or deletes CLI outputs while timing. A separate verifier must inspect
**every invocation**, including diagnostics and expected error cases, and compare
native signatures across measured blocks and CPU budgets.

Timing includes Python helper startup, fresh CLI startup, launch/redirection and
per-invocation event logging. It excludes preparation, input conditioning, output
verification, observer joining and post-exit fsync. Report total batch time,
batch throughput and mean time per invocation; this is not a distribution of
individual CLI latencies. Failed/incomplete batches have no throughput or mean
fields. A successful execution explicitly retains `correctness_validated: false`
until the separate experiment verifier establishes correctness.

`calibrated_count` grows the process count using the fastest observation across
both arms and every selected CPU budget. It never changes input size. The caller
must supply the complete calibration population, retain failed/short rounds,
freeze the resulting count and scripts, and verify every calibration arm is at
least 10 seconds. Formal execution still requires one warmup, three balanced
paired measured blocks, a recorded order seed, and ratios computed inside each
block before taking their median.

## Observed resources

Defaults are 8 GiB aggregate observed process RSS, 32 GiB workspace files,
16 GiB available filesystem headroom, 1800 seconds and a 1-second sample interval.
Logical and allocated regular-file sizes are both recorded; exceeding either file
limit aborts the batch. Hard links are counted once per inode. Outputs from earlier
CLI invocations remain present for later verification, so the file limit applies
to their cumulative batch footprint. This is stricter than a separate file limit
for each short CLI. Reserve both comparison arms, retained anchors and input
preparation separately before starting an experiment.

The observer discovers the subprocess session, known descendants and visible
parent relationships, retaining PID/start-tick identities. It sums process RSS,
which can count shared pages more than once. GNU-time's high-water RSS is also
checked, but does not represent simultaneous aggregate RSS. System samples include
load, temperatures and frequency readings when available. CPU use, filesystem I/O
and untimed fsync duration are retained with the batch.

Observed violations, an observer error, incomplete execution, unexpected exits,
changed code/plan identities or descendants surviving the helper prevent an
execution pass. A separate timer enforces timeout even during a slow file scan.
Joining a slow observer is not charged to the CLI timeout. Process termination
uses the new session and checked known identities; this is not a security sandbox.

These are sampled abort guards, not OS-enforced memory/disk bounds. Samples can
miss short peaks, rapidly exiting or undiscovered escaping children, short-lived
files and unlinked open files. File scans are not atomic. Files outside the
workspace are not counted. Symlinks and nonregular workspace artifacts cause an
observer failure rather than being followed or silently omitted. Directory space,
quotas and other processes can also exhaust the filesystem; recorded available
space does not reserve it. The experiment driver must account for these limits
when deciding resource eligibility and must preserve failures.

## Calibration and paired-plan checks

Essential protocol rules are checked by `superiority_plan.py` (without running
commands):

- Every case must retain all 1/2/4-core budgets and both arms. Missing, duplicate,
  extra or reordered observations are rejected rather than silently removed.
- A fixed input identity and batch count apply across budgets. Complete calibration
  proposes count growth from the fastest arm/budget. Freeze uses the count actually
  measured, never the newly proposed unmeasured count, and requires all arms at
  least 10 seconds with every invocation validated.
- One warmup and three measured pairs are frozen with a recorded seed and explicit
  schedule. AB/BA order is exactly balanced over the four pairs; the three measured
  pairs necessarily have a 2:1 order split. Actual execution must match the schedule.
- Within-block current/upstream ratios are computed first, then their median.
  Every eligible condition must be at most 0.90. There is no suite-average shortcut
  and upstream/current=1.10 is not enough.
- Failed, short or incompletely validated conditions remain visible without rates.
  Warmup results are validated but excluded from ratios. Native output signatures
  must agree across calibration, warmups, measured blocks and CPU budgets.
- Correctness corrections do not automatically establish equal work. Each case
  declares comparison scope and an independently established equal-work assessment.
  Descriptive/unequal-work conditions have no ratio and prevent an overall pass.

The module consumes **normalized evidence supplied by a caller**. Report hashes
and validation booleans are not self-authenticating. The complete-byte verifier
below reads and checks raw artifacts, events and observed bounds, but broader
adapters still need family semantics, runtime/environment/input isolation and an
independent equal-work assessment. A synthetic test pass is not formal timing
acceptance. The pure manifest does not prove that every implemented family was
listed.

## Complete-byte raw evidence verification

`superiority_evidence.validate_byte_batch` is a read-only verifier for deterministic
commands whose complete result can be specified by exact bytes. Supply the original
batch specification, measurement executable/script identities frozen **before**
execution, the expected SHA256 for stdout/stderr and every output or partial file,
and the complete expected directory set. Expected files may be nested; `tmp` must
be declared even when empty. Do not construct the expectations from the report's
passing flag or blindly copy its claimed output hashes.

```python
receipt = validate_byte_batch(
    batch_directory, frozen_spec, frozen_measurement_identities,
    {"stdout": expected_stdout_sha256, "stderr": expected_stderr_sha256,
     "output.bam": expected_complete_bam_sha256},
    expected_directories=("tmp",), limits=Limits(),
)
```

It checks every retained invocation, exact typed event/plan equality, fresh
PID/start identities, command/exit/initial affinity, before/after measurement
identities, all file bytes and the absence of unaccounted files/directories.
Symlinks, nonregular files and artifact hard-link aliases are rejected. Expected
nonzero CLI exits are supported: their partial files and diagnostics must be part
of the complete contract, while the batch helper itself must exit successfully.

Resource validation reparses GNU-time, rejects duplicate required metrics and
contradictory exit/signal lines, reconciles RSS and file peaks with all samples,
requires exactly one terminal sample, and checks final artifacts against the
same envelope. Duplicate JSON keys, nonfinite values and bool-for-integer
substitutions are rejected in checked identities/observations. The verifier
rehashes artifacts to detect ordinary mutation during verification. It does not
make concurrent filesystem activity atomic or authenticate telemetry.

The returned receipt includes invocation signatures, raw report and artifact
manifest hashes, observed elapsed time and a validated invocation count. It
explicitly leaves `formal_eligibility_established` false. The caller must establish
input provenance, environment/runtime coverage, complete family population, equal
work, cross-batch native determinism, calibration, schedule and acceptance. Formats
requiring semantic normalization or intentional nondeterminism need separate
validators; this helper does not discard fields to make them fit a byte contract.

Fourteen additional test methods use fresh real batches, including expected-error
partial output and adversarial mutations of raw outputs/events/resources. Review
found three acceptance holes in the first prototype; seven executable negative
controls reproduced them before the parser fixes. A scheduling-dependent test
was then made deterministic by inserting a distinct earlier sample. All 50
resource/batch/plan/evidence tests passed; original failures remain under
`target/surpass-d3d8edc/evidence-prototype1/` and `evidence-negative-controls.log`.

Sixteen synthetic protocol tests exercise these rules, including a review-found
loophole where a self-consistent manifest could omit the 2/4-core budgets. That
loophole is closed; tests explicitly reject partial-budget manifests even when
their remaining command identities are internally consistent. A separate local
control found Python equality accepted a frozen schedule block changed from `0`
to `false`; canonical JSON fingerprints now enforce type-sensitive identity.

## Verification so far

The 20 local Linux controls exercise real aggregate parent/child RSS, growing
files, headroom, timeout, observer failure, retained partial output, an orphaned
child, source-plan mutation, literal shell syntax, complete fresh-process output,
expected error exits and a deliberately slow observer. They also check hard-link
and sparse-file accounting, process-name parsing and calibration/input validation.
The physical-CPU-4 execution controls skip on hosts where CPU 4 is unavailable;
this skip is not equivalent to running them on that host.

The current suite is included by `tests/ci/check.sh` and can be run directly:

```bash
python3 -W error::ResourceWarning -m unittest discover \
  -s bench -p 'test_superiority_*.py'
```

An additional local validation executed 64 fresh `rust-bam flags 99` and 64 pinned
`samtools flags 99` processes with the final primitives. Every stdout was checked
against the independently decoded flags and every stderr was empty. Sources,
commands, resource samples and all outputs remain under
`target/surpass-d3d8edc/batch-flags-validation2/`. The earlier primitive validation
is retained separately. Neither validation was calibrated or is a performance
acceptance result.

A later short-family wiring control ran 108 fresh CLIs: nine flags/head/quickcheck
cases × all 1/2/4-core budgets × both arms × two invocations. It includes SAM/BAM,
numeric/named flags, quickcheck with one or 100 input arguments and combined
header/EOF failure masks. The 100-argument controls repeat the same valid path
(with one failing path in the mixed case); they do not establish coverage of 100
distinct files. That population is still required before formal family acceptance.
Every invocation's complete stdout/stderr/exit and absence of extra artifacts were
checked, along with raw plans/events, identities, sampled resources and native
cross-budget signatures. The pure calibration gate correctly rejected this short
population. These controls ran alongside CI and **are not timing evidence**.
Artifacts and the executed adapter prototype are in
`target/surpass-d3d8edc/short-family-controls1/`. Broader family validators and the
formal driver still need integration.

A subsequent complete-byte verifier control ran **204 fresh CLIs**: 17 cases ×
1/2/4-core budgets × native/pinned upstream × two invocations. It adds 100 distinct
BAM files (identical valid contents, different paths/inodes), mixed-file failures,
plain/gzip/BGZF SAM headers and redirected regular-file stdin, including missing
EOF. These stdin cases do not claim live-pipe coverage. Independently specified
stdout/stderr/exit expectations were frozen before execution; each raw batch was
verified, all invocation PID/start pairs checked globally, and signatures matched
across arms and CPU budgets. Receipts live outside measured batch directories, so
the complete file inventory remains intact. The receipts fed the pure calibration
module, which correctly rejected the short observations. This was correctness and
integration work on a busy host, with **no timing claim**. Inputs, expected file
hashes, the frozen manifest, scripts, 102 receipts and raw batches are retained in
`target/surpass-d3d8edc/byte-evidence-real1/`.
