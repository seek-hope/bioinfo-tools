#!/usr/bin/env bash
# Fast benchmark smoke: metrics are produced and the JSON has the required keys.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=tests/ci/oracle.sh
. "$ROOT/tests/ci/oracle.sh"
if ! command -v samtools >/dev/null 2>&1; then
  echo "ERROR: samtools not found. Set UMI_TOOLS_ORACLE_PATH or add it to PATH." >&2
  exit 3
fi

cd "$ROOT"
# shellcheck source=tests/ci/env.sh
. "$ROOT/tests/ci/env.sh"
bash "$ROOT/tests/ci/check.sh" build --release >/dev/null

out="$(mktemp)"
trap 'rm -f "$out"' EXIT
python3 "$ROOT/tests/bench/bench.py" --smoke --out "$out"
python3 - "$out" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1]))
required = {"name", "wall_seconds_min", "peak_rss_kb", "threads_peak", "output_bytes"}
names = {case["name"] for case in report["cases"]}
missing = [name for name in ("dedup", "group", "count", "count_tab") if name not in names]
assert not missing, f"missing cases: {missing}"
for case in report["cases"]:
    absent = required - set(case)
    assert not absent, f"{case['name']} missing {absent}"
    assert case["wall_seconds_min"] > 0
    assert case["peak_rss_kb"] > 0
    assert case["output_bytes"] > 0
provenance = report["provenance"]
assert provenance["binary_sha256"] != "missing"
print(f"benchmark smoke ok: {len(report['cases'])} cases, commit={provenance['commit'][:12]}")
PY
