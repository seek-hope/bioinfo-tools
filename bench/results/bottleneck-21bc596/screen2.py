#!/usr/bin/env python3
"""Short screening only; not a calibrated or publishable speed comparison."""
import hashlib
import json
import os
from pathlib import Path
import random
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / 'target/bottleneck-21bc596'
OUT = WORK / 'screen-2'
OUT.mkdir(exist_ok=False)
os.chdir(ROOT)
ENV = dict(os.environ, LC_ALL='C', TMPDIR=str(OUT), RUST_CUTADAPT_COMPRESSION_LEVEL='6')
ARMS = {'before': WORK/'candidate-1', 'current': WORK/'candidate-2b'}
SOURCES = Path('/tmp/bioinfo-readme-benchmark')
CASES = [
    ('sort-c1', 'rust-bam', ['sort', '--max-memory', '512MiB', '--threads', '1'], SOURCES/'bam-10000000/unsorted.bam', '4', True),
    ('sort-c2', 'rust-bam', ['sort', '--max-memory', '512MiB', '--threads', '2'], SOURCES/'bam-10000000/unsorted.bam', '4,6', True),
    ('sort-c4', 'rust-bam', ['sort', '--max-memory', '512MiB', '--threads', '4'], SOURCES/'bam-10000000/unsorted.bam', '0,2,4,6', True),
    ('sort-small-runs-c4', 'rust-bam', ['sort', '--max-memory', '1MiB', '--threads', '4'], SOURCES/'bam-100000/unsorted.bam', '0,2,4,6', True),
]

def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(8*1024*1024), b''):
            h.update(chunk)
    return h.hexdigest()

def condition(path):
    with path.open('rb') as stream:
        while stream.read(8*1024*1024):
            pass

report = {'kind': 'exploratory-screening', 'seed': 20261102, 'passed': False,
          'inputs': {str(c[3]): digest(c[3]) for c in CASES},
          'binaries': {str(folder/name): digest(folder/name) for folder in ARMS.values() for name in ['rust-bam', 'rust-fx']},
          'observations': []}

def save():
    (OUT/'report.json').write_text(json.dumps(report, indent=2)+'\n')

save()
try:
    rng = random.Random(report['seed'])
    for name, binary, arguments, source, cpus, bam in CASES:
        orders = [('before', 'current'), ('current', 'before')]*2
        rng.shuffle(orders)
        expected = None
        for block, order in zip([-1, 0, 1, 2], orders):
            for arm in order:
                directory = OUT/f'{name}-{block}-{arm}'
                directory.mkdir()
                output = directory/('out.bam' if bam else 'stdout')
                command = [str(ARMS[arm]/binary), *arguments]
                if bam:
                    command += ['-o', str(output)]
                command.append(str(source))
                timed = ['taskset', '-c', cpus, '/usr/bin/time', '-v', '-o', str(directory/'time.txt'), *command]
                condition(source)
                time.sleep(2)
                print('START', name, block, arm, flush=True)
                with (directory/'stdout').open('wb') as stdout, (directory/'stderr').open('wb') as stderr:
                    start = time.monotonic()
                    result = subprocess.run(timed, stdout=stdout, stderr=stderr, env=ENV, timeout=600)
                    elapsed = time.monotonic()-start
                sync_start = time.monotonic()
                if output.exists():
                    with output.open('rb') as stream:
                        os.fsync(stream.fileno())
                sync_seconds = time.monotonic()-sync_start
                metrics = (directory/'time.txt').read_text()
                user = float(re.search(r'User time \(seconds\): ([0-9.]+)', metrics)[1])
                system = float(re.search(r'System time \(seconds\): ([0-9.]+)', metrics)[1])
                rss = int(re.search(r'Maximum resident set size \(kbytes\): (\d+)', metrics)[1])
                output_sha = digest(output) if output.exists() else None
                observation = {'case': name, 'block': block, 'arm': arm, 'command': timed,
                    'affinity': cpus, 'returncode': result.returncode, 'elapsed_seconds': elapsed,
                    'user_seconds': user, 'system_seconds': system, 'max_rss_kib': rss,
                    'untimed_fsync_seconds': sync_seconds, 'output_sha256': output_sha}
                report['observations'].append(observation)
                save()
                assert result.returncode == 0, observation
                if expected is None:
                    expected = output_sha
                assert output_sha == expected, observation
                print('DONE', name, block, arm, round(elapsed, 6), flush=True)
    assert len(report['observations']) == 32
    assert all(digest(Path(path)) == sha for path, sha in report['inputs'].items())
    assert all(digest(Path(path)) == sha for path, sha in report['binaries'].items())
    report['passed'] = True
except BaseException as error:
    report['error'] = repr(error)
    raise
finally:
    save()
print('SCREENING COMPLETE; OUTPUTS IDENTICAL', flush=True)
