#!/usr/bin/env bash
# Dependency-policy checks. Advisory failure is intentionally not waived.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT"

# Reuse the project build environment so offline cargo-deny sees the same
# CARGO_HOME/registry cache as the other gates.
# shellcheck source=../rust-umitools/tests/ci/env.sh
. "$ROOT/rust-umitools/tests/ci/env.sh"
PATH="$ROOT/tests/ci/env/bin:$PATH"
export PATH

mode=${1:---licenses}
args=(--locked)
if [ "${CARGO_DENY_OFFLINE:-0}" = "1" ]; then
    args+=(--offline)
fi

case "$mode" in
    --licenses*) exec cargo deny "${args[@]}" check licenses sources ;;
    --advisories) exec cargo deny "${args[@]}" check advisories ;;
    --all) exec cargo deny "${args[@]}" check ;;
    -h|--help)
        printf '%s\n' \
            'Usage: bash tests/security/check.sh [--licenses|--advisories|--all]' \
            '' \
            '--licenses    license and source policy (must pass)' \
            '--advisories  advisory policy (currently fails on RUSTSEC-2025-0058)' \
            '--all         full cargo-deny check' \
            '' \
            'Set CARGO_DENY_OFFLINE=1 to add --offline.'
        exit 0
        ;;
    *) printf 'ERROR: unknown mode: %s\n' "$mode" >&2; exit 2 ;;
esac
