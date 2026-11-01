# Static review before expanded calibration

Read-only helper reviewed scale driver/common/input/proposal/validation/summary
scripts. It ran no tests and did not review the sealed native implementation.

Findings and resolution:

1. Missing `import sys` after adding benchmark-interpreter provenance: fixed
   before any invocation of that driver revision.
2. Stock CI wrappers resolve their toolchain from ambient TMPDIR. Driver now
   installs its selected TMPDIR in both os.environ and the child environment,
   so validators using ambient subprocess settings resolve the same cache.
   The actual base-calibration entrypoints were staged absolute-path wrappers;
   their `umi` and `fastqc` directories are symlinks to /tmp/bioinfo-ci-toolchain.
   Thus review did not establish an actual base-calibration decoder mismatch.
3. Added the executed UMI console entrypoint and any sibling `_toolchain`
   scripts to version/identity checks and execution snapshots. Their hashes
   must also match between final calibration and formal execution.
4. Native determinism was checked after deleting compared payloads. Driver now
   validates each pair's evidence immediately, compares the native signature
   before deletion, and retains the first native payload until every CPU budget
   for its workload agrees. A mismatch preserves both anchor and failing pair.
   FASTQ output reservation consequently increased from2.1x to3.1x input size.

Additional self-check: injected `records=-1` from a canonicalizer was accepted
by the standalone transport helper, although the BAM population comparison
would reject it. `validator-controls-initial.log` retains the failing control;
the count parser now accepts only a nonempty ASCII decimal count followed by
one LF. All five transport/timeout/EOF/count controls pass after the change.

Whole-runner smoke and the CI gates still need execution after these edits.
