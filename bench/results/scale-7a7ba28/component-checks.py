import subprocess
from pathlib import Path
steps=[('final-fx-behavior', ['python3', 'bench/check_optimization.py', '--before', 'target/scale-benchmark/before/rust-fx', '--after', 'target/scale-benchmark/sealed/rust-fx', '--out', 'target/scale-benchmark/final-fx-behavior.json']), ('final-cutadapt', ['python3', 'rust-cutadapt/tests/oracle/differential.py', '--rust', 'target/scale-benchmark/sealed/rust-cutadapt', '--oracle', '/home/rimuru/.conda/envs/ta-cutadapt/bin/cutadapt', '--json', 'target/scale-benchmark/final-cutadapt.json'])]
for name,cmd in steps:
 with Path("target/scale-benchmark",name+".log").open("w") as log:
  code=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT).returncode
 Path("target/scale-benchmark",name+".status").write_text(str(code)+"\n")
 if code: raise SystemExit(code)
