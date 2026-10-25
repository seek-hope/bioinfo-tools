#!/usr/bin/env bash
# Read-only entry point for the W1-1 byte-neutrality gate.
# Recording is deliberately unavailable here: an acceptance run must not
# replace the oracle it is meant to verify.
set -euo pipefail

usage() {
    printf '%s\n' \
        'Usage: bash tests/gate/acceptance.sh [--help | --verify]' \
        '' \
        'No arguments / --help: show this help without running tools.' \
        '--verify: run m3.sh against existing goldens; propagate its exit status.' \
        'Binary and oracle overrides are the same as for m3.sh.' \
        'Recording is disabled. Golden updates require separate maintainer review.'
}

# Validate the complete command line before any work (including --help).
# No argument must not look like a successful verification to a CI wrapper.
if [ "$#" -eq 0 ]; then
    usage >&2
    exit 2
fi
if [ "$#" -gt 1 ]; then
    printf 'ERROR: expected at most one argument\n' >&2
    usage >&2
    exit 2
fi
case "$1" in
    -h|--help) usage; exit 0 ;;
    --verify) ;;
    *) printf 'ERROR: unsupported argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac

# The committed oracle is the comparison target. Do not let an inherited
# environment redirect this safety wrapper to a different directory.
if [ -n "${GOLDEN_DIR+x}" ] || [ -n "${GOLDEN_MANIFEST+x}" ]; then
    printf 'ERROR: GOLDEN_DIR/GOLDEN_MANIFEST overrides are not accepted; use m3.sh directly for diagnostics\n' >&2
    exit 2
fi

SOURCE=${BASH_SOURCE[0]}
if command -v readlink >/dev/null 2>&1; then
    SOURCE=$(readlink -f "$SOURCE" 2>/dev/null || printf '%s' "$SOURCE")
fi
ROOT=$(cd "$(dirname "$SOURCE")/../.." && pwd)
cd "$ROOT"
exec bash tests/gate/m3.sh
