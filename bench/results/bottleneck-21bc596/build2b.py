import json
import os
from pathlib import Path
import subprocess
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / 'target/bottleneck-21bc596'
os.chdir(ROOT)
steps = [
    ('candidate-2b-tests', ['cargo', 'test', '--release', '--locked', '-p', 'rust-fx', '-p', 'rust-bam']),
    ('candidate-2b-clippy', ['cargo', 'clippy', '--release', '--locked', '-p', 'rust-bam', '-p', 'rust-fx', '--all-targets', '--', '-D', 'warnings']),
    ('candidate-2b-seal', ['python3', 'bench/scale_seal.py', '--outdir', str(WORK / 'candidate-2b')]),
]
results = []
for name, command in steps:
    print(name, 'START', datetime.now(timezone.utc).isoformat(), flush=True)
    with (WORK / f'{name}.log').open('x') as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    (WORK / f'{name}.status').write_text(f'{result.returncode}\n')
    results.append({'name': name, 'command': command, 'status': result.returncode})
    (WORK / 'candidate-2b-checks.json').write_text(json.dumps(results, indent=2) + '\n')
    print(name, 'END', result.returncode, datetime.now(timezone.utc).isoformat(), flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
print('CANDIDATE READY', flush=True)
