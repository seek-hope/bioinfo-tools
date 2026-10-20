#!/usr/bin/env bash
# Compare rust-umitools against the pinned UMI-tools oracle.
#
# The oracle needs UMI-tools 1.1.6 plus samtools. Set UMI_TOOLS_ORACLE_PATH to
# the directory holding them, or put them on PATH yourself.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=tests/ci/oracle.sh
. "$ROOT/tests/ci/oracle.sh"

if ! command -v umi_tools >/dev/null 2>&1; then
  echo "ERROR: umi_tools not found. Set UMI_TOOLS_ORACLE_PATH or add it to PATH." >&2
  exit 3
fi

cd "$ROOT"
# shellcheck source=tests/ci/env.sh
. "$ROOT/tests/ci/env.sh"
python3 "$ROOT/tests/oracle/versions.py" || exit $?
bash "$ROOT/tests/ci/check.sh" build --release >/dev/null
exec python3 "$ROOT/tests/oracle/oracle.py"
