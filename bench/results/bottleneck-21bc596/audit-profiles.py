#!/usr/bin/env python3
"""Audit instruction-profile population, output equality, and candidate snapshots."""
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT/'target/bottleneck-21bc596'

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(8*1024*1024), b''):
            h.update(chunk)
    return h.hexdigest()

report = {'passed': False, 'kind': 'callgrind-instruction-audit', 'cases': [], 'source_checks': {}}
for candidate in ['candidate-1', 'candidate-2b']:
    provenance = json.loads((WORK/candidate/'build-provenance.json').read_text())
    assert provenance['passed'] is True
    for name, expected in provenance['source_sha256'].items():
        saved = WORK/f'{candidate}-source'/name
        source = saved if saved.exists() else ROOT/name
        assert sha(source) == expected, (candidate, name)
    report['source_checks'][candidate] = len(provenance['source_sha256'])

folders = {'before': WORK/'profiles', 'current': WORK/'profiles-candidate-1'}
records = {arm: json.loads((folder/'profiles.json').read_text()) for arm, folder in folders.items()}
assert all(len(rows) == 4 and {r['name'] for r in rows} == {'stats-real', 'stats-synthetic', 'flagstat', 'sort'}
           and all(r['exit'] == 0 for r in rows) for rows in records.values())
for name in ['stats-real', 'stats-synthetic', 'flagstat', 'sort']:
    values = {}
    for arm, folder in folders.items():
        record = next(r for r in records[arm] if r['name'] == name)
        source = Path(record['command'][-1])
        output = folder/('sort.bam' if name == 'sort' else name+'.stdout')
        raw = (folder/f'{name}.callgrind').read_text()
        matches = re.findall(r'^summary: (\d+)$', raw, re.M)
        assert len(matches) == 1
        values[arm] = {'instructions': int(matches[0]), 'input_sha256': sha(source),
                       'output_sha256': sha(output), 'callgrind_sha256': sha(folder/f'{name}.callgrind'),
                       'command': record['command']}
    assert values['before']['input_sha256'] == values['current']['input_sha256']
    assert values['before']['output_sha256'] == values['current']['output_sha256']
    report['cases'].append({'name': name, 'observations': values,
        'instruction_reduction_fraction': 1 - values['current']['instructions']/values['before']['instructions']})
report['passed'] = True
with (WORK/'profile-audit.json').open('x') as stream:
    json.dump(report, stream, indent=2)
    stream.write('\n')
print('PROFILE AUDIT PASS: four matching inputs/outputs; candidate source snapshots123/124 hashes verified')
