# Larger-input and multicore benchmark protocol

This protocol compares the pinned upstream implementations with the current
native executables. Before/after optimization experiments are supplementary.
The earlier README and optimization archives remain historical measurements.

## Population and calibration

Use the existing validated 1M real FASTQ subset, 1M synthetic FASTQ subset,
and synthetic BAM fixtures as source populations. Expanded inputs repeat the
complete source population an integer number of times. Record that multiplicity,
the source hashes, output hashes, exact record/base counts, and input paths.
These are processing workloads with repeated names and sequences, not additional
biological samples or newly observed distinct sequences. In particular, repeated
QC inputs do not exercise growth in distinct-sequence state. Retain both real and
synthetic FASTQ results; do not silently substitute one for the other.

Measure cutadapt plain and gzip-6 output, extended stats, FASTQ-to-FASTA, lint,
BAM flagstat, BAM sort, UMI deduplication, and descriptive FastQC reporting.
Use CPU budgets of 1, 2, and 4 independent physical P cores. Workload/core
combinations and generated input sizes are frozen in a plan before formal runs.
Calibration measures both implementations under all applicable core budgets.
Choose an integer source multiplicity targeting 15 seconds for the fastest
calibration observation. Every observation in the final calibration must reach
10 seconds before freezing. Use the same input size across core budgets for a
workload. If a
formal condition is still shorter than 10 seconds, retain it and mark the size
criterion unmet; any larger follow-up is an additional recorded experiment.

Three measured blocks and one warm-up block per condition balance the two arm
orders twice each across all four blocks. Shuffle that multiset with a recorded
seed and retain the complete plan. These are timing repeats, not biological
replicates. Report medians and observed min–max ranges, not confidence intervals.
Compute the upstream/current ratio within each measured block, then report its
median. Do not average ratios across workloads.

## CPU and storage controls

Pin the complete process tree to CPU 4, CPUs 4/6, or CPUs 0/2/4/6 after verifying
the topology. The reported budget is the number of available physical cores,
not a claim about application worker counts. Record every exact command and its
thread settings. Set supported concurrency options; do not invent native
parallel execution for flags that are currently unused. In particular, FX's
accepted `-j` remains unused, flagstat retains serial full validation, and native
FastQC analyzes one input serially. Native BAM sort parallelizes final BGZF
compression; sorting and intermediate runs remain serial. Report process CPU
time divided by wall time to show whether the additional cores were used.

Place enlarged inputs, outputs, and temporary files under a directory on the
local NVMe filesystem, not `/tmp` tmpfs. Record filesystem/mount information and
available capacity. Preflight the entire input population, both arm outputs,
a retained first native output for determinism checks, sort runs, and at least
10 GiB of headroom. FASTQ-producing cases reserve 3.1× input bytes; BAM sort/UMI
reserve 8× decoded input bytes. These are conservative space estimates, not
observed peaks. Hash-verified populations from a completed preparation may be
reused when source metadata and multiplicity match; existing bytes are recorded
without reserving them again as new allocations. Flush generated files before
calibration or timing.
Condition each input with a sequential read before a run, but do not call this
fully cache-resident: large files and outputs can exceed available page cache.
Do not drop system caches or change privileged system settings.

Time fresh CLI processes, including startup and ordinary file I/O. Flush each
output file after the CLI exits and before the next trial, recording this
untimed duration separately. CLI elapsed time does not claim durable storage
completion. Correctness scans, output transfer/removal, compilation, generation,
and oracle suites must not overlap accepted timing. Record CPU topology,
executable/source/runtime hashes, temperatures, frequency observations, load,
RSS, CPU time, filesystem I/O, and sampled temporary storage. Thermal/load/core
frequency snapshots are taken before/after and approximately once per second;
shorter peaks can be missed. GNU time's peak RSS is not the sum of simultaneous
child-process RSS. Shared SMT siblings, unfixed frequencies and thermal variation
remain limitations. On this host CPU4/6 have a 5.0 GHz maximum and CPU0/2 a 4.8 GHz
maximum, so core budgets also differ in their possible turbo ceilings.

The final calibration and formal run must use matching native executables,
upstream payloads and entrypoints, benchmark Python/GNU time/taskset binaries,
input manifests, and executed measurement scripts. Archive the executed scripts
and hashes. Validation subprocesses use the same selected runtime environment.
A preliminary calibration with older instrumentation remains a sizing record;
it cannot certify a later formal run.

## Correctness and reporting

Require zero exit codes and verification of every accepted output. Compare
trimmed FASTQ, stats TSV, FASTA and flagstat output exactly (decode gzip first).
Lint requires independent valid-input checks and invalid-tail rejection. Sorting
requires full record/count/order validation, BGZF finalization, and upstream
comparison with only declared header/tie normalization. Preserve the native
thread-count determinism checks. Compare each new native signature before
removing its payload; keep the first native payload until all blocks and core
budgets for that workload match. A mismatch retains both the baseline and the
failing pair. The 64 KiB internal BAM frame threshold limits the added captured
copy; vector capacity may retain up to roughly twice that threshold, and each
open decoder retains its largest record-body buffer separately. This is not a
64 KiB total-memory bound. UMI representatives must agree with the
independent expected set for the expanded, coordinate-sorted input; repeated
source names and the changed bundle multiplicities must be explicit.

FastQC remains descriptive: retain all raw table differences and independently
check counts, displayed bases and lengths. Missing native graphics and unresolved
GC/duplication differences exclude an equal-work upstream speed ratio. Do not
present those differences as approved corrections.

The primary bilingual README table shows input scale, CPU budget, upstream time,
current time and, where eligible, the paired ratio. Include slower cases and
actual resource limits. Keep before/after figures in supporting evidence. Retain
failed or interrupted runs and their reasons. A complete final report must have
every planned arm/block/condition and passing required checks; reject partial
reports rather than summarize only their successful rows.
