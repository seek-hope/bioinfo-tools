#!/usr/bin/env bash
# M0 gate: W0-1 (README truth) and W0-4 (governance cleanup).
#
# Run from the rust-cutadapt repository root:
#   bash tests/gate/m0.sh
#
# This gate is intentionally about the documents and workspace state. The
# 119-case differential suite remains the behavioural authority.

set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT" || exit 1

fail=0
pass() { printf 'PASS  %s\n' "$*"; }
problem() { printf 'FAIL  %s\n' "$*"; fail=1; }

if [ ! -x target/release/rust-cutadapt ]; then
    if ! cargo build --release --offline >/dev/null 2>&1; then
        printf 'FAIL  cannot build target/release/rust-cutadapt\n'
        exit 1
    fi
fi
BIN=target/release/rust-cutadapt

options=$("$BIN" --help 2>/dev/null | grep -Ec '^  +-')
if [ "$options" -eq 60 ]; then
    pass "help exposes 60 options"
else
    problem "help exposes $options options, expected 60"
fi

if grep -Fq '| Documented cutadapt 4.6 options | 60, all implemented |' README.md; then
    pass "README states the 60-option surface"
else
    problem "README does not state the 60-option surface"
fi

if grep -Fq '119 / 119 pass' README.md; then
    pass "README states the differential result"
else
    problem "README does not state the differential result"
fi

for f in docs/FULL-COMPATIBILITY.md docs/HISTORY.md docs/RELEASE-FREEZE.md \
         docs/adr/0001-dependency-policy.md; do
    if [ -f "$f" ]; then
        pass "present: $f"
    else
        problem "missing: $f"
    fi
done

if [ ! -e ../.work/src ]; then
    pass ".work/src is absent"
else
    problem ".work/src still exists"
fi

if find ../.work -name Cargo.toml -print -quit 2>/dev/null | grep -q .; then
    problem ".work still contains a Cargo source tree"
else
    pass ".work contains no duplicate Cargo source tree"
fi

if [ -z "$(git status --porcelain)" ]; then
    pass "rust-cutadapt working tree is clean"
else
    problem "rust-cutadapt working tree is dirty"
fi

if [ -z "$(git -C ../rust-umitools status --porcelain 2>/dev/null)" ]; then
    pass "rust-umitools working tree is clean"
else
    problem "rust-umitools working tree is dirty or missing"
fi

if [ -z "$(git tag -l)" ]; then
    pass "no source tag exists"
else
    problem "a source tag exists during the freeze"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf '@r1\nACGTACGTACGT\n+\nIIIIIIIIIIII\n@r2\nTTTTACGT\n+\nIIIIIIII\n' > "$tmp/r1.fastq"
printf '@r1\nACGTACGTACGT\n+\nIIIIIIIIIIII\n@r2\nTTTTACGT\n+\nIIIIIIII\n' > "$tmp/r2.fastq"

expect_exit() {
    expected=$1
    name=$2
    shift 2
    if "$@" >/dev/null 2>"$tmp/stderr"; then
        actual=0
    else
        actual=$?
    fi
    if [ "$actual" -eq "$expected" ]; then
        pass "probe $name exits $expected"
    else
        problem "probe $name exits $actual, expected $expected"
    fi
}

expect_exit 0 help "$BIN" --help
expect_exit 0 version "$BIN" --version
expect_exit 0 baseline "$BIN" --quiet -a AGATCGGAAGAGC -m 10 -o "$tmp/out.fastq" "$tmp/r1.fastq"
expect_exit 0 front "$BIN" --quiet -g ACGT -o "$tmp/front.fastq" "$tmp/r1.fastq"
expect_exit 0 quality "$BIN" --quiet -q 20 -o "$tmp/q.fastq" "$tmp/r1.fastq"
expect_exit 0 cut "$BIN" --quiet -u 4 -o "$tmp/u.fastq" "$tmp/r1.fastq"
expect_exit 0 json "$BIN" --quiet -a ACGT --json "$tmp/report.json" -o "$tmp/json.fastq" "$tmp/r1.fastq"
expect_exit 0 cores "$BIN" --quiet -j 2 -a ACGT -o "$tmp/j.fastq" "$tmp/r1.fastq"
expect_exit 0 wildcard "$BIN" --quiet -N -a ACGT -o "$tmp/n.fastq" "$tmp/r1.fastq"
expect_exit 0 polya "$BIN" --quiet --poly-a -o "$tmp/poly.fastq" "$tmp/r1.fastq"
expect_exit 0 paired "$BIN" --quiet -a ACGT -A ACGT -p "$tmp/p2.fastq" -o "$tmp/p1.fastq" "$tmp/r1.fastq" "$tmp/r2.fastq"
expect_exit 2 unknown "$BIN" --not-a-real-option

if [ "$fail" -eq 0 ]; then
    printf '\nM0 PASS\n'
else
    printf '\nM0 FAIL\n'
fi
exit "$fail"
