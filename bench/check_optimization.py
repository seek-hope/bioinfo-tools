#!/usr/bin/env python3
"""Before/after checks for record reuse, parsing failures and lint diagnostics."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists():
        parser.error('refusing to overwrite evidence')
    valid = b'@first description\nACGT\n+first\n!5?I\n'
    cases = {'empty': b'', 'valid': valid, 'duplicates': valid * 2,
             'empty-read': b'@empty\n\n+\n\n',
             'mixed-lengths': valid + b'@empty\n\n+\n\n@short\nC\n+\nI',
             'crlf': (valid * 2).replace(b'\n', b'\r\n'),
             'padding': b'\n\n' + valid + b'\n\n',
             'middle-blank': valid + b'\n' + valid,
             'bad-plus': valid + b'@bad\nAA\nwrong\nII\n',
             'bad-next-header': valid + b'bad\nAA\n+\nII\n',
             'unequal': valid + b'@bad\nAA\n+\nI\n',
             'extra-quality': valid + b'I\n',
             'fasta': b'>long name\nACGT\nNN\n>empty\n>short\nA\n',
             'non-utf8-name': b'@r\xff\nACGT\n+\nIIII\n',
             'quality-bytes': b'@q\n' + b'A' * 10 + b'\n+\n' + bytes([0, 1, 32, 33, 34, 63, 64, 127, 128, 255]) + b'\n'}
    for length in (65535, 65536, 65537, 70000):
        cases[f'long-{length}'] = b'@long\n' + b'A' * length + b'\n+\n' + b'I' * length + b'\n' + valid
    for index, tail in enumerate((b'@tail', b'@tail\nAA', b'@tail\nAA\n+', b'@tail\nAA\n+\nI')):
        cases[f'truncated-{index}'] = valid + tail
    compressed = gzip.compress(valid * 4, mtime=0)
    cases['gzip'] = compressed
    cases['gzip-truncated'] = compressed[:-6]
    corrupt = bytearray(compressed)
    corrupt[-8] ^= 1
    cases['gzip-corrupt'] = bytes(corrupt)
    commands = [['stats', '-a', '-T'], ['fq2fa'], ['fq2fa', '-w', '7'], ['lint']]
    report = {'before_sha256': sha(args.before.read_bytes()), 'after_sha256': sha(args.after.read_bytes()), 'cases': []}
    with tempfile.TemporaryDirectory(prefix='bioinfo-optimization-check-') as temporary:
        for name, data in cases.items():
            path = Path(temporary) / (name + ('.fastq.gz' if name.startswith('gzip') else '.fastq'))
            path.write_bytes(data)
            for command in commands:
                observations = {}
                for side, executable in [('before', args.before), ('after', args.after)]:
                    result = subprocess.run([str(executable.resolve()), *command, str(path)], capture_output=True, timeout=30)
                    stdout, stderr = result.stdout, result.stderr
                    if command[0] == 'lint':
                        pattern = rb'\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?'
                        stdout = re.sub(pattern, b'<timestamp>', stdout)
                        stderr = re.sub(pattern, b'<timestamp>', stderr)
                    observations[side] = {'exit': result.returncode, 'stdout_sha256': sha(stdout), 'stderr_sha256': sha(stderr)}
                report['cases'].append({'name': name, 'options': command, 'input_sha256': sha(data),
                                        'observations': observations, 'passed': observations['before'] == observations['after']})
    report['passed'] = all(case['passed'] for case in report['cases'])
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(f"Optimization behavior: {sum(case['passed'] for case in report['cases'])}/{len(report['cases'])}")
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
