#!/usr/bin/env bash
# W1-QC oracle harness: differential `rust-fastqc` against FastQC 0.13.0,
# plus a MultiQC 1.35 parse check.
#
# Design rules, matching the rest of this workspace:
#
#   * resolve, never hard-code, the oracle paths;
#   * fail closed with exit 3 when an oracle is missing -- never skip, never exit 0
#     (a harness that silently passes without the oracle is worse than none);
#   * one PASS/FAIL line per fixture and a summary, so a failure is diagnosable;
#   * a --self-test that proves the comparison machinery can fail, requiring no
#     oracles at all.
#
# Usage:
#   bash tests/oracle/run_qc_oracle.sh [--rust PATH] [--json FILE] [--self-test]
#
# Resolution order for each oracle:
#   command-line flag, then $FASTQC_ORACLE / $MULTIQC_ORACLE, then tests/ci/env/bin,
#   then PATH.
#
# Exit codes:
#   0  every fixture passed and MultiQC parsed the report
#   1  at least one fixture failed
#   3  fastqc, multiqc or the rust-fastqc binary could not be resolved

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT" || exit 3

RUST_BIN="${RUST_FASTQC:-target/release/rust-fastqc}"
JSON_OUT=""
SELF_TEST=""

while [ $# -gt 0 ]; do
    case "$1" in
        --rust) RUST_BIN="$2"; shift 2 ;;
        --json) JSON_OUT="$2"; shift 2 ;;
        --self-test) SELF_TEST="--self-test"; shift ;;
        -h|--help) sed -n '2,30p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 3 ;;
    esac
done

# --- self-test needs no oracle --------------------------------------------- #

if [ -n "$SELF_TEST" ]; then
    exec python3 "$ROOT/tests/oracle/qc_diff.py" --self-test
fi

# --- resolve the oracles --------------------------------------------------- #

resolve_tool() { # $1 = tool name, $2 = override variable value, $3 = subdir
    local tool="$1" override="$2"
    if [ -n "$override" ]; then
        if [ -x "$override" ] || command -v "$override" >/dev/null 2>&1; then
            command -v "$override" 2>/dev/null || printf '%s' "$override"
            return 0
        fi
        return 1
    fi
    local candidate
    for candidate in \
        "$ROOT/tests/ci/env/bin/$tool" \
        "${CONDA_PREFIX:-/nonexistent}/bin/$tool"
    do
        if [ -x "$candidate" ]; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    command -v "$tool" 2>/dev/null
}

FASTQC_BIN="$(resolve_tool fastqc "${FASTQC_ORACLE:-}")" || FASTQC_BIN=""
MULTIQC_BIN="$(resolve_tool multiqc "${MULTIQC_ORACLE:-}")" || MULTIQC_BIN=""

missing=0
if [ -z "$FASTQC_BIN" ]; then
    echo "ERROR: FastQC 0.13.0 not found." >&2
    echo "       Set FASTQC_ORACLE, create tests/ci/env/bin/fastqc, or put fastqc" >&2
    echo "       on PATH. See tests/ci/PINS-QC.md." >&2
    echo "       Refusing to run: the oracle comparison cannot be evaluated." >&2
    missing=1
fi
if [ -z "$MULTIQC_BIN" ]; then
    echo "ERROR: MultiQC 1.35 not found." >&2
    echo "       Set MULTIQC_ORACLE, create tests/ci/env/bin/multiqc, or put multiqc" >&2
    echo "       on PATH. See tests/ci/PINS-QC.md." >&2
    echo "       Refusing to run: the MultiQC parse check cannot be evaluated." >&2
    missing=1
fi
if [ ! -x "$RUST_BIN" ]; then
    echo "ERROR: rust-fastqc binary not found at $RUST_BIN" >&2
    echo "       Build it with: cargo build --release --offline -p rust-fastqc" >&2
    missing=1
fi
if [ "$missing" -ne 0 ]; then
    exit 3
fi

# --- report versions ------------------------------------------------------- #

echo "rust-fastqc : $RUST_BIN"
echo "fastqc      : $FASTQC_BIN"
echo "multiqc     : $MULTIQC_BIN"
"$FASTQC_BIN" --version 2>&1 | head -3
"$MULTIQC_BIN" --version 2>&1 | head -3
echo

# --- run the matrix -------------------------------------------------------- #

python3 "$ROOT/tests/oracle/qc_diff.py" \
    --rust "$RUST_BIN" \
    --fastqc "$FASTQC_BIN" \
    --multiqc "$MULTIQC_BIN" \
    --validate-multiqc \
    ${JSON_OUT:+--json "$JSON_OUT"}
status=$?

exit "$status"
