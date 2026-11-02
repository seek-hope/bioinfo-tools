#!/usr/bin/env bash
# W1-3 oracle harness: differential `rust-fx` against SeqKit 2.14.0 and fq 0.12.0.
#
# THIS HARNESS HAS NEVER REPORTED A PASS. Both oracles are absent from this
# environment and cannot be obtained offline (see docs/EVIDENCE-W1-3.md and
# .work/w1-3-oracle-status.md). It is committed ready to run so that the
# differential can be executed the moment the tools are available, and so that
# its absence is machine-checkable rather than a claim in prose.
#
# Design rules, matching the rest of this workspace:
#
#   * resolve, never hard-code, the oracle paths (plan W0-2 rule);
#   * fail closed with exit 3 when an oracle is missing — never skip, never
#     exit 0 (a harness that silently passes without the oracle is worse than no
#     harness at all);
#   * one PASS/FAIL line per case and a summary, so a failure is diagnosable.
#
# Usage:
#   bash tests/oracle/run_w13_oracle.sh [--rust PATH] [--json FILE] [-v]
#
# Resolution order for each oracle:
#   $SEQKIT_ORACLE / $FQ_ORACLE, then tests/ci/env/bin, then $CONDA_PREFIX/bin,
#   then PATH.
#
# Exit codes:
#   0  every case passed
#   1  at least one case failed
#   3  an oracle (or the rust-fx binary) could not be resolved

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT" || exit 3

RUST_BIN="${RUST_FX:-target/release/rust-fx}"
JSON_OUT=""
VERBOSE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --rust) RUST_BIN="$2"; shift 2 ;;
        --json) JSON_OUT="$2"; shift 2 ;;
        -v|--verbose) VERBOSE="-v"; shift ;;
        -h|--help) sed -n '2,30p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 3 ;;
    esac
done

# --- resolve the oracles --------------------------------------------------- #

resolve_tool() { # $1 = tool name, $2 = override variable value
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

SEQKIT_BIN="$(resolve_tool seqkit "${SEQKIT_ORACLE:-}")" || SEQKIT_BIN=""
FQ_BIN="$(resolve_tool fq "${FQ_ORACLE:-}")" || FQ_BIN=""

missing=0
if [ -z "$SEQKIT_BIN" ]; then
    echo "ERROR: seqkit 2.14.0 not found." >&2
    echo "       Set SEQKIT_ORACLE, create tests/ci/env/bin/seqkit, activate the conda" >&2
    echo "       environment from tests/ci/env.yml, or put seqkit on PATH." >&2
    echo "       Refusing to run: W1-3's differential criterion cannot be evaluated." >&2
    missing=1
fi
if [ -z "$FQ_BIN" ]; then
    echo "ERROR: fq 0.12.0 not found." >&2
    echo "       Set FQ_ORACLE, create tests/ci/env/bin/fq, activate the conda environment" >&2
    echo "       from tests/ci/env.yml, or put fq on PATH." >&2
    missing=1
fi
if [ ! -x "$RUST_BIN" ]; then
    echo "ERROR: rust-fx binary not found at $RUST_BIN" >&2
    echo "       Build it with: cargo build --release --offline -p rust-fx" >&2
    missing=1
fi
if [ "$missing" -ne 0 ]; then
    exit 3
fi

# --- report versions ------------------------------------------------------- #

echo "rust-fx : $RUST_BIN"
echo "seqkit  : $SEQKIT_BIN"
echo "fq      : $FQ_BIN"
"$SEQKIT_BIN" version 2>&1 | head -3
"$FQ_BIN" --version 2>&1 | head -3
echo

# --- run the matrix -------------------------------------------------------- #

python3 "$ROOT/tests/oracle/w13_diff.py" \
    --rust "$RUST_BIN" \
    --seqkit "$SEQKIT_BIN" \
    --fq "$FQ_BIN" \
    ${JSON_OUT:+--json "$JSON_OUT"} \
    ${VERBOSE:+--verbose}
status=$?

exit "$status"
