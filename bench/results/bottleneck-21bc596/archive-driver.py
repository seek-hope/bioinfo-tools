#!/usr/bin/env python3
"""Archive this completed batch without binaries or reproducible large payloads."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT/'target/bottleneck-21bc596'
DEST = ROOT/'bench/results/bottleneck-21bc596'
assert json.loads((WORK/'formal-audit.json').read_text())['passed']
assert not DEST.exists(), 'refuse to overwrite an evidence archive'
changed = ['rust-bam/src/commands.rs', 'rust-bam/src/lib.rs', 'rust-bam/src/merge_tree.rs',
           'rust-bam/tests/external_sort.rs', 'rust-fx/src/commands.rs', 'rust-fx/src/stats.rs']
for name in changed:
    target = WORK/'final-source'/name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT/name, target)
    if name != 'rust-bam/src/merge_tree.rs':
        original = subprocess.check_output(['git','show',f'21bc59628b3d0ecfc5f32d8218273b1ece992b71:{name}'],cwd=ROOT)
        target = WORK/'before-source'/name
        target.parent.mkdir(parents=True,exist_ok=True)
        target.write_bytes(original)
patch = subprocess.check_output(['git','diff','--binary','HEAD','--',*changed],cwd=ROOT)
new_file = subprocess.run(['git','diff','--no-index','--binary','/dev/null','rust-bam/src/merge_tree.rs'],cwd=ROOT,stdout=subprocess.PIPE)
assert new_file.returncode == 1
(WORK/'native-source.patch').write_bytes(patch + new_file.stdout)
source_hashes = {name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in changed}
(WORK/'final-source-sha256.json').write_text(json.dumps(source_hashes,indent=2)+'\n')

suffixes = {'.py','.rs','.md','.json','.log','.status','.txt','.annotation','.callgrind','.calls','.asm','.stderr','.stdout','.valgrind','.tsv','.patch'}
exclude = {'STATE.md','render-docs.py','archive.py','check-docs.py','report.md'}
DEST.mkdir(parents=True)
for directory, folders, files in os.walk(WORK,followlinks=False):
    folders[:] = [d for d in folders if not (Path(directory)/d).is_symlink() and d != '__pycache__']
    for filename in files:
        path = Path(directory)/filename
        relative = path.relative_to(WORK)
        if path.is_symlink() or path.suffix not in suffixes or (len(relative.parts)==1 and filename in exclude):
            continue
        target = DEST/relative
        target.parent.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(path,target)

support = [
 'bench/scale_common.py','bench/scale_inputs.py','bench/scale_plan.py','bench/scale_benchmark.py',
 'bench/scale_seal.py','bench/scale_validation.py','bench/summarize_scale.py','bench/readme_benchmark.py',
 'bench/readme_inputs.py','bench/check_optimization.py','bench/canonical_sam.rs','bench/run.py','bench/upstream.py',
 'bench/SCALE-PROTOCOL.md','bench/SCALE-REPRODUCTION.md','bench/BOTTLENECK.md',
 'tests/ci/check.sh','tests/oracle/w13_diff.py','tests/oracle/run_w13_oracle.sh','tests/oracle/w14_diff.py']
support += [str(p.relative_to(ROOT)) for p in (ROOT/'bench').glob('test_scale*.py')]
for name in support:
    target=DEST/'support'/name
    target.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(ROOT/name,target)
for source, name in [('/tmp/bioinfo-readme-benchmark/inputs.json','source-inputs.json'),
                     ('/tmp/bioinfo-readme-benchmark/inputs-large.json','source-inputs-large.json'),
                     (str(ROOT/'target/scale-benchmark/inputs-2/inputs.json'),'reused-input-manifest.json')]:
    shutil.copyfile(source,DEST/name)
shutil.copyfile(__file__,DEST/'archive-driver.py')
shutil.copyfile(WORK/'render-docs.py',DEST/'render-docs.py')
print('ARCHIVE COPIED',len(list(DEST.rglob('*'))),'entries; README/validation/manifest still to finalize')
