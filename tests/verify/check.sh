#!/usr/bin/env bash
# Targeted verification gate. Fast mode: proptest + Miri on a pure-Rust crate.
# Full mode additionally requires Kani and TLA+ tools; absence is a hard failure.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT"

mode=fast
for arg in "$@"; do
    case "$arg" in
        --fast) mode=fast ;;
        --full) mode=full ;;
        -h|--help)
            printf '%s\n' 'Usage: bash tests/verify/check.sh [--fast|--full]'
            printf '%s\n' '  --fast  proptest invariants + Miri on bioinfo-common (K4)'
            printf '%s\n' '  --full  also require Kani harnesses and TLA+ model checking'
            exit 0
            ;;
        *) printf 'ERROR: unknown argument: %s\n' "$arg" >&2; exit 2 ;;
    esac
done

printf '== proptest invariants ==\n'
cargo test --manifest-path verification/Cargo.toml

# K4 provisions the pinned Miri toolchain/component if absent. Installation
# failure is a hard exit 3; Miri is never silently skipped.
printf '\n== Miri: bioinfo-common ==\n'
MIRI_TOOLCHAIN=${MIRI_TOOLCHAIN:-nightly-2026-05-28}
if ! rustup run "$MIRI_TOOLCHAIN" cargo miri --version >/dev/null 2>&1; then
    printf 'Miri is not installed for %s; installing the pinned toolchain and component (network is allowed)...\n' "$MIRI_TOOLCHAIN" >&2
    rustup toolchain install "$MIRI_TOOLCHAIN" --profile minimal --component miri >&2 || {
        printf 'ERROR: could not install Miri for %s; fast verification cannot claim it\n' "$MIRI_TOOLCHAIN" >&2
        exit 3
    }
fi
if ! rustup run "$MIRI_TOOLCHAIN" cargo miri --version >/dev/null 2>&1; then
    printf 'ERROR: Miri is still unavailable for %s after installation\n' "$MIRI_TOOLCHAIN" >&2
    exit 3
fi
rustup run "$MIRI_TOOLCHAIN" cargo miri test -p bioinfo-common --lib

if [ "$mode" = "full" ]; then
    printf '\n== Kani bounded harnesses ==\n'
    cargo kani --version >/dev/null 2>&1 || {
        printf 'ERROR: cargo kani is not installed; full verification is blocked\n' >&2
        exit 3
    }
    cargo kani --manifest-path verification/kani/Cargo.toml

    printf '\n== TLA+ models ==\n'
    TLA_JAR=${TLA2TOOLS_JAR:-"$ROOT/verification/tla/tla2tools.jar"}
    if [ ! -f "$TLA_JAR" ]; then
        printf 'ERROR: TLA+ tools not found at %s; full verification is blocked\n' "$TLA_JAR" >&2
        printf 'Download tla2tools.jar from the tlaplus release and set TLA2TOOLS_JAR.\n' >&2
        exit 3
    fi
    ( cd verification/tla && java -cp "$TLA_JAR" tlc2.TLC -config CutadaptPipeline.cfg CutadaptPipeline.tla )
    ( cd verification/tla && java -cp "$TLA_JAR" tlc2.TLC -config BamSort.cfg BamSort.tla )
    rm -rf verification/tla/states verification/tla/*_TTrace_*.tla
    printf '\nFULL VERIFICATION PASS (proptest + Miri + Kani + TLA+)\n'
else
    printf '\nFAST VERIFICATION PASS (proptest + Miri bioinfo-common)\n'
fi
