#!/usr/bin/env bash
# Native quality gate. Network is allowed by default; --offline is opt-in and
# --isolate-network runs the oracle probes/tests in a user/network namespace.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: bash tests/ci/check.sh [--offline] [--isolate-network] [--with-fastx-qc-oracles]

Default: network access/installation is allowed; run fmt, clippy, release tests,
native binaries and the cutadapt visible-fixture smoke.
  --offline             export CARGO_NET_OFFLINE=true and pass --offline to Cargo;
                        missing toolchains/oracles fail instead of being installed.
  --isolate-network     run oracle version probes and oracle suites under
                        `unshare -Urn`; optional and requires permitted namespaces.
  --with-fastx-qc-oracles
                        also run installed SeqKit/fq and FastQC/MultiQC differentials.
EOF
}

offline=0
isolate_network=0
with_oracles=0
for arg in "$@"; do
    case "$arg" in
        --offline) offline=1 ;;
        --isolate-network) isolate_network=1 ;;
        --with-fastx-qc-oracles) with_oracles=1 ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'ERROR: unknown argument: %s\n' "$arg" >&2; usage >&2; exit 2 ;;
    esac
done

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT"

# MultiQC's update check is unrelated to our report and would make oracle runs
# depend on the network even when the fixture data are local.
export MULTIQC_NO_VERSION_CHECK=true

cargo_offline=()
if [ "$offline" -eq 1 ]; then
    export CARGO_NET_OFFLINE=true
    cargo_offline=(--offline)
else
    unset CARGO_NET_OFFLINE || true
fi

fail_preflight() { printf 'ERROR: %s\n' "$*" >&2; exit 3; }

command -v python3 >/dev/null || fail_preflight 'python3 (3.11+) is required'
toolchain=$(python3 -c 'import tomllib; print(tomllib.load(open("rust-toolchain.toml", "rb"))["toolchain"]["channel"])') \
    || fail_preflight 'cannot read rust-toolchain.toml (Python 3.11+ required)'

CARGO=(cargo)
RUSTC=(rustc)
if command -v rustup >/dev/null; then
    if ! rustup run "$toolchain" rustc --version >/dev/null 2>&1; then
        if [ "$offline" -eq 1 ]; then
            fail_preflight "Rust $toolchain is not installed; rerun without --offline to install it"
        fi
        printf 'Installing pinned Rust %s with rustfmt and clippy...\n' "$toolchain"
        rustup toolchain install "$toolchain" --profile minimal --component rustfmt --component clippy
    fi
    CARGO=(rustup run "$toolchain" cargo)
    RUSTC=(rustup run "$toolchain" rustc)
fi

version=$("${RUSTC[@]}" --version) || fail_preflight "Rust $toolchain is required"
case "$version" in
    "rustc $toolchain "*) ;;
    *) fail_preflight "expected Rust $toolchain, got $version" ;;
esac
"${CARGO[@]}" --version || fail_preflight 'cargo is required'
[ -z "${CARGO_BUILD_TARGET:-}" ] || fail_preflight 'this is a native gate; unset CARGO_BUILD_TARGET'
printf 'Quality gate (%s; %s): %s\n' \
    "$([ "$offline" -eq 1 ] && printf isolated || printf online)" \
    "$([ "$isolate_network" -eq 1 ] && printf 'oracles isolated' || printf 'host network')" \
    "$version"

oracle_prefix=()
if [ "$isolate_network" -eq 1 ]; then
    command -v unshare >/dev/null || fail_preflight '--isolate-network needs Linux unshare'
    unshare -Urn true || fail_preflight '--isolate-network needs permitted user/network namespaces'
    oracle_prefix=(unshare -Urn)
fi

check_oracle() {
    local variable="$1" tool="$2" version_arg="$3" expected="$4"
    local override="${!variable:-}" candidate resolved="" banner
    if [ -n "$override" ]; then
        resolved=$(command -v -- "$override") || fail_preflight "$variable does not resolve: $override"
    else
        for candidate in "$ROOT/tests/ci/env/bin/$tool" "${CONDA_PREFIX:-/nonexistent}/bin/$tool" "$tool"; do
            if resolved=$(command -v -- "$candidate"); then break; fi
        done
    fi
    [ -n "$resolved" ] || fail_preflight "preinstall $tool or set $variable"
    banner=$("${oracle_prefix[@]}" "$resolved" "$version_arg" 2>&1) || fail_preflight "$tool version probe failed: $banner"
    banner=${banner%%$'\n'*}
    case "$banner" in
        "$expected"|"$expected ("*) ;;
        *) fail_preflight "$tool version mismatch: expected '$expected', got '$banner'" ;;
    esac
    printf -v "$variable" '%s' "$resolved"
    export "${variable:?}"
    printf 'Oracle: %s\n' "$banner"
}

if [ "$with_oracles" -eq 1 ]; then
    check_oracle SEQKIT_ORACLE seqkit version 'seqkit v2.14.0'
    check_oracle FQ_ORACLE fq --version 'fq v0.12.0'
    check_oracle FASTQC_ORACLE fastqc --version 'FastQC v0.13.0'
    check_oracle MULTIQC_ORACLE multiqc --version 'multiqc, version 1.35'
fi

run() {
    printf '\n==>'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

run python3 tests/ci/check_docs.py
run python3 tests/ci/check_compat.py
run python3 tests/ci/test_compat_exceptions.py
run python3 tests/ci/test_seqkit_exception_gate.py
run python3 tests/ci/test_tile_proof.py
run python3 tests/ci/test_kmer_proof.py
run python3 bench/test_readme_benchmark.py
run python3 -W error::ResourceWarning bench/test_optimization_benchmark.py
run python3 -W error::ResourceWarning bench/test_scale_benchmark.py
run python3 -W error::ResourceWarning bench/test_scale_inputs.py
run python3 -W error::ResourceWarning bench/test_scale_validation.py
run python3 tests/gate/test_acceptance.py
run "${CARGO[@]}" fmt --all -- --check
run "${CARGO[@]}" clippy --workspace --all-targets --locked "${cargo_offline[@]}" -- -D warnings
RUSTDOCFLAGS="-D warnings" run "${CARGO[@]}" doc --workspace --no-deps --locked "${cargo_offline[@]}"
run "${CARGO[@]}" test --workspace --release --locked "${cargo_offline[@]}"
run "${CARGO[@]}" build --workspace --bins --release --locked "${cargo_offline[@]}"
run python3 -W error::ResourceWarning bench/test_canonical_sam.py

# Cargo metadata respects CARGO_TARGET_DIR and build.target-dir configuration.
metadata=$("${CARGO[@]}" metadata --no-deps --format-version 1 --locked "${cargo_offline[@]}")
target_dir=$(printf '%s' "$metadata" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
bin_dir="$target_dir/release"
[ -x "$bin_dir/rust-cutadapt" ] || fail_preflight "native binary missing at $bin_dir/rust-cutadapt; check Cargo target configuration"
run python3 rust-cutadapt/tests/check.py --bin "$bin_dir/rust-cutadapt"

if [ "$with_oracles" -eq 1 ]; then
    # Always test the binaries built above, not a stale default target directory.
    run "${oracle_prefix[@]}" bash tests/oracle/run_w13_oracle.sh --rust "$bin_dir/rust-fx"
    run "${oracle_prefix[@]}" python3 bench/check_boundaries.py --rust "$bin_dir/rust-fx" --seqkit "$SEQKIT_ORACLE" --out "$target_dir/readme-boundaries.json"
    run "${oracle_prefix[@]}" python3 tests/oracle/check_seqkit_exceptions.py --seqkit "$SEQKIT_ORACLE" --fq "$FQ_ORACLE" --rust "$bin_dir/rust-fx"
    run "${oracle_prefix[@]}" bash tests/oracle/run_qc_oracle.sh --rust "$bin_dir/rust-fastqc"
    run "${oracle_prefix[@]}" python3 tests/oracle/fastqc_adapters_diff.py --fastqc "$FASTQC_ORACLE" --rust "$bin_dir/rust-fastqc"
    run "${oracle_prefix[@]}" python3 tests/oracle/fastqc_tile_diff.py --fastqc "$FASTQC_ORACLE" --rust "$bin_dir/rust-fastqc"
    run "${oracle_prefix[@]}" python3 tests/oracle/fastqc_kmer_limits_diff.py --fastqc "$FASTQC_ORACLE" --rust "$bin_dir/rust-fastqc"
    run "${oracle_prefix[@]}" bash tests/oracle/run_qc_oracle.sh --self-test
    printf '\nQUALITY PASS (base + FASTX/QC oracles)\n'
else
    printf '\nQUALITY PASS (base only; external oracles NOT RUN)\n'
fi
