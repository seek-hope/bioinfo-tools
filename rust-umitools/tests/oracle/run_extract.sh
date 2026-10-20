#!/usr/bin/env bash
# extract checks: string pattern, 3', gzip, paired, quality filter/mask (K10).
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
bash "$ROOT/tests/ci/check.sh" build --release >/dev/null
python3 "$ROOT/tests/oracle/extract.py"
exec python3 "$ROOT/tests/oracle/whitelist.py"
