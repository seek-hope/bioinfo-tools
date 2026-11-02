#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[2]
WORK=ROOT/'target/bottleneck-21bc596'
ARCHIVE=ROOT/'bench/results/bottleneck-21bc596'
summary=json.loads((WORK/'formal-summary/summary.json').read_text())

def section(text,heading):
    start=text.index(heading)
    end=text.index('\n## ',start+len(heading))
    return start,end,text[start:end]

for file,heading in [('README.md','## Performance Comparison with Reference Software'),('README.zh-CN.md','## 和参考软件的性能对比')]:
    before=subprocess.check_output(['git','show',f'HEAD:{file}'],cwd=ROOT,text=True)
    after=(ROOT/file).read_text()
    a,b,_=section(before,heading);x,y,_=section(after,heading)
    assert before[:a]==after[:x] and before[b:]==after[y:],file
    assert re.findall(r'^## .+$',before,re.M)==re.findall(r'^## .+$',after,re.M)
    assert re.findall(r'```(?:bash|mermaid)\n.*?```',before,re.S)==re.findall(r'```(?:bash|mermaid)\n.*?```',after,re.S)
    for marker in ['SCALE','BOTTLENECK']:
        assert after.count(f'<!-- README_{marker}_START -->')==after.count(f'<!-- README_{marker}_END -->')==1
    pattern=r'<!-- README_SCALE_START -->(.*?)<!-- README_SCALE_END -->'
    assert re.search(pattern,before,re.S).group(1)==re.search(pattern,after,re.S).group(1)
    table=re.search(r'<!-- README_BOTTLENECK_START -->(.*?)<!-- README_BOTTLENECK_END -->',after,re.S).group(1)
    rows=[line for line in table.splitlines() if line.startswith('| ')][2:]
    assert len(rows)==len(summary)==9
    for line,result in zip(rows,summary):
        cells=[c.strip() for c in line.split('|')[1:-1]]
        assert cells[2]==str(result['cores'])
        assert str(result['records']//1000000)+'M' in cells[1]
        for index,arm in [(3,'upstream'),(4,'current')]:
            value=result['arms'][arm]
            assert cells[index]==f"{value['median_s']:.2f} [{value['min_s']:.2f}–{value['max_s']:.2f}]"
        assert cells[5]==f"{result['paired_upstream_over_current']['median']:.2f}×"
print('PASS README non-performance prose, five-section order, shell/Mermaid blocks and historical table unchanged')
print('PASS all nine bilingual rows match the complete summary')

pages=['README.md','README.zh-CN.md','bench/BOTTLENECK.md','bench/README.md','docs/DEVELOPMENT-PLAN.md','bench/results/bottleneck-21bc596/README.md']
links=0
for file in pages:
    path=ROOT/file
    for link in re.findall(r'\]\(([^)]+)\)',path.read_text()):
        if '://' in link or link.startswith('#'):
            continue
        target=link.split('#')[0]
        assert (path.parent/target).exists(),(file,link)
        links+=1
print('PASS local document links:',links)

for number in [1,2]:
    data=json.loads((WORK/f'screen-{number}/report.json').read_text())
    assert data['passed'] and len(data['observations'])==32
    cases={o['case'] for o in data['observations']}
    assert len(cases)==4
    expected={(case,block,arm) for case in cases for block in [-1,0,1,2] for arm in ['before','current']}
    actual={(o['case'],o['block'],o['arm']) for o in data['observations']}
    assert actual==expected
    assert all(o['returncode']==0 for o in data['observations'])
    for case in cases:
        assert len({o['output_sha256'] for o in data['observations'] if o['case']==case})==1
print('PASS both complete 32-invocation screens; all output bytes match')

sealed=json.loads((WORK/'sealed/build-provenance.json').read_text())
for name,digest in sealed['source_sha256'].items():
    assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest,name
assert not (ROOT/'rust-bam/src/sort_run_writer.rs').exists()
for name,digest in json.loads((ARCHIVE/'final-source-sha256.json').read_text()).items():
    assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest,name
    assert hashlib.sha256((ARCHIVE/'final-source'/name).read_bytes()).hexdigest()==digest,name
changed=subprocess.check_output(['git','diff','--name-only','HEAD'],cwd=ROOT,text=True).splitlines()
protected=['rust-umitools/','compat/','packaging/','dist/','bench/results/scale-7a7ba28/','tests/']
assert not any(name.startswith(tuple(protected)) for name in changed),changed
print('PASS final sealed source and archived integration test match; protected artifacts unchanged')
