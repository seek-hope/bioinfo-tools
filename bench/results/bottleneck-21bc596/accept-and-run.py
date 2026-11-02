#!/usr/bin/env python3
"""Accept the serial candidate, prepare fresh inputs, calibrate, then measure."""
import json
import os
from pathlib import Path
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / 'target/bottleneck-21bc596'
ORACLES = Path('/tmp/bioinfo-readme-repro-oracles')
CUTADAPT = Path('/home/rimuru/.conda/envs/ta-cutadapt/bin/cutadapt')
os.chdir(ROOT)
sys.path.insert(0, str(ROOT/'bench'))
from scale_common import validate_complete

steps = []

def run(name, command):
    started = datetime.now(timezone.utc).isoformat()
    print(name, 'START', started, flush=True)
    with (WORK/f'{name}.log').open('x') as log:
        process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
        step = {'name': name, 'command': command, 'pid': process.pid, 'started_utc': started}
        steps.append(step)
        (WORK/'pipeline-steps.json').write_text(json.dumps(steps, indent=2)+'\n')
        code = process.wait()
    step.update(returncode=code, completed_utc=datetime.now(timezone.utc).isoformat())
    (WORK/'pipeline-steps.json').write_text(json.dumps(steps, indent=2)+'\n')
    (WORK/f'{name}.status').write_text(f'{code}\n')
    print(name, 'END', code, step['completed_utc'], flush=True)
    if code:
        raise SystemExit(code)

run('accept-base', ['bash', 'tests/ci/check.sh'])
run('accept-oracles', ['bash', 'tests/ci/check.sh', '--with-fastx-qc-oracles'])
run('seal', ['python3', 'bench/scale_seal.py', '--outdir', str(WORK/'sealed')])
run('sealed-fx-behavior', ['python3', 'bench/check_optimization.py', '--before', str(WORK/'before/rust-fx'),
    '--after', str(WORK/'sealed/rust-fx'), '--out', str(WORK/'sealed-fx-behavior.json')])
run('sealed-fx-oracle', ['bash', 'tests/oracle/run_w13_oracle.sh', '--rust', str(WORK/'sealed/rust-fx')])
run('sealed-bam-oracle', ['python3', 'tests/oracle/w14_diff.py', '--rust', str(WORK/'sealed/rust-bam'),
    '--samtools', str(ORACLES/'bin/samtools'), '--json', str(WORK/'sealed-bam-oracle.json')])

plan = WORK/'initial-plan.json'
reuse = [ROOT/'target/scale-benchmark/inputs-2/inputs.json']
base = ['python3', 'bench/scale_benchmark.py', '--native-dir', str(WORK/'sealed'),
    '--canonicalizer', str(WORK/'sealed/canonical-sam'), '--oracle-dir', str(ORACLES/'bin'),
    '--toolchain-cache', str(ORACLES), '--cutadapt', str(CUTADAPT)]
for index in range(4):
    inputs = WORK/f'inputs-{index}'
    run(f'prepare-{index}', ['python3', 'bench/scale_inputs.py', '--plan', str(plan),
        '--sources', '/tmp/bioinfo-readme-benchmark/inputs.json',
        '--samtools', str(ORACLES/'bin/samtools'), '--reuse-manifest', *map(str, reuse),
        '--outdir', str(inputs)])
    reuse.append(inputs/'inputs.json')
    calibration = WORK/f'calibration-{index}'
    run(f'calibration-{index}', [*base, '--calibration', '--plan', str(plan),
        '--inputs', str(inputs/'inputs.json'), '--outdir', str(calibration)])
    report = json.loads((calibration/'report.json').read_text())
    validate_complete(report, formal=False)
    minimum = min(o['elapsed_s'] for row in report['results'] for o in row['observations'])
    print('CALIBRATION MINIMUM', minimum, flush=True)
    if minimum >= report['plan']['minimum_desired_seconds']:
        break
    plan = WORK/f'enlarged-plan-{index+1}.json'
    run(f'propose-{index+1}', ['python3', 'bench/scale_plan.py', '--calibration', str(calibration/'report.json'),
        '--out', str(plan)])
else:
    raise RuntimeError('four calibration rounds did not qualify')

formal_plan = WORK/'formal-plan.json'
run('freeze', ['python3', 'bench/scale_plan.py', '--calibration', str(calibration/'report.json'),
    '--freeze', '--out', str(formal_plan)])
run('formal', [*base, '--plan', str(formal_plan), '--inputs', str(inputs/'inputs.json'),
    '--outdir', str(WORK/'formal')])
run('summary', ['python3', 'bench/summarize_scale.py', str(WORK/'formal/report.json'),
    '--outdir', str(WORK/'formal-summary')])
print('PIPELINE COMPLETE', flush=True)
