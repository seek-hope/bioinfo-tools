#!/usr/bin/env python3
"""Sequential acceptance, smoke, adaptive calibration, then complete formal run."""
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT/'target/scale-benchmark'

def run(name, command):
    print(name, 'START', time.strftime('%Y-%m-%dT%H:%M:%S%z'), flush=True)
    with (WORK/f'{name}.log').open('x') as log:
        result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    (WORK/f'{name}.status').write_text(str(result.returncode)+'\n')
    print(name, 'EXIT', result.returncode, flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)

common = ['--native-dir', str(WORK/'sealed'), '--canonicalizer', str(WORK/'sealed/canonical-sam'),
          '--oracle-dir', '/tmp/bioinfo-readme-repro-oracles/bin',
          '--toolchain-cache', '/tmp/bioinfo-ci-toolchain',
          '--cutadapt', '/home/rimuru/.conda/envs/ta-cutadapt/bin/cutadapt']
generator = [sys.executable, 'bench/scale_inputs.py', '--sources', '/tmp/bioinfo-readme-benchmark/inputs.json',
             '--samtools', '/tmp/bioinfo-readme-repro-oracles/bin/samtools']

for name in ('test_scale_benchmark', 'test_scale_inputs', 'test_scale_validation', 'test_canonical_sam'):
    run('accept-'+name, [sys.executable, '-W', 'error::ResourceWarning', f'bench/{name}.py'])
run('accept-bam-inputs', [sys.executable, '-W', 'error::ResourceWarning', 'bench/test_scale_bam_inputs.py',
                        '--samtools', '/tmp/bioinfo-readme-repro-oracles/bin/samtools'])
run('accept-gate-controls', [sys.executable, 'tests/ci/test_check.py'])
run('accept-base', ['bash', 'tests/ci/check.sh'])
run('accept-oracles', ['bash', 'tests/ci/check.sh', '--with-fastx-qc-oracles'])
run('smoke-inputs', generator + ['--plan', str(WORK/'base-plan.json'), '--outdir', str(WORK/'smoke-inputs')])
run('smoke-calibration', [sys.executable, 'bench/scale_benchmark.py', '--calibration',
    '--plan', str(WORK/'base-plan.json'), '--inputs', str(WORK/'smoke-inputs/inputs.json'),
    '--outdir', str(WORK/'smoke-calibration'), *common])

plan = WORK/'enlarged-plan.json'
inputs = WORK/'enlarged-inputs/inputs.json'
for iteration in range(1, 7):
    name = f'calibration-{iteration}'
    report = WORK/name/'report.json'
    run(name, [sys.executable, 'bench/scale_benchmark.py', '--calibration', '--plan', str(plan),
              '--inputs', str(inputs), '--outdir', str(WORK/name), *common])
    data = json.loads(report.read_text())
    minimum = min(o['elapsed_s'] for row in data['results'] for o in row['observations'])
    if minimum >= data['plan']['minimum_desired_seconds']:
        frozen = WORK/'formal-plan.json'
        run('freeze', [sys.executable, 'bench/scale_plan.py', '--freeze', '--calibration', str(report), '--out', str(frozen)])
        run('formal', [sys.executable, 'bench/scale_benchmark.py', '--plan', str(frozen),
                     '--inputs', str(inputs), '--outdir', str(WORK/'formal'), *common])
        run('formal-summary', [sys.executable, 'bench/summarize_scale.py', str(WORK/'formal/report.json'),
                              '--outdir', str(WORK/'formal-summary')])
        break
    next_plan = WORK/f'plan-{iteration+1}.json'
    next_inputs = WORK/f'inputs-{iteration+1}'
    run(f'propose-{iteration+1}', [sys.executable, 'bench/scale_plan.py', '--calibration', str(report), '--out', str(next_plan)])
    run(f'prepare-{iteration+1}', generator + ['--plan', str(next_plan), '--outdir', str(next_inputs),
                                            '--reuse-manifest', str(inputs)])
    plan, inputs = next_plan, next_inputs/'inputs.json'
else:
    raise SystemExit('six complete calibration iterations did not meet the duration minimum')
print('PIPELINE COMPLETE', flush=True)
