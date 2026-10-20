#!/usr/bin/env bash
# One-shot acceptance runner for the W1-1 byte-neutrality gate.
#
# This exists because the instance that WROTE tests/gate/* could not execute
# commands (its tool set had no shell). It runs acceptance items (a)-(e) from
# the task verbatim and prints the real, unedited output of each so a reviewer
# can paste it into the settlement.
#
# Run from the workspace root:
#   bash tests/gate/acceptance.sh
#
# Exit status: 0 only when (a)-(d) all pass. Item (e) is reported, not asserted:
# it is a "no change" check and its before/after baseline must be captured by
# whoever started the work.

set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT" || exit 1

BASELINE_CUTADAPT=.work/w1-1-baseline/bin/rust-cutadapt.before
BASELINE_UMITOOLS=.work/w1-1-baseline/bin/rust-umitools.before
GOLDEN=tests/gate/golden
SNAPSHOT=tests/gate/snapshot.py

fail=0
step() { printf '\n=== %s\n' "$*"; }
ok() { printf 'OK    %s\n' "$*"; }
bad() { printf 'BAD   %s\n' "$*"; fail=1; }

printf 'W1-1 gate acceptance runner\n'
printf 'workspace: %s\n' "$ROOT"
printf 'python3  : %s\n' "$(python3 --version 2>&1)"

step "preflight: tools and reference binaries"
for tool in python3 sha256sum samtools; do
    if command -v "$tool" >/dev/null 2>&1; then
        ok "$tool on PATH ($(command -v "$tool"))"
    else
        bad "$tool not on PATH"
    fi
done
for bin in "$BASELINE_CUTADAPT" "$BASELINE_UMITOOLS"; do
    if [ -x "$bin" ]; then
        ok "reference binary $bin"
        printf '      sha256 %s\n' "$(sha256sum "$bin" | cut -d' ' -f1)"
    else
        bad "reference binary missing: $bin"
    fi
done

step "a1) --record from the PRE-refactor pair"
rm -rf "$GOLDEN"
if python3 "$SNAPSHOT" --record \
    --cutadapt "$BASELINE_CUTADAPT" \
    --umitools "$BASELINE_UMITOOLS" \
    --out "$GOLDEN"
then
    ok "--record exit 0"
else
    bad "--record exited non-zero"
fi
printf '\n  golden files: %s, total bytes: %s\n' \
    "$(find "$GOLDEN" -maxdepth 1 -name '*.json' | wc -l | tr -d ' ')" \
    "$(du -sb "$GOLDEN" 2>/dev/null | cut -f1)"

step "a2) --verify against the SAME pair (must exit 0, every case PASS)"
if python3 "$SNAPSHOT" --verify \
    --cutadapt "$BASELINE_CUTADAPT" \
    --umitools "$BASELINE_UMITOOLS" \
    --golden "$GOLDEN"
then
    ok "--verify exit 0"
else
    bad "--verify exited non-zero"
fi

step "c) determinism: a second --record must be byte-identical"
first=$(sha256sum "$GOLDEN"/*.json | sha256sum | cut -d' ' -f1)
python3 "$SNAPSHOT" --record \
    --cutadapt "$BASELINE_CUTADAPT" \
    --umitools "$BASELINE_UMITOOLS" \
    --out "$GOLDEN" >/dev/null 2>&1
second=$(sha256sum "$GOLDEN"/*.json | sha256sum | cut -d' ' -f1)
printf '  first  %s\n  second %s\n' "$first" "$second"
if [ "$first" = "$second" ]; then
    ok "two recordings are byte-identical"
else
    bad "two recordings differ"
fi
printf '\n  per-file sha256:\n'
sha256sum "$GOLDEN"/*.json | sed 's/^/    /'

step "b) --self-test must reject a mutated golden and pass the clean one"
if python3 "$SNAPSHOT" --self-test \
    --cutadapt "$BASELINE_CUTADAPT" \
    --umitools "$BASELINE_UMITOOLS" \
    --golden "$GOLDEN"
then
    ok "--self-test exit 0 (SELFTEST PASS)"
else
    bad "--self-test exited non-zero (SELFTEST FAIL)"
fi

step "b-reverse) a mutated golden must make --verify exit NON-ZERO"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -r "$GOLDEN" "$work/mutated"
victim=$(find "$work/mutated" -maxdepth 1 -name '*.json' | sort | head -1)
python3 - "$victim" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
data = json.loads(path.read_text())
# Flip one exit code: the strongest, most obviously detectable mutation.
data["exit_code"] = data["exit_code"] + 7
path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
print(f"  mutated exit_code of {path.name} by +7")
PY
if python3 "$SNAPSHOT" --verify \
    --cutadapt "$BASELINE_CUTADAPT" \
    --umitools "$BASELINE_UMITOOLS" \
    --golden "$work/mutated" >/dev/null 2>&1
then
    bad "--verify accepted a mutated golden (the harness cannot fail)"
else
    ok "--verify rejected the mutated golden (non-zero)"
fi

step "d) m3.sh against the pre-refactor per-crate builds"
CUTADAPT_BIN=${CUTADAPT_BIN:-rust-cutadapt/target/release/rust-cutadapt}
UMITOOLS_BIN=${UMITOOLS_BIN:-/tmp/rust-umitools-build/target/release/rust-umitools}
printf '  CUTADAPT_BIN=%s\n  UMITOOLS_BIN=%s\n' "$CUTADAPT_BIN" "$UMITOOLS_BIN"
CUTADAPT_BIN="$CUTADAPT_BIN" UMITOOLS_BIN="$UMITOOLS_BIN" \
    BASELINE_CUTADAPT="$BASELINE_CUTADAPT" \
    BASELINE_UMITOOLS="$BASELINE_UMITOOLS" \
    GOLDEN_DIR="$GOLDEN" \
    bash tests/gate/m3.sh
m3=$?
printf '\n  m3.sh exit status: %d\n' "$m3"
if [ "$m3" -eq 0 ]; then
    ok "m3.sh passed"
else
    bad "m3.sh failed"
fi

step "e) git state of both repositories"
printf 'rust-cutadapt:\n'
git -C rust-cutadapt status --porcelain | sed 's/^/  /'
printf 'rust-umitools:\n'
git -C rust-umitools status --porcelain | sed 's/^/  /'
printf 'git state is reported, not asserted: compare with the baseline you captured\n'

printf '\n'
if [ "$fail" -eq 0 ]; then
    printf 'ACCEPTANCE PASS\n'
    exit 0
fi
printf 'ACCEPTANCE FAIL\n'
exit 1
