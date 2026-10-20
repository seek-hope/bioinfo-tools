#!/usr/bin/env python3
"""Audit the effect of Python `set` iteration order on adjacency grouping.

Upstream emits the non-lead members of an adjacency group in `set` iteration
order, which depends on PYTHONHASHSEED. This audit answers two questions with
evidence:

  1. does upstream output order change between hash seeds?      (expected: yes)
  2. does the *membership* or the dedup representative change?   (expected: no)
  3. is rust-umitools output stable across runs?                 (expected: yes)
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:100000\n"
ALPHABET = "ACGT"


def satellite(index: int) -> str:
    """One substitution away from AAAAAAAA, cycling through positions/bases."""
    position = index % 8
    base = ALPHABET[1 + (index // 8) % 3]
    return "A" * position + base + "A" * (7 - position)


def body() -> str:
    rows = []
    for _ in range(60):
        rows.append(f"lead_{'A' * 8}\t0\tchr1\t100\t60\t10M\t*\t0\t0\t{'A' * 10}\t{'I' * 10}\n")
    for index in range(24):
        umi = satellite(index)
        rows.append(f"sat{index}_{umi}\t0\tchr1\t100\t60\t10M\t*\t0\t0\t{'A' * 10}\t{'I' * 10}\n")
    # A second, unrelated bundle to keep the table non-trivial.
    rows.append(f"other_{'T' * 8}\t0\tchr1\t5000\t60\t10M\t*\t0\t0\t{'A' * 10}\t{'I' * 10}\n")
    return "".join(rows)


def run(command: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        command, capture_output=True, text=True, env={**os.environ, **(env or {})}, check=False
    )


def rows_without_header(path: Path) -> set[str]:
    lines = path.read_text().splitlines()
    return set(lines[1:]) if lines else set()


def main() -> int:
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="umitools-adj-") as directory:
        work = Path(directory)
        sam = work / "in.sam"
        bam = work / "in.bam"
        sam.write_text(HEADER + body())
        subprocess.run(["samtools", "sort", "-o", str(bam), str(sam)], check=True, capture_output=True)
        subprocess.run(["samtools", "index", str(bam)], check=True, capture_output=True)

        rust_runs = []
        for index in range(3):
            out = work / f"rust{index}.tsv"
            result = run([str(BINARY), "group", "-I", str(bam), "--group-out", str(out),
                          "--method", "adjacency"])
            if result.returncode != 0:
                print(f"FAIL rust run {index}: {result.stderr[-300:]}")
                return 1
            rust_runs.append(out.read_text())

        seeds = {}
        for seed in ("0", "1"):
            out = work / f"umi_{seed}.tsv"
            result = run(["umi_tools", "group", "-I", str(bam), "--group-out", str(out),
                          "--method", "adjacency"], {"PYTHONHASHSEED": seed})
            if result.returncode != 0:
                print(f"FAIL umi_tools seed {seed}: {result.stderr[-300:]}")
                return 1
            seeds[seed] = out

        rust_stable = len(set(rust_runs)) == 1
        upstream_order_stable = seeds["0"].read_bytes() == seeds["1"].read_bytes()
        upstream_membership_stable = rows_without_header(seeds["0"]) == rows_without_header(seeds["1"])
        rust_matches_membership = rows_without_header(seeds["0"]) == rows_without_header(
            work / "rust0.tsv"
        )

        print(f"rust output stable across 3 runs      : {rust_stable}")
        print(f"upstream byte order stable across seeds: {upstream_order_stable}")
        print(f"upstream membership stable across seeds: {upstream_membership_stable}")
        print(f"rust membership equals upstream        : {rust_matches_membership}")

        ok = rust_stable and upstream_membership_stable and rust_matches_membership
        print(
            "\nconclusion: "
            + (
                "set order only changes row order; membership and representatives match"
                if ok
                else "membership differs, investigate before claiming compatibility"
            )
        )
        return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
