#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[2]
WORK=ROOT/'target/bottleneck-21bc596'
DEST=ROOT/'bench/results/bottleneck-21bc596'
checks=[]
for name,command in [
 ('documentation',['python3',str(WORK/'check-docs.py')]),
 ('repository-docs',['python3','tests/ci/check_docs.py']),
 ('format',['cargo','fmt','--all','--','--check']),
 ('whitespace',['git','diff','--check'])]:
    log=DEST/'validation'/f'{name}.log'
    log.parent.mkdir(parents=True,exist_ok=True)
    with log.open('x') as output:
        result=subprocess.run(command,cwd=ROOT,stdout=output,stderr=subprocess.STDOUT)
    checks.append({'name':name,'command':command,'returncode':result.returncode})
    assert result.returncode==0,(name,result.returncode)
(DEST/'validation/checks.json').write_text(json.dumps(checks,indent=2)+'\n')
shutil.copyfile(WORK/'check-docs.py',DEST/'validation/check-docs.py')
shutil.copyfile(__file__,DEST/'validation/freeze-archive.py')
for name in ['README.md','README.zh-CN.md','docs/DEVELOPMENT-PLAN.md','bench/README.md','.gitattributes']:
    target=DEST/'publication'/name
    target.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(ROOT/name,target)
files={str(p.relative_to(DEST)):hashlib.sha256(p.read_bytes()).hexdigest()
       for p in sorted(DEST.rglob('*')) if p.is_file() and p != DEST/'sha256.json'}
size=sum((DEST/name).stat().st_size for name in files)
with (DEST/'sha256.json').open('x') as output:
    json.dump({'algorithm':'sha256','files':files},output,indent=2)
    output.write('\n')
for name,digest in files.items():
    assert hashlib.sha256((DEST/name).read_bytes()).hexdigest()==digest,name
print('ARCHIVE PASS:',len(files),'SHA-covered files;',size,'covered bytes')
print('FINAL DOCUMENTATION/FORMAT CHECKS PASS')
