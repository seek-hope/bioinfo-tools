# Completed statistics and merge-selection experiment

This archive records the optimization batch based on
`21bc59628b3d0ecfc5f32d8218273b1ece992b71`. The current source changes are in
`native-source.patch` and `final-source/`; the latter also includes the integration
test file, which the native build seal's source glob does not cover.

The formal run completed on 2026-11-02 from 03:02:52.762541 to 04:35:02.085050 UTC:
9 conditions, 54 measured observations, 18 warm-ups and 36 paired output checks.
All passed. The minimum measured duration was 19.579817 s; no measured invocation
was below 10 s. Input/runtime/source identities, native determinism and independent
summary recomputation passed. See [the report](../../BOTTLENECK.md).

## Evidence map

| Evidence | Location |
| --- | --- |
| Original baseline identity | `baseline.json`, `before-source/` |
| Complexity and decision record | `PLAN.md`, `analysis.md`, `review-notes.md` |
| Baseline/current instruction profiles | `profiles/`, `profiles-candidate-1/`, `profile-audit.json` |
| Accepted serial candidate | `candidate-1/`, `candidate-1-source/`, `screen-1/` |
| Failed parallel candidate build | `candidate-2-tests.log`, `candidate-2-checks.json` |
| Corrected but rejected parallel candidate | `candidate-2b/`, `candidate-2b-source/`, `screen-2/` |
| Final source and build | `final-source/`, `final-source-sha256.json`, `native-source.patch`, `sealed/` |
| Fresh acceptance | `accept-base.log`, `accept-oracles.log`, `sealed-*-oracle.log`, `sealed-fx-behavior.json` |
| Input preparation/reuse | `inputs-0/inputs.json`, `source-inputs*.json`, `reused-input-manifest.json` |
| Qualifying calibration | `calibration-0/`, `initial-plan.json` |
| Frozen formal population | `formal-plan.json`, `formal/report.json`, `formal/<condition>/<block>-<arm>/` |
| Summaries and independent audit | `formal-summary/`, `formal-audit.json`, `formal-audit.log` |
| Executed orchestration | `accept-and-run.py`, `pipeline-steps.json`, `pipeline.log`, `*.status` |
| Measurement/test support snapshots | `formal/executed/`, `calibration-0/executed/`, `support/` |
| Unexecuted follow-up lead | `next-merge-schedule-static-review.md` |

Both screens contain all 32 invocations, including warm-ups. Screen 1 compares
21bc596 with the serial candidate; screen 2 compares that serial candidate with
the rejected parallel prototype. Short screening times are not calibrated speedup
claims. The rejected prototype's source, build failure, successful corrected tests
and all observations remain distinct from the accepted implementation. Final
sealed executables match the profiled serial candidate byte for byte.

Raw reports preserve original absolute paths. Executed scripts and audits describe
the original checkout/work directory; they are not portable replay entrypoints
for this archive. Large reproducible FASTQ/BAM payloads and executable binaries
are omitted, with source/input/executable hashes and build/generation recipes
retained. Reused input manifests refer to earlier preparation directories; those
payload dependencies were not deleted or overwritten. The preceding
[scale archive](../scale-7a7ba28/README.md) is unchanged.

For a new experiment use the [reproduction guide](../../SCALE-REPRODUCTION.md),
select the three workloads, review affinities and recalibrate fresh binaries and
inputs. Source or measurement-script changes invalidate the old calibration.
The static merge-scheduling note is an unexecuted lead, not a delivered feature.

## Integrity

`sha256.json` covers every archive file except itself. From the repository root:

```bash
python3 - <<'PY'
import hashlib
import json
from pathlib import Path
root = Path('bench/results/bottleneck-21bc596')
manifest = json.loads((root / 'sha256.json').read_text())
actual = {str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()
          and p != root / 'sha256.json'}
assert actual == set(manifest['files'])
for name, expected in manifest['files'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected, name
print('Archive SHA-256 coverage and contents passed:', len(actual), 'files')
PY
```

The manifest is byte-integrity evidence, not a release signature. The archive
contains no release, tag or package publication approval. Remote CI is checked
against the delivery commit separately; these local oracle logs do not imply that
an optional remote oracle job ran.
