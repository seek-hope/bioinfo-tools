import os, subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[2]
work=root/'target/scale-benchmark'
env=dict(os.environ,CARGO_TARGET_DIR=str(root/'target'))
steps=[('fmt',['cargo','fmt','--all']),
       ('base',['bash','tests/ci/check.sh']),
       ('oracles',['bash','tests/ci/check.sh','--with-fastx-qc-oracles']),
       ('package-fx',['cargo','build','--release','--locked','-p','rust-fx']),
       ('canonical-compile',['rustc','--edition','2021','--test','bench/canonical_sam.rs','-o',str(work/'canonical-sam-tests')]),
       ('canonical-tests',[str(work/'canonical-sam-tests')])]
for name,cmd in steps:
 with (work/f'phase6-{name}.log').open('w') as log:
  result=subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 (work/f'phase6-{name}.status').write_text(str(result.returncode)+'\n')
 if result.returncode: raise SystemExit(result.returncode)
