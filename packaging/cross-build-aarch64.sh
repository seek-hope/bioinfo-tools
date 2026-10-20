#!/usr/bin/env bash
# Real aarch64 cross build for rust-cutadapt, with a qemu smoke test.
#
# Usage:
#   bash packaging/cross-build-aarch64.sh [PACKAGE]
#
# Exit codes:
#   0  the aarch64 binary was built AND the smoke test passed
#   1  the build succeeded but the smoke test failed
#   3  the cross toolchain is incomplete, so no build was attempted
#   4  the cross build itself failed
#
# ## Why every step is checked explicitly
#
# Two false-success paths were found while writing this script, and both are
# guarded against now:
#
#   1. `binfmt_misc` may register qemu-aarch64 so that an aarch64 ELF "runs"
#      directly. When the loader is missing, the *qemu* helper prints an error but
#      the shell still sees exit status 0. So the smoke test never trusts an
#      implicit interpreter: it always invokes `qemu-aarch64 -L <sysroot>`
#      explicitly and checks the exit status.
#   2. An earlier version of this script reported BLOCKED and could fall through to
#      `exit 0`. Every failure path below returns non-zero, and the final line is
#      the only place that returns 0.
#
# ## Required environment
#
# A complete cross toolchain: aarch64-linux-gnu-gcc, -g++, -ld, and an aarch64
# sysroot with libc and libgcc. On Debian/Ubuntu:
#   apt-get install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross

set -uo pipefail

PACKAGE="${1:-rust-cutadapt}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 3

TARGET="aarch64-unknown-linux-gnu"
OUT="${CARGO_TARGET_DIR:-target}/$TARGET/release"

fail() { echo "ERROR: $*" >&2; }

echo "== host =="
uname -sm
rustc --version
cargo --version
echo

# ---------- 1. the cross toolchain must be complete ---------- #
echo "== cross toolchain =="
missing=0
for tool in aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ aarch64-linux-gnu-ld; do
    if path=$(command -v "$tool" 2>/dev/null); then
        echo "  $tool: $path"
    else
        echo "  $tool: MISSING"
        missing=1
    fi
done

# The sysroot needs both the dynamic loader and libgcc: without them the link step
# or the smoke test fails, and a build that cannot run is not worth claiming.
sysroot=""
for candidate in /usr/aarch64-linux-gnu /usr/aarch64-linux-gnu/sysroot; do
    if [ -e "$candidate/lib/ld-linux-aarch64.so.1" ]; then
        sysroot="$candidate"
        break
    fi
done
if [ -n "$sysroot" ]; then
    echo "  sysroot: $sysroot"
else
    echo "  sysroot: MISSING (no lib/ld-linux-aarch64.so.1)"
    missing=1
fi

if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    echo "  rust target $TARGET: NOT installed"
    missing=1
else
    echo "  rust target $TARGET: installed"
fi

if [ "$missing" -ne 0 ]; then
    fail "cross toolchain incomplete; no build attempted."
    echo "  On Debian/Ubuntu: apt-get install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross" >&2
    exit 3
fi
echo

# ---------- 2. the real build ---------- #
# These four variables are what make the C dependencies (zstd-sys, lzma-sys) and
# the final link use the cross toolchain instead of the host one.
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# Same portability trick as the native reproducible build, so the aarch64 artifact
# does not bake in this checkout's absolute paths either.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$ROOT=/build/source --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/build/cargo"

echo "== cross build ($PACKAGE -> $TARGET) =="
echo "  CC_aarch64_unknown_linux_gnu=$CC_aarch64_unknown_linux_gnu"
echo "  CXX_aarch64_unknown_linux_gnu=$CXX_aarch64_unknown_linux_gnu"
echo "  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"
if ! cargo build --release --locked --offline -p "$PACKAGE" --target "$TARGET"; then
    fail "cross build failed"
    exit 4
fi
echo

BIN="$OUT/$PACKAGE"
if [ ! -f "$BIN" ]; then
    fail "build reported success but $BIN is missing"
    exit 4
fi

# ---------- 3. the artifact must really be aarch64 ---------- #
echo "== artifact =="
ls -l "$BIN" | awk '{print "  size : "$5" bytes"}'
file "$BIN" | sed 's/^/  file : /'
sha256sum "$BIN" | sed 's/^/  sha256: /'

if ! file "$BIN" | grep -q "ARM aarch64"; then
    fail "artifact is not an aarch64 ELF"
    exit 4
fi
# A remapped build must not leak this checkout's path.
leaked=$(strings -a "$BIN" 2>/dev/null | grep -c "$ROOT" || true)
echo "  absolute source paths: $leaked (want 0)"
echo

# ---------- 4. smoke test under qemu ---------- #
# Never rely on binfmt_misc: invoke qemu explicitly with the sysroot so the
# dynamic loader is unambiguous, and record the real exit status.
echo "== smoke test (qemu-aarch64) =="
if ! command -v qemu-aarch64 >/dev/null 2>&1; then
    echo "  qemu-aarch64: MISSING -> build succeeded, but the artifact was not executed"
    echo
    echo "RESULT: BUILD OK, SMOKE TEST NOT RUN"
    exit 1
fi

status=0
version_out=$(qemu-aarch64 -L "$sysroot" "$BIN" --version 2>&1)
version_rc=$?
echo "  --version -> exit $version_rc: $version_out"
if [ "$version_rc" -ne 0 ] || ! printf '%s' "$version_out" | grep -q "rust-cutadapt"; then
    fail "--version failed or did not print the program name"
    status=1
fi

# A version check alone proves the loader works but not that trimming does. Run a
# real trim and compare the output byte for byte against the host build, which is
# the strongest statement available without an aarch64 CI runner.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf '@r1\nAGATCGGAAGAGCACACGTCTGAACTCCAGTCAC\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII\n' > "$work/in.fq"

qemu-aarch64 -L "$sysroot" "$BIN" -a AGATCGGAAGAGC -o "$work/arm.out" "$work/in.fq" >/dev/null 2>&1
arm_rc=$?
echo "  trim (aarch64) -> exit $arm_rc"
[ "$arm_rc" -eq 0 ] || { fail "the aarch64 binary failed to trim"; status=1; }

# Cross-check against the native build when one exists.
native="$ROOT/target/release/$PACKAGE"
if [ -x "$native" ]; then
    "$native" -a AGATCGGAAGAGC -o "$work/host.out" "$work/in.fq" >/dev/null 2>&1
    native_rc=$?
    echo "  trim (host)    -> exit $native_rc"
    if [ "$native_rc" -eq 0 ] && cmp -s "$work/arm.out" "$work/host.out"; then
        echo "  trimmed output: IDENTICAL across architectures"
    else
        fail "trimmed output differs between aarch64 and the host build"
        status=1
    fi
else
    echo "  host build absent; cross-architecture comparison skipped"
fi
echo

# ---------- 5. verdict ---------- #
if [ "$status" -eq 0 ]; then
    echo "RESULT: PASS (aarch64 build + qemu smoke test)"
    exit 0
fi
echo "RESULT: FAIL (see errors above)" >&2
exit 1
