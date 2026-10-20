#!/usr/bin/env bash
# Bounded-memory checks: forced partitioning equals the default path (K9).
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
exec python3 "$ROOT/tests/oracle/spill.py"
