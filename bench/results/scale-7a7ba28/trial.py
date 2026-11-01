import sys, os, json, subprocess, time, hashlib, random
from pathlib import Path
root=Path(__file__).resolve().parent
sys.path.insert(0,str(root.parent.parent/'bench'))
import readme_benchmark as b
inputs=json.loads(Path('/tmp/bioinfo-readme-benchmark/inputs.json').read_text())
d=next(x for x in inputs['datasets'] if x['id']=='synthetic-1000000')
tools={k:Path('/tmp/bioinfo-readme-repro-oracles/bin')/k for k in ['seqkit','fq','cutadapt']}
tools['cutadapt']=Path('/home/rimuru/.conda/envs/ta-cutadapt/bin/cutadapt')
result={'kind':'exploratory only; not formal benchmark','dataset':d,'samples':[]}
for case in ['cutadapt_plain','cutadapt_4cpu','fx_stats','fx_fq2fa','fq_lint']:
 for repeat in range(3):
  arms=['before','after','upstream']; random.Random(20261101+repeat).shuffle(arms)
  for side in arms:
   directory=root/'trial'/f'{case}-{repeat}-{side}'
   bindir=root/'before' if side=='before' else root.parent/'release'
   rust={'cutadapt':bindir/'rust-cutadapt','fx':bindir/'rust-fx'}
   cmd,out,compression,paths=b.commands(case,d,'upstream' if side=='upstream' else 'rust',directory,rust,tools,'4','0,2,4,6')
   b.warm(paths)
   begin=time.perf_counter()
   p=subprocess.run(cmd,capture_output=True,timeout=120)
   elapsed=time.perf_counter()-begin
   if case=='fq_lint': out.write_bytes(p.stdout)
   if out.exists():
    with out.open('rb') as f: os.fsync(f.fileno())
   row={'case':case,'repeat':repeat,'arm':side,'command':cmd,'elapsed_s':elapsed,'exit':p.returncode,'sha256':b.digest(out) if out.exists() else None,'stderr':p.stderr.decode(errors='replace')}
   result['samples'].append(row)
   (root/'trial.json').write_text(json.dumps(result,indent=2)+'\n')
   print(case,repeat,side,round(elapsed,4),p.returncode,flush=True)
   if p.returncode: raise RuntimeError(row)
   out.unlink()
