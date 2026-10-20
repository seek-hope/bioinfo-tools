#!/usr/bin/env bash
# Reproducible-build check for rust-cutadapt.
#
# Builds the same source twice, in two separate CARGO_TARGET_DIR trees, and proves
# the resulting binaries are byte-identical. Prints the command, toolchain, commit
# and SHA-256 so the result can be recorded in the evidence.
#
# Usage:
#   bash packaging/repro-check.sh [PACKAGE]
#
# Two modes are checked, and they answer different questions:
#
#   1. Plain build. Two clean builds from the same checkout path and the same
#      registry path give the same bytes. This is what "reproducible on this
#      machine" means.
#   2. Path-remapped build. `--remap-path-prefix` rewrites the absolute source and
#      registry paths, so the output does not depend on where the checkout lives.
#      This is what "reproducible across machines" needs, and it is the mode a
#      packager should use.
#
# The plain build DOES embed absolute paths (from debug info), so it is only
# reproducible at the same path. That distinction is stated rather than glossed
# over: see docs/EVIDENCE-PACKAGING.md.
#
# Exit codes: 0 reproducible, 1 not reproducible, 3 build failed.

set -uo pipefail

PACKAGE="${1:-rust-cutadapt}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 3

WORK="${TMPDIR:-/tmp}/repro-$$"
BIN_NAME="$(grep -m1 '^name' "$PACKAGE/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"

cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

echo "== reproducible build check =="
echo "package   : $PACKAGE"
echo "rustc     : $(rustc --version)"
echo "cargo     : $(cargo --version)"
echo "commit    : $(git rev-parse HEAD 2>/dev/null || echo '(not a git repo)')"
echo "host      : $(uname -sm)"
echo

run_build() { # $1 = target dir, $2 = extra RUSTFLAGS
    if [ -n "$2" ]; then
        CARGO_TARGET_DIR="$1" RUSTFLAGS="$2" \
            cargo build --release --locked --offline -p "$PACKAGE"
    else
        CARGO_TARGET_DIR="$1" cargo build --release --locked --offline -p "$PACKAGE"
    fi
}

# ---------- mode 1: plain ----------
echo "== mode 1: two clean builds, no remapping =="
if ! run_build "$WORK/a" "" >/dev/null 2>&1; then
    echo "BUILD FAILED (a)" >&2
    exit 3
fi
if ! run_build "$WORK/b" "" >/dev/null 2>&1; then
    echo "BUILD FAILED (b)" >&2
    exit 3
fi

A="$WORK/a/release/$BIN_NAME"
B="$WORK/b/release/$BIN_NAME"
if [ ! -f "$A" ] || [ ! -f "$B" ]; then
    echo "artefact missing: $A or $B" >&2
    exit 3
fi

sha_a=$(sha256sum "$A" | cut -d' ' -f1)
sha_b=$(sha256sum "$B" | cut -d' ' -f1)
echo "  build a: $sha_a"
echo "  build b: $sha_b"
if [ "$sha_a" = "$sha_b" ]; then
    echo "  RESULT : IDENTICAL"
    plain_ok=1
else
    echo "  RESULT : DIFFER"
    plain_ok=0
fi
echo "  size   : $(stat -c %s "$A") bytes"
echo

# ---------- mode 2: path-remapped ----------
echo "== mode 2: two clean builds with --remap-path-prefix =="
REMAP="--remap-path-prefix=$ROOT=/build/source --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/build/cargo"
if ! run_build "$WORK/c" "$REMAP" >/dev/null 2>&1; then
    echo "BUILD FAILED (c)" >&2
    exit 3
fi
if ! run_build "$WORK/d" "$REMAP" >/dev/null 2>&1; then
    echo "BUILD FAILED (d)" >&2
    exit 3
fi

C="$WORK/c/release/$BIN_NAME"
D="$WORK/d/release/$BIN_NAME"
sha_c=$(sha256sum "$C" | cut -d' ' -f1)
sha_d=$(sha256sum "$D" | cut -d' ' -f1)
echo "  build c: $sha_c"
echo "  build d: $sha_d"
if [ "$sha_c" = "$sha_d" ]; then
    echo "  RESULT : IDENTICAL"
    remap_ok=1
else
    echo "  RESULT : DIFFER"
    remap_ok=0
fi

# How many absolute host paths survived? Zero is the point of remapping.
leaked=$(strings -a "$C" 2>/dev/null | grep -c "$ROOT" || true)
echo "  absolute source paths embedded: $leaked (want 0)"
echo "  size   : $(stat -c %s "$C") bytes"
echo

# A stripped, remapped binary is what a package would ship.
strip -o "$WORK/shipped" "$C" 2>/dev/null && \
    echo "stripped sha256: $(sha256sum "$WORK/shipped" | cut -d' ' -f1)"
echo

echo "== summary =="
echo "plain (same path)     : $([ "$plain_ok" = 1 ] && echo REPRODUCIBLE || echo NOT-REPRODUCIBLE)"
echo "remapped (portable)   : $([ "$remap_ok" = 1 ] && echo REPRODUCIBLE || echo NOT-REPRODUCIBLE)"

if [ "$plain_ok" = 1 ] && [ "$remap_ok" = 1 ] && [ "$leaked" = 0 ]; then
    exit 0
fi
exit 1
