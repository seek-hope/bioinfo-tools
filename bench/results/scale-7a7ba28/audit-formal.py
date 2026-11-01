#!/usr/bin/env python3
"""Audit the completed scale run from the matching repository root checkout."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import sys

sys.path.insert(0, str(Path.cwd() / "bench"))
from scale_common import validate_complete

parser = argparse.ArgumentParser()
parser.add_argument("--workdir", type=Path, default=Path("target/scale-benchmark"))
args = parser.parse_args()
work = args.workdir
report = json.loads((work / "formal/report.json").read_text())
summary = json.loads((work / "formal-summary/summary.json").read_text())
validate_complete(report)
assert all((work / f"{name}.status").read_text().strip() == "0"
           for name in ("formal", "formal-summary", "accept-base", "accept-oracles"))
assert "PIPELINE COMPLETE" in (work / "pipeline.log").read_text()
by_id = {row["condition"]: row for row in summary}
assert len(by_id) == len(summary) == len(report["results"]) == 28
blocks = Counter()
measured = []
ratio_count = 0
qc_masks = []
for row in report["results"]:
    observations = row["observations"]
    result = by_id[row["condition"]["id"]]
    assert len(observations) == 8 and len(row["checks"]) == 4
    blocks.update(o["block"] for o in observations)
    for arm in ("current", "upstream"):
        selected = [o for o in observations if o["arm"] == arm and o["block"] >= 0]
        assert len(selected) == 3
        values = sorted(o["elapsed_s"] for o in selected)
        assert math.isclose(result["arms"][arm]["median_s"], values[1])
        measured.extend(selected)
    ratios = []
    for block in range(3):
        pair = {o["arm"]: o["elapsed_s"] for o in observations if o["block"] == block}
        assert set(pair) == {"current", "upstream"}
        ratios.append(pair["upstream"] / pair["current"])
    if row["condition"]["case"] != "fastqc":
        assert math.isclose(result["paired_upstream_over_current"]["median"], sorted(ratios)[1])
        ratio_count += 1
    else:
        for check in row["checks"]:
            mask = check["module_equal"]
            assert {k for k, v in mask.items() if not v} == {
                "Per sequence GC content", "Sequence Duplication Levels"}
            assert len(mask) == 11 and sum(mask.values()) == 9
            qc_masks.append(mask)
assert blocks == {-1: 56, 0: 56, 1: 56, 2: 56}
assert len(measured) == 168 and ratio_count == 26 and len(qc_masks) == 8
for filename, expected in report["identities"].items():
    h = hashlib.sha256()
    with Path(filename).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    assert h.hexdigest() == expected, filename
assert all(report[k] for k in ("inputs_unchanged", "runtime_payloads_unchanged", "identities_unchanged"))
routes = json.loads((work / "runtime-routes-after-formal.json").read_text())
assert routes["matches_initial_routes"] and all(x["same_directory"] for x in routes["routes"].values())
audit = {
    "passed": True,
    "conditions": len(report["results"]),
    "measured": len(measured),
    "warmups": blocks[-1],
    "pairs": sum(len(row["checks"]) for row in report["results"]),
    "minimum_formal_seconds": min(o["elapsed_s"] for o in measured),
    "below_10_seconds": sum(o["elapsed_s"] < 10 for o in measured),
    "independent_median_and_paired_ratio_checks": True,
    "identity_files_rehashed": len(report["identities"]),
    "qc_checks": len(qc_masks),
    "qc_equal_modules": 9,
    "qc_modules_compared": 11,
    "runtime_routes_unchanged": True,
    "started_utc": report["started_utc"],
    "completed_utc": report["completed_utc"],
}
(work / "formal-audit.json").write_text(json.dumps(audit, indent=2) + "\n")
print("PASS independent audit: 28 medians, 26 paired ratios, 224 invocations, 112 checks")
print(f"PASS final identities: {len(report['identities'])} files rehashed")
print(f"Minimum formal seconds: {audit['minimum_formal_seconds']}; below 10 s: {audit['below_10_seconds']}")
print("FastQC: 8 checks each retain 9/11 equal data modules; no speed ratio")
