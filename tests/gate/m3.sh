#!/usr/bin/env bash
# M3 gate: W1-1 is behaviour-neutral.
#
# The plan's convention (docs/DEVELOPMENT-PLAN.md, "Milestone gates") is one
# script per milestone that prints one PASS/FAIL line per criterion and exits
# non-zero on any failure. Reviewers run this script; they do not re-run every
# individual suite.
#
# Run from the workspace root:
#   bash tests/gate/m3.sh
#
# This gate never builds. If a binary is missing it fails the "binaries present"
# criterion instead, so a reviewer cannot gate a stale artifact by accident.
#
# Overrides (all optional):
#   CUTADAPT_BIN        binary under test   (default target/release/rust-cutadapt)
#   UMITOOLS_BIN        binary under test   (default target/release/rust-umitools)
#   BASELINE_CUTADAPT   reference binary    (default .work/w1-1-baseline/bin/rust-cutadapt.before)
#   BASELINE_UMITOOLS   reference binary    (default .work/w1-1-baseline/bin/rust-umitools.before)
#   GOLDEN_DIR          golden directory    (default tests/gate/golden)
#   GOLDEN_MANIFEST     golden SHA-256 list  (default tests/gate/golden.sha256)
#
# Exit status: 0 when every criterion passes, 1 otherwise.

set -u

SOURCE=${BASH_SOURCE[0]}
if command -v readlink >/dev/null 2>&1; then
    SOURCE=$(readlink -f "$SOURCE" 2>/dev/null || printf '%s' "$SOURCE")
fi
ROOT=$(cd "$(dirname "$SOURCE")/../.." && pwd)
cd "$ROOT" || exit 1

CUTADAPT_BIN=${CUTADAPT_BIN:-target/release/rust-cutadapt}
UMITOOLS_BIN=${UMITOOLS_BIN:-target/release/rust-umitools}
BASELINE_CUTADAPT=${BASELINE_CUTADAPT:-.work/w1-1-baseline/bin/rust-cutadapt.before}
BASELINE_UMITOOLS=${BASELINE_UMITOOLS:-.work/w1-1-baseline/bin/rust-umitools.before}
GOLDEN_DIR=${GOLDEN_DIR:-tests/gate/golden}
GOLDEN_MANIFEST=${GOLDEN_MANIFEST:-tests/gate/golden.sha256}

SNAPSHOT=tests/gate/snapshot.py

if [ -d "$ROOT/tests/ci/env/bin" ]; then
    PATH="$ROOT/tests/ci/env/bin:$PATH"
    export PATH
fi

# Resolve the canonical oracle directory when PATH exposes samtools and
# umi_tools through the same test-environment directory. This keeps the
# recorded @PG CL paths stable without hard-coding a personal path.
if [ -z "${UMI_TOOLS_ORACLE_PATH:-}" ] && command -v samtools >/dev/null 2>&1 && command -v umi_tools >/dev/null 2>&1; then
    samtools_real=$(readlink -f "$(command -v samtools)" 2>/dev/null || true)
    umitools_real=$(readlink -f "$(command -v umi_tools)" 2>/dev/null || true)
    if [ -n "$samtools_real" ] && [ -n "$umitools_real" ] && [ "$(dirname "$samtools_real")" = "$(dirname "$umitools_real")" ]; then
        UMI_TOOLS_ORACLE_PATH=$(dirname "$samtools_real")
        export UMI_TOOLS_ORACLE_PATH
    fi
fi

pass=0
fail=0
criteria=()

pass_criterion() {
    printf 'PASS  %s\n' "$*"
    criteria+=("PASS|$*")
    pass=$((pass + 1))
}
fail_criterion() {
    printf 'FAIL  %s\n' "$*"
    criteria+=("FAIL|$*")
    fail=$((fail + 1))
}

printf 'M3 gate: W1-1 byte neutrality\n'
printf '  workspace      : %s\n' "$ROOT"
printf '  cutadapt       : %s\n' "$CUTADAPT_BIN"
printf '  umitools       : %s\n' "$UMITOOLS_BIN"
printf '  baseline       : %s\n' "$BASELINE_CUTADAPT"
printf '  baseline       : %s\n' "$BASELINE_UMITOOLS"
printf '  golden         : %s\n\n' "$GOLDEN_DIR"

# --------------------------------------------------------------------------- #
# C1. the reference binaries are present and are the recorded ones
# --------------------------------------------------------------------------- #
# The SHA256 values are from docs/EVIDENCE-2026-10.md, so this also proves the
# baseline has not been quietly rebuilt or replaced.
CUTADAPT_SHA=c98c28990e9439f9d9cd1f75d4001dec0f8de7fa31baafe52f22db54ee4099de
UMITOOLS_SHA=df563faa8d27abfa8055f972f2ed7a8a2765a2da59dbdc22ce435c9dc19f6f83

if [ ! -x "$BASELINE_CUTADAPT" ]; then
    fail_criterion "reference binary present: $BASELINE_CUTADAPT"
elif [ ! -x "$BASELINE_UMITOOLS" ]; then
    fail_criterion "reference binary present: $BASELINE_UMITOOLS"
else
    pass_criterion "reference binaries present"
fi

if ! command -v sha256sum >/dev/null 2>&1; then
    fail_criterion "reference binaries match the recorded SHA256 (sha256sum missing)"
elif [ ! -x "$BASELINE_CUTADAPT" ] || [ ! -x "$BASELINE_UMITOOLS" ]; then
    fail_criterion "reference binaries match the recorded SHA256 (binaries missing)"
else
    got_cutadapt=$(sha256sum "$BASELINE_CUTADAPT" | cut -d' ' -f1)
    got_umitools=$(sha256sum "$BASELINE_UMITOOLS" | cut -d' ' -f1)
    if [ "$got_cutadapt" = "$CUTADAPT_SHA" ] && [ "$got_umitools" = "$UMITOOLS_SHA" ]; then
        pass_criterion "reference binaries match the recorded SHA256"
    else
        fail_criterion "reference binaries match the recorded SHA256"
        printf '        cutadapt %s (want %s)\n' "$got_cutadapt" "$CUTADAPT_SHA"
        printf '        umitools %s (want %s)\n' "$got_umitools" "$UMITOOLS_SHA"
    fi
fi

# --------------------------------------------------------------------------- #
# C2. the binaries under test are present
# --------------------------------------------------------------------------- #
if [ -x "$CUTADAPT_BIN" ] && [ -x "$UMITOOLS_BIN" ]; then
    pass_criterion "binaries under test present"
else
    fail_criterion "binaries under test present"
    [ -x "$CUTADAPT_BIN" ] || printf '        missing: %s\n' "$CUTADAPT_BIN"
    [ -x "$UMITOOLS_BIN" ] || printf '        missing: %s\n' "$UMITOOLS_BIN"
fi

# --------------------------------------------------------------------------- #
# C3. the goldens exist
# --------------------------------------------------------------------------- #
golden_count=0
if [ -d "$GOLDEN_DIR" ]; then
    golden_count=$(find "$GOLDEN_DIR" -maxdepth 1 -name '*.json' -type f | wc -l | tr -d ' ')
fi
if [ "$golden_count" -gt 0 ]; then
    pass_criterion "goldens present ($golden_count case files)"
else
    fail_criterion "goldens present (0 case files in $GOLDEN_DIR)"
fi

# The count alone accepts a rewritten corpus. Bind the committed files to a
# reviewed digest list before comparing behavior against them.
if [ ! -f "$GOLDEN_MANIFEST" ]; then
    fail_criterion "goldens match integrity manifest (missing $GOLDEN_MANIFEST)"
elif ! command -v sha256sum >/dev/null 2>&1; then
    fail_criterion "goldens match integrity manifest (sha256sum missing)"
elif [ "$(wc -l < "$GOLDEN_MANIFEST" | tr -d ' ')" -ne "$golden_count" ]; then
    fail_criterion "goldens match integrity manifest (manifest/count mismatch)"
elif sha256sum -c "$GOLDEN_MANIFEST" >/dev/null 2>&1; then
    pass_criterion "goldens match integrity manifest ($golden_count files)"
else
    fail_criterion "goldens match integrity manifest (digest mismatch or missing file)"
fi

# --------------------------------------------------------------------------- #
# C4. the oracle resolves (umi_tools + samtools). Fail closed, never skip.
# --------------------------------------------------------------------------- #
if python3 - "$ROOT" <<'PY'
import sys
from pathlib import Path

sys.path.insert(0, "tests/gate")
import snapshot  # noqa: E402  (the resolver lives in the tool under test)

resolved = snapshot.resolve_oracle_bin(Path(sys.argv[1]).resolve())
if resolved is None:
    print("        umi_tools and samtools did not resolve")
    raise SystemExit(1)
print(f"        oracle: {resolved}")
PY
then
    pass_criterion "umitools oracle resolves (UMI_TOOLS_ORACLE_PATH/CONDA_PREFIX/tests-ci-env/PATH)"
else
    fail_criterion "umitools oracle resolves (set UMI_TOOLS_ORACLE_PATH; refusing to skip)"
fi

# --------------------------------------------------------------------------- #
# C5. verify: the binaries under test reproduce the recorded behaviour
# --------------------------------------------------------------------------- #
if [ -x "$CUTADAPT_BIN" ] && [ -x "$UMITOOLS_BIN" ] && [ "$golden_count" -gt 0 ]; then
    printf '\n--- verify: %s\n' "$CUTADAPT_BIN"
    if python3 "$SNAPSHOT" --verify \
        --cutadapt "$CUTADAPT_BIN" --umitools "$UMITOOLS_BIN" \
        --golden "$GOLDEN_DIR" --workspace "$ROOT"
    then
        pass_criterion "verify: every case matches the recorded behaviour"
    else
        fail_criterion "verify: every case matches the recorded behaviour"
    fi
else
    fail_criterion "verify: every case matches the recorded behaviour (preconditions unmet)"
fi

# --------------------------------------------------------------------------- #
# C6. self-test: the harness can actually fail
# --------------------------------------------------------------------------- #
# This is the anti-trivial check. It runs even when C5 failed: "the verifier
# detects nothing" and "the verifier rejects a planted mutation" are different
# facts and a reviewer needs both.
if [ -x "$CUTADAPT_BIN" ] && [ -x "$UMITOOLS_BIN" ] && [ "$golden_count" -gt 0 ]; then
    printf '\n--- self-test\n'
    if python3 "$SNAPSHOT" --self-test \
        --cutadapt "$CUTADAPT_BIN" --umitools "$UMITOOLS_BIN" \
        --golden "$GOLDEN_DIR" --workspace "$ROOT"
    then
        pass_criterion "self-test: a mutated golden is rejected"
    else
        fail_criterion "self-test: a mutated golden is rejected"
    fi
else
    fail_criterion "self-test: a mutated golden is rejected (preconditions unmet)"
fi

# --------------------------------------------------------------------------- #
# C7. the provenance block is byte-exact (explicit, independent of case order)
# --------------------------------------------------------------------------- #
if [ -x "$UMITOOLS_BIN" ]; then
    expected='rust-umitools 0.2.0
reference        : UMI-tools 1.1.6 (3f2cb4f41e07fe4a6cea3b3389f534bee957176b)
htslib           : via rust-htslib, libdeflate compression
parallel         : rayon worker pool, bounded BGZF threads
commands         : dedup, group, count, count_tab
exit codes       : 0 ok, 2 usage, 3 input, 4 io/resource'
    actual=$("$UMITOOLS_BIN" --version --verbose 2>/dev/null)
    if [ "$actual" = "$expected" ]; then
        pass_criterion "provenance block is byte-exact"
    else
        fail_criterion "provenance block is byte-exact"
        printf '        expected:\n'
        printf '%s\n' "$expected" | sed 's/^/          /'
        printf '        got:\n'
        printf '%s\n' "$actual" | sed 's/^/          /'
    fi
else
    fail_criterion "provenance block is byte-exact (binary missing)"
fi

# --------------------------------------------------------------------------- #
# C8. the exit-code contract is exercised, at least one case per code
# --------------------------------------------------------------------------- #
if [ "$golden_count" -gt 0 ]; then
    codes=$(python3 - "$GOLDEN_DIR" <<'PY'
import json
import sys
from pathlib import Path

seen = set()
for path in Path(sys.argv[1]).glob("*.json"):
    seen.add(json.loads(path.read_text())["exit_code"])
print(" ".join(str(code) for code in sorted(seen)))
PY
)
    missing=""
    # cutadapt contributes 0, 1, 2; umitools contributes 0, 2, 3, 4.
    for code in 0 1 2 3 4; do
        case " $codes " in
            *" $code "*) ;;
            *) missing="$missing $code" ;;
        esac
    done
    if [ -z "$missing" ]; then
        pass_criterion "exit-code contract exercised for 0/1/2/3/4"
    else
        fail_criterion "exit-code contract exercised (missing class(es):$missing)"
    fi
else
    fail_criterion "exit-code contract exercised (no goldens)"
fi

# --------------------------------------------------------------------------- #
# summary
# --------------------------------------------------------------------------- #
total=${#criteria[@]}
printf '\n--- summary\n'
for entry in "${criteria[@]}"; do
    printf '%s  %s\n' "${entry%%|*}" "${entry#*|}"
done
printf '\n%d criterion/criteria: %d passed, %d failed\n' "$total" "$pass" "$fail"

if [ "$fail" -eq 0 ]; then
    printf '\nM3 PASS\n'
    exit 0
fi
printf '\nM3 FAIL\n'
exit 1
