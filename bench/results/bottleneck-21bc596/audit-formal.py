#!/usr/bin/env python3
"""Independent completed-run audit; run only after the measurement pipeline exits."""
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT/'target/bottleneck-21bc596'
sys.path.insert(0, str(ROOT/'bench'))
from scale_common import validate_complete


def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(8*1024*1024), b''):
            h.update(chunk)
    return h.hexdigest()

report = json.loads((WORK/'formal/report.json').read_text())
summary = json.loads((WORK/'formal-summary/summary.json').read_text())
validate_complete(report)
for name in ['accept-base', 'accept-oracles', 'seal', 'sealed-fx-behavior', 'sealed-fx-oracle',
             'sealed-bam-oracle', 'formal', 'summary']:
    assert (WORK/f'{name}.status').read_text().strip() == '0', name
assert 'PIPELINE COMPLETE' in (WORK/'pipeline.log').read_text()
assert {w['id'] for w in report['plan']['workloads']} == {'real-stats', 'synthetic-stats', 'bam-sort'}
assert all(w['cpu_budgets'] == [1, 2, 4] for w in report['plan']['workloads'])
assert sha(report['plan']['calibration_report']) == report['plan']['calibration_report_sha256']
by_id = {row['condition']: row for row in summary}
assert len(by_id) == len(summary) == len(report['results']) == 9
blocks = Counter()
measured = []
for row in report['results']:
    observations = row['observations']
    result = by_id[row['condition']['id']]
    assert len(observations) == 8 and len(row['checks']) == 4
    blocks.update(o['block'] for o in observations)
    for arm in ['current', 'upstream']:
        selected = [o for o in observations if o['arm'] == arm and o['block'] >= 0]
        assert len(selected) == 3
        values = sorted(o['elapsed_s'] for o in selected)
        assert math.isclose(result['arms'][arm]['median_s'], values[1])
        assert math.isclose(result['arms'][arm]['min_s'], values[0])
        assert math.isclose(result['arms'][arm]['max_s'], values[2])
        measured.extend(selected)
    ratios = []
    for block in range(3):
        pair = {o['arm']: o['elapsed_s'] for o in observations if o['block'] == block}
        assert set(pair) == {'current', 'upstream'}
        ratios.append(pair['upstream']/pair['current'])
    assert math.isclose(result['paired_upstream_over_current']['median'], sorted(ratios)[1])
assert blocks == {-1: 18, 0: 18, 1: 18, 2: 18}
assert len(measured) == 54
for path, expected in report['identities'].items():
    assert sha(path) == expected, path
assert all(report[k] for k in ['inputs_unchanged', 'runtime_payloads_unchanged', 'identities_unchanged'])

profiled = json.loads((WORK/'candidate-1/build-provenance.json').read_text())
sealed = json.loads((WORK/'sealed/build-provenance.json').read_text())
for name, metadata in sealed['executables'].items():
    assert metadata['sha256'] == profiled['executables'][name]['sha256'], name
    assert sha(metadata['path']) == metadata['sha256'], name

initial_routes = json.loads((WORK/'runtime-routes-during-formal.json').read_text())
routes = {'queried_utc': datetime.now(timezone.utc).isoformat(), 'routes': {}}
for name, expected in initial_routes['routes'].items():
    staged = Path(expected['staged'])
    resolved = str(staged.resolve(strict=True))
    assert resolved == expected['resolved']
    assert staged.samefile(expected['selected_cache'])
    routes['routes'][name] = dict(expected, resolved=resolved, same_directory=True)
routes['matches_midrun_sample'] = True
with (WORK/'runtime-routes-after-formal.json').open('x') as stream:
    json.dump(routes, stream, indent=2)
    stream.write('\n')

audit = {'passed': True, 'conditions': 9, 'measured': 54, 'warmups': 18,
    'pairs': sum(len(row['checks']) for row in report['results']),
    'minimum_formal_seconds': min(o['elapsed_s'] for o in measured),
    'below_10_seconds': sum(o['elapsed_s'] < 10 for o in measured),
    'independent_medians_ranges_and_paired_ratios': True,
    'identity_files_rehashed': len(report['identities']),
    'profiled_and_final_binaries_identical': True, 'runtime_routes_match_midrun_sample': True,
    'started_utc': report['started_utc'], 'completed_utc': report['completed_utc']}
with (WORK/'formal-audit.json').open('x') as stream:
    json.dump(audit, stream, indent=2)
    stream.write('\n')
print('PASS independent audit: 9 timing medians/ranges, 9 paired ratios, 72 invocations, 36 checks')
print('PASS final identities:', len(report['identities']), 'files rehashed')
print('Minimum measured seconds:', audit['minimum_formal_seconds'], '; below 10s:', audit['below_10_seconds'])
print('PASS profiled/final executable identity and final runtime routes')
