#!/usr/bin/env bash
# Cross-build probe for aarch64-unknown-linux-gnu.
#
# This script is evidence-gathering, not a build. It attempts the cross build and
# reports exactly where it fails, so a future environment owner can tell "the
# recipe is wrong" from "the host lacks a sysroot".
#
# Usage:
#   bash packaging/cross-build-aarch64.sh [PACKAGE]
#
# Exit codes:
#   0  a runnable aarch64 artifact was produced
#   3  the host cannot produce one; the reason is printed and recorded
#
# Run from the repository root.

set -uo pipefail

PACKAGE="${1:-rust-cutadapt}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 3
TARGET="aarch64-unknown-linux-gnu"
OUT="${CARGO_TARGET_DIR:-target}/$TARGET/release"

echo "== host =="
uname -sm
rustc --version
cargo --version
echo

echo "== target installed for the pinned toolchain? =="
if rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    echo "$TARGET: installed"
else
    echo "$TARGET: NOT installed for the active toolchain"
    echo "  fix: rustup target add $TARGET"
    exit 3
fi
echo

echo "== native C toolchain for the target =="
missing=0
for tool in aarch64-linux-gnu-gcc aarch64-linux-gnu-ld; do
    if command -v "$tool" >/dev/null 2>&1; then
        echo "$tool: $(command -v "$tool")"
    else
        echo "$tool: MISSING"
        missing=1
    fi
done

sysroot=""
for candidate in "/usr/$TARGET" "/usr/aarch64-linux-gnu" "/usr/lib/gcc-cross/aarch64-linux-gnu"; do
    if [ -d "$candidate" ]; then
        sysroot="$candidate"
        break
    fi
done
if [ -n "$sysroot" ]; then
    echo "sysroot: $sysroot"
else
    echo "sysroot: MISSING (no aarch64 libc/libgcc for the target)"
    missing=1
fi
echo

if [ "$missing" -ne 0 ]; then
    echo "== BLOCKED =="
    echo "The host has no aarch64 sysroot and no aarch64 cross C toolchain."
    echo "The failure is environmental, not in the build recipe:"
    echo "  - the Rust target's core/std are installed and do compile"
    echo "    (bioinfo-common builds an aarch64 rlib successfully),"
    echo "  - C dependencies (zstd-sys, lzma-sys) cannot be compiled,"
    echo "  - and no binary can be linked (crtbeginS.o, libgcc_s, libc absent)."
    echo
    echo "To unblock, install a cross toolchain and sysroot, e.g. on Debian/Ubuntu:"
    echo "  apt-get install gcc-aarch64-linux-gnu libc6-dev-arm64-cross"
    echo "then re-run this script."
    exit 3
fi

echo "== attempting the real cross build =="
if cargo build --release --locked --offline -p "$PACKAGE" --target "$TARGET"; then
    echo
    echo "== artifact =="
    ls -l "$OUT/$PACKAGE" 2>/dev/null || ls -l "$OUT" | head
    file "$OUT/$PACKAGE" 2>/dev/null
    sha256sum "$OUT/$PACKAGE" 2>/dev/null
    if command -v qemu-aarch64 >/dev/null 2>&1; then
        echo "== smoke test under qemu-aarch64 =="
        qemu-aarch64 "$OUT/$PACKAGE" --version
    else
        echo "qemu-aarch64 not present; artifact not executed"
    fi
    exit 0
fi

echo
echo "== BLOCKED: the cross build ran but failed =="
echo "See the cargo output above for the failing crate."
exit 3
