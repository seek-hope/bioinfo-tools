#!/usr/bin/env python3
"""Executable pinned-version checks for the two README-pilot compatibility fixes."""
import argparse
import json
from pathlib import Path
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--rust', type=Path, default=Path('target/release/rust-fx'))
    p.add_argument('--seqkit', type=Path, default=Path('tests/ci/env/bin/seqkit'))
    p.add_argument('--out', type=Path, required=True)
    args = p.parse_args()
    version = subprocess.check_output([str(args.seqkit), 'version'], text=True).strip()
    assert version == 'seqkit v2.14.0', version
    cases = []
    for encoding, base in [('sanger',33), ('illumina-1.3+',64)]:
        for score in [19,20,21,29,30,31]:
            command = ['stats','-a','-T','-E',encoding,'-']
            data = b'@read\nACGT\n+\n' + bytes([base + score])*4 + b'\n'
            observations = {}
            correct = True
            for name, exe in [('rust',args.rust),('upstream',args.seqkit)]:
                r = subprocess.run([str(exe),*command],input=data,capture_output=True)
                lines = r.stdout.decode().splitlines()
                row = dict(zip(lines[0].split('\t'),lines[1].split('\t'))) if len(lines)==2 else {}
                good = r.returncode == 0 and row.get('Q20(%)') == ('100' if score>=20 else '0') and row.get('Q30(%)') == ('100' if score>=30 else '0')
                correct &= good
                observations[name] = {'exit':r.returncode,'stdout':r.stdout.decode(),'stderr':r.stderr.decode(),'independent_threshold_check':good}
            cases.append({'case':f'{encoding}-Q{score}','command':command,'passed':correct,'observations':observations})
    sequence = b'ACGT'*38
    for kind in ['fastq','fasta']:
        data = b'@read description\n'+sequence+b'\n+\n'+b'I'*len(sequence)+b'\n' if kind=='fastq' else b'>read description\n'+sequence+b'\n'
        for width in [None,0,1,60,80]:
            command = ['fq2fa'] + ([] if width is None else ['-w',str(width)]) + ['-']
            chunk = width or len(sequence)
            expected = b'>read description\n'+b''.join(sequence[i:i+chunk]+b'\n' for i in range(0,len(sequence),chunk))
            observations = {}; correct = True
            for name, exe in [('rust',args.rust),('upstream',args.seqkit)]:
                r = subprocess.run([str(exe),*command],input=data,capture_output=True)
                good = r.returncode == 0 and r.stdout == expected
                correct &= good
                observations[name] = {'exit':r.returncode,'stdout':r.stdout.decode(),'stderr':r.stderr.decode(),'independent_format_check':good}
            cases.append({'case':f'{kind}-width-{width}','command':command,'passed':correct,'observations':observations})
    args.out.write_text(json.dumps({'upstream':version,'rust':str(args.rust),'cases':cases},indent=2)+'\n')
    print(f'README boundary oracle: {sum(c["passed"] for c in cases)}/{len(cases)}')
    return 0 if all(c['passed'] for c in cases) else 1


if __name__=='__main__':
    raise SystemExit(main())
