#!/usr/bin/env python3
"""Sequential instruction profiles; elapsed instrumentation time is not a benchmark."""
import json
from pathlib import Path
import subprocess
import time

root = Path.cwd()
work = root / 'target/bottleneck-21bc596'
profiles = work / 'profiles'
profiles.mkdir(exist_ok=False)
commands = [
    ('stats-synthetic', [str(work/'before/rust-fx'), 'stats', '-a', '-T', '/tmp/bioinfo-readme-benchmark/synthetic-100000.fastq']),
    ('stats-real', [str(work/'before/rust-fx'), 'stats', '-a', '-T', '/tmp/bioinfo-readme-benchmark/real-100000.fastq']),
    ('flagstat', [str(work/'before/rust-bam'), 'flagstat', '/tmp/bioinfo-readme-benchmark/bam-100000/unsorted.bam']),
    ('sort', [str(work/'before/rust-bam'), 'sort', '--max-memory', '1MiB', '--threads', '1', '-o', str(profiles/'sort.bam'), '/tmp/bioinfo-readme-benchmark/bam-100000/unsorted.bam']),
]
records=[]
for name, command in commands:
    print('START',name,flush=True)
    full=['taskset','-c','4','valgrind','--tool=callgrind','--error-exitcode=97',f'--callgrind-out-file={profiles/name}.callgrind',f'--log-file={profiles/name}.valgrind']+command
    with (profiles/f'{name}.stdout').open('w') as out, (profiles/f'{name}.stderr').open('w') as err:
        started=time.monotonic()
        result=subprocess.run(full,stdout=out,stderr=err,timeout=600)
    records.append({'name':name,'command':full,'exit':result.returncode,'instrumented_elapsed_s':time.monotonic()-started})
    (profiles/'profiles.json').write_text(json.dumps(records,indent=2)+'\n')
    if result.returncode:raise SystemExit(result.returncode)
    with (profiles/f'{name}.annotation').open('w') as out:
        subprocess.run(['callgrind_annotate','--auto=no','--inclusive=no','--threshold=95',str(profiles/f'{name}.callgrind')],stdout=out,check=True)
    print('DONE',name,flush=True)
print('PROFILE COMPLETE',flush=True)
