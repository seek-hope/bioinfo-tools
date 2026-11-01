import sys, os, json, subprocess, time, random, statistics
from pathlib import Path
root=Path(__file__).resolve().parent
sys.path.insert(0,str(root.parent.parent/'bench'))
import readme_benchmark as b
manifest=json.loads(Path('/tmp/bioinfo-readme-benchmark/inputs.json').read_text())
result={'kind':'exploratory phase3; not formal','samples':[]}
tools={k:Path('/tmp/bioinfo-readme-repro-oracles/bin')/k for k in ['seqkit','fq','samtools']}
for case, dataset in [(c,d) for c in ['fx_stats','fq_lint'] for d in ['real-1000000','synthetic-1000000']]+[('bam_sort','synthetic-bam-1000000')]:
 d=next(x for x in manifest['datasets'] if x['id']==dataset)
 for repeat in range(3):
  arms=['before','phase2','after','upstream'] if case!='bam_sort' else ['before','after','after4','upstream','upstream4']
  random.Random(20261102+repeat).shuffle(arms)
  for side in arms:
   directory=root/'trial3'/f'{case}-{dataset}-{repeat}-{side}'
   bindir=root/side if side in ['before','phase2'] else root.parent/'release'
   rust={'fx':bindir/'rust-fx','bam':bindir/'rust-bam'}
   cmd,out,compression,paths=b.commands(case,d,'upstream' if side.startswith('upstream') else 'rust',directory,rust,tools,'4','0,2,4,6')
   if side.endswith('4'):
    cmd[2]='0,2,4,6'
    option='-@' if side=='upstream4' else '--threads'
    cmd[cmd.index(option)+1]='3' if side=='upstream4' else '4'
   directory.joinpath('tmp').mkdir(exist_ok=True)
   b.warm(paths)
   begin=time.perf_counter()
   p=subprocess.run(cmd,capture_output=True,timeout=120,env=dict(os.environ,TMPDIR=str(directory/'tmp')))
   elapsed=time.perf_counter()-begin
   if case=='fq_lint': out.write_bytes(p.stdout)
   if out.exists():
    with out.open('rb') as f: os.fsync(f.fileno())
   row={'case':case,'dataset':dataset,'repeat':repeat,'arm':side,'command':cmd,'elapsed_s':elapsed,'exit':p.returncode,'sha256':b.digest(out) if out.exists() else None,'stderr':p.stderr.decode(errors='replace')}
   if case=='bam_sort': row['validation']=b.bam_summary(out,tools['samtools'],None)
   result['samples'].append(row)
   (root/'trial3.json').write_text(json.dumps(result,indent=2)+'\n')
   print(case,dataset,repeat,side,round(elapsed,4),p.returncode,flush=True)
   if p.returncode: raise RuntimeError(row)
   out.unlink()
