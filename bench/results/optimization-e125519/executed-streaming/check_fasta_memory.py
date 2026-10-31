import hashlib
import json
from pathlib import Path
import subprocess
import sys
sys.path.insert(0, 'bench')
from run import parse_time

root = Path('/tmp/bioinfo-perf-e125519')
work = root / 'fasta-memory'
work.mkdir()
path = work / 'two-large.fa'
length = 32 * 1024 * 1024
expected = hashlib.sha256()
with path.open('wb') as out:
    for name, base in [(b'first', b'A'), (b'second', b'C')]:
        out.write(b'>' + name + b'\n')
        expected.update(b'>' + name + b'\n')
        for _ in range(32):
            chunk = base * (1024 * 1024)
            out.write(chunk + b'\n')
            expected.update(chunk)
        expected.update(b'\n')
rows = {}
for arm, directory in [('before', root), ('superseded', root/'optimized-final'), ('after', root/'optimized-current')]:
    output = work / (arm + '.fa')
    time = work / (arm + '.time')
    command = ['/usr/bin/time', '-v', '-o', str(time), 'taskset', '-c', '4', str(directory/'rust-fx'), 'fq2fa', '-w', '0', '-o', str(output), str(path)]
    result = subprocess.run(command, capture_output=True, check=True)
    actual = hashlib.sha256()
    with output.open('rb') as source:
        for chunk in iter(lambda: source.read(1024*1024), b''):
            actual.update(chunk)
    rows[arm] = parse_time(time) | {'command': command, 'output_sha256': actual.hexdigest(), 'passed': actual.digest() == expected.digest()}
    output.unlink()
path.unlink()
passed = all(row['passed'] for row in rows.values())
passed &= rows['after']['max_rss_kb'] <= rows['before']['max_rss_kb'] * 1.25 + 16*1024
passed &= rows['after']['max_rss_kb'] < rows['superseded']['max_rss_kb'] * .8
report = {'purpose': 'record-lifetime regression check; one observation per artifact, not a speed claim', 'length_per_record': length, 'records': 2, 'expected_sha256': expected.hexdigest(), 'observations': rows, 'passed': passed}
(root/'fasta-memory.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report, indent=2))
raise SystemExit(0 if passed else 1)
