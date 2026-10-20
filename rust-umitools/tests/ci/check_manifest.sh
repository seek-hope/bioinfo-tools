#!/usr/bin/env bash
# The committed compatibility matrix must match its generator.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec python3 "$ROOT/tests/compat/matrix.py" --check "$ROOT/docs/compatibility.tsv"
