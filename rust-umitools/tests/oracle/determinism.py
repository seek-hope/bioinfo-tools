#!/usr/bin/env python3
"""Determinism suite: one input, different concurrency, identical bytes.

Each variant runs the same command with a different thread count or sort budget.
`RUST_UMITOOLS_MAX_FANIN=2` forces the multi-pass merge path with a small input.
All outputs are compared as decompressed BAM (header + records) or raw TSV.
"""

from __future__ import annotations

import gzip
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10000000\n"
ALPHABET = "ACGT"


def umi_at(index: int) -> str:
    """Distinct 8 nt UMIs at Hamming distance two from each other."""
    return "".join(ALPHABET[(index >> (2 * (7 - position))) & 3] for position in range(8))


def record(name: str, umi: str, pos: int, seq: str = "A" * 50) -> str:
    return f"{name}_{umi}\t0\tchr1\t{pos}\t60\t50M\t*\t0\t0\t{seq}\t{'I' * len(seq)}\n"


def build_fixture(work: Path) -> Path:
    rows = []
    for position in range(0, 3000, 10):
        for family in range(5):
            umi = umi_at(family + (position // 10) % 3)
            for _ in range(20):
                rows.append(record(f"r{len(rows)}", umi, position + 1))
    sam = work / "det.sam"
    bam = work / "det.bam"
    sam.write_text(HEADER + "".join(rows))
    subprocess.run(["samtools", "sort", "-o", str(bam), str(sam)], check=True, capture_output=True)
    subprocess.run(["samtools", "index", str(bam)], check=True, capture_output=True)
    return bam


def same_bytes(left: Path, right: Path) -> int | None:
    offset = 0
    with gzip.open(left, "rb") as a, gzip.open(right, "rb") as b:
        while True:
            x = a.read(1 << 20)
            y = b.read(1 << 20)
            if x != y:
                for i in range(min(len(x), len(y))):
                    if x[i] != y[i]:
                        return offset + i
                return offset + min(len(x), len(y))
            offset += len(x)
            if not x:
                return None


def run(args: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(BINARY), *args],
        capture_output=True,
        text=True,
        env={**os.environ, **(env or {})},
        check=False,
    )


def main() -> int:
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    failures = 0
    with tempfile.TemporaryDirectory(prefix="umitools-det-") as directory:
        work = Path(directory)
        bam = build_fixture(work)
        base = work / "threads1.bam"
        result = run(["dedup", "-I", str(bam), "-S", str(base), "--random-seed", "1", "--threads", "1"])
        if result.returncode != 0:
            print(f"FAIL  baseline dedup exit={result.returncode} {result.stderr[:200]}")
            return 1
        print("PASS  baseline dedup (threads=1)")

        variants = [
            ("threads=2", ["--threads", "2"], {}),
            ("threads=8", ["--threads", "8"], {}),
            ("threads=16", ["--threads", "16"], {}),
            (
                "spill multi-run (fan-in 32)",
                ["--threads", "4", "--memory-limit", "64MiB"],
                {},
            ),
            (
                "sort low-memory",
                ["--threads", "4", "--sort-strategy", "low-memory"],
                {},
            ),
            (
                "sort high-throughput",
                ["--threads", "4", "--sort-strategy", "high-throughput"],
                {},
            ),
            (
                "spill multi-pass (fan-in 2)",
                ["--threads", "4", "--memory-limit", "64MiB"],
                {"RUST_UMITOOLS_MAX_FANIN": "2"},
            ),
        ]
        for label, extra, env in variants:
            out = work / f"det_{abs(hash(label))}.bam"
            result = run(
                ["dedup", "-I", str(bam), "-S", str(out), "--random-seed", "1", *extra], env
            )
            if result.returncode != 0:
                print(f"FAIL  dedup {label} exit={result.returncode} {result.stderr[:200]}")
                failures += 1
                continue
            offset = same_bytes(base, out)
            if offset is None:
                print(f"PASS  dedup {label} matches baseline")
            else:
                print(f"FAIL  dedup {label} differs at decompressed offset {offset}")
                failures += 1

        group_base = work / "group1.bam"
        group_tsv_base = work / "group1.tsv"
        result = run(
            ["group", "-I", str(bam), "-S", str(group_base), "--output-bam",
             "--group-out", str(group_tsv_base), "--threads", "1"]
        )
        if result.returncode != 0:
            print(f"FAIL  baseline group exit={result.returncode} {result.stderr[:200]}")
            return 1
        for label, threads in [("threads=4", "4"), ("threads=16", "16")]:
            out = work / f"group_{threads}.bam"
            tsv = work / f"group_{threads}.tsv"
            result = run(
                ["group", "-I", str(bam), "-S", str(out), "--output-bam",
                 "--group-out", str(tsv), "--threads", threads]
            )
            if result.returncode != 0:
                print(f"FAIL  group {label} exit={result.returncode} {result.stderr[:200]}")
                failures += 1
                continue
            bam_offset = same_bytes(group_base, out)
            tsv_ok = group_tsv_base.read_bytes() == tsv.read_bytes()
            if bam_offset is None and tsv_ok:
                print(f"PASS  group {label} matches baseline")
            else:
                print(f"FAIL  group {label} bam_offset={bam_offset} tsv_equal={tsv_ok}")
                failures += 1

    print(f"\n{'FAILED' if failures else 'deterministic'}: {failures} mismatches")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
