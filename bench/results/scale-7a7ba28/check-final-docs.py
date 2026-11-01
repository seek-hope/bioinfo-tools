#!/usr/bin/env python3
"""Verify published scale documentation against the accepted evidence."""
from pathlib import Path
import re
import subprocess

root = Path.cwd()
files = ["README.md", "README.zh-CN.md", "bench/SCALE.md", "bench/SCALE-REPRODUCTION.md", "bench/README.md", "bench/results/scale-7a7ba28/README.md"]
texts = {name: (root / name).read_text() for name in files}
for name, text in texts.items():
    for label, url in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", text):
        url = url.strip("<>").split("#", 1)[0]
        if not url or "://" in url or url.startswith("mailto:"):
            continue
        assert (root / name).parent.joinpath(url).exists(), (name, label, url)
    print(f"PASS links: {name}")

for name in files[:2]:
    text = texts[name]
    before = subprocess.check_output(["git", "show", f"dacff08:{name}"], text=True)
    current_sections = re.split(r"(?m)^## ", text)
    original_sections = re.split(r"(?m)^## ", before)
    assert len(current_sections) == len(original_sections) == 6
    for i in (0, 1, 2, 3, 5):
        assert current_sections[i] == original_sections[i], (name, "Argon section changed", i)
    assert text.count("<!-- README_SCALE_START -->") == 1
    assert text.count("<!-- README_SCALE_END -->") == 1
    assert "TBD" not in current_sections[4] and "PENDING" not in current_sections[4]
    print(f"PASS preserved Argon High sections and completed performance table: {name}")

blocks = [re.findall(r"(?ms)^```bash\n(.*?)^```", texts[name]) for name in files[:2]]
assert blocks[0] == blocks[1] and len(blocks[0]) == 6
for block in blocks[0]:
    subprocess.run(["bash", "-n"], input=block, text=True, check=True)
print("PASS 6 identical bilingual Bash blocks; bash -n")

def numbers(name):
    section = texts[name].split("<!-- README_SCALE_START -->", 1)[1].split("<!-- README_SCALE_END -->", 1)[0]
    rows = [line for line in section.splitlines() if line.startswith("| ")][2:]
    assert len(rows) == 26
    result = []
    for row in rows:
        cells = [cell.strip() for cell in row.split("|")[1:-1]]
        assert len(cells) == 6 and all(cells)
        result.append((re.search(r"(\d+)M", cells[1]).group(1), cells[2:]))
    return result
assert numbers("README.md") == numbers("README.zh-CN.md")
qc = []
for name in files[:2]:
    section = texts[name].split("<!-- README_SCALE_END -->", 1)[1]
    rows = [line for line in section.splitlines() if line.startswith("| ")][2:]
    assert len(rows) == 2
    qc.append([[cell.strip() for cell in row.split("|")[3:-1]] for row in rows])
assert qc[0] == qc[1]
print("PASS 26 comparable rows + 2 descriptive QC rows: bilingual values identical")

for name in ("bench/SCALE.md", "bench/SCALE-REPRODUCTION.md"):
    assert not re.search(r"DRAFT|PENDING COMPLETE|TBD", texts[name])
for block in re.findall(r"(?ms)^```bash\n(.*?)^```", texts["bench/SCALE-REPRODUCTION.md"]):
    subprocess.run(["bash", "-n"], input=block, text=True, check=True)
print("PASS completed report and reproduction shell syntax")
changed = subprocess.check_output(["git", "diff", "--name-only", "HEAD"], text=True).splitlines()
assert not any(p.startswith(("rust-umitools/", "tests/gate/golden", "tests/oracle/", "compat/matrix")) for p in changed)
print("PASS protected UMI, fixtures and comparison harnesses unchanged")
