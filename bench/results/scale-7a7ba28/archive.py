#!/usr/bin/env python3
"""Archive completed scale evidence, excluding reproducible large payloads/binaries."""
import json
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'bench'))
from scale_common import validate_complete
from readme_inputs import digest

WORK = ROOT/'target/scale-benchmark'
DEST = ROOT/'bench/results/scale-7a7ba28'
report = json.loads((WORK/'formal/report.json').read_text())
validate_complete(report)
for name in ('accept-base', 'accept-oracles', 'accept-gate-controls'):
    if (WORK/f'{name}.status').read_text().strip() != '0':
        raise ValueError(f'acceptance failed: {name}')
selected = set()
for path in WORK.iterdir():
    if (path.is_file() and not path.name.endswith('-draft.md')
            and path.name not in ('README.md', 'README.zh-CN.md',
                                  'readme-section.en.md', 'readme-section.zh.md')
            and path.suffix in ('.json', '.log', '.status', '.patch', '.md', '.txt', '.py')):
        selected.add(path)
selected.update([WORK/'sealed/build-provenance.json', WORK/'sealed/build.log'])
for directory in WORK.iterdir():
    if not directory.is_dir() or directory.is_symlink():
        continue
    if (directory/'inputs.json').is_file():
        selected.add(directory/'inputs.json')
        selected.update(directory.glob('*/generation.json'))
        selected.update(p for p in (directory/'executed').glob('*') if p.is_file())
    if (directory/'report.json').is_file():
        data = json.loads((directory/'report.json').read_text())
        selected.add(directory/'report.json')
        selected.update(p for p in (directory/'executed').glob('*') if p.is_file())
        # Each measurement has exactly two directory levels below its run root.
        for condition in data.get('results', []):
            for observation in condition['observations']:
                path = Path(observation['directory'])
                for name in ('stdout.txt', 'stderr.txt', 'time.txt'):
                    if (path/name).is_file(): selected.add(path/name)
        for source, checksum in data.get('identities', {}).items():
            if Path(source).name in ('scale_benchmark.py','scale_common.py','scale_validation.py','canonical_sam.rs',
                                     'scale_seal.py','readme_benchmark.py','readme_inputs.py','run.py','upstream.py'):
                snapshot = directory/'executed'/Path(source).name
                if digest(snapshot) != checksum:
                    raise ValueError(f'executed source differs: {snapshot}')
selected.update(p for p in (WORK/'formal-summary').iterdir() if p.is_file())
DEST.mkdir(parents=True, exist_ok=False)
for path in sorted(selected):
    relative = path.relative_to(WORK)
    output = DEST/relative
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(path, output)
# Analysis/control sources are not timed. Preserve their exact versions beside
# the driver snapshots rather than relying on a future checkout's versions.
for relative in (
    'bench/scale_plan.py', 'bench/summarize_scale.py', 'bench/scale_inputs.py',
    'bench/test_scale_benchmark.py', 'bench/test_scale_inputs.py',
    'bench/test_scale_bam_inputs.py', 'bench/test_scale_validation.py',
    'bench/test_canonical_sam.py', 'bench/SCALE-PROTOCOL.md',
    'tests/ci/check.sh', 'tests/ci/test_check.py', 'docs/ADR-FASTX-BYTE-SEARCH.md',
):
    output = DEST/'support'/relative
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT/relative, output)
manifest = {str(p.relative_to(DEST)): digest(p) for p in sorted(DEST.rglob('*')) if p.is_file()}
(DEST/'sha256.json').write_text(json.dumps(manifest, indent=2)+'\n')
print(f'ARCHIVE: {len(manifest)} files, {sum(p.stat().st_size for p in DEST.rglob("*") if p.is_file())} bytes')
print('Write the archive README, then regenerate sha256.json to include that README.')
