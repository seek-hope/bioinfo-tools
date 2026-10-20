#!/usr/bin/env python3
"""Randomised differential test: generated inputs, upstream cross-check.

For each trial a seeded generator builds a small BAM with several positions,
UMI families at Hamming distance one and two, both strands and varying counts.
The same command runs through rust-umitools and UMI-tools and the decompressed
BAM bytes must match. For directional and cluster a Python brute-force
component count is compared with the number of records both tools emit, so the
oracle itself is checked too.

A failing trial is written to tests/fixtures/ so it becomes a permanent,
minimal reproduction instead of a transient failure.
"""

from __future__ import annotations

import gzip
import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:1000000\n"
METHODS = ["unique", "percentile", "cluster", "adjacency", "directional"]
ALPHABET = "ACGT"


def hamming(left: str, right: str) -> int:
    return sum(a != b for a, b in zip(left, right))


def random_umi(rng: random.Random, length: int = 8) -> str:
    return "".join(rng.choice(ALPHABET) for _ in range(length))


def generate(rng: random.Random) -> list[tuple[int, bool, str, bool]]:
    reads: list[tuple[int, bool, str, bool]] = []
    positions = sorted(rng.sample(range(100, 5000, 100), rng.randint(2, 4)))
    for position in positions:
        for reverse in (False, True):
            if rng.random() < 0.25:
                continue
            base = random_umi(rng)
            families = [base]
            for _ in range(rng.randint(0, 3)):
                mutated = list(base)
                for _ in range(rng.randint(1, 2)):
                    mutated[rng.randrange(8)] = rng.choice(ALPHABET)
                families.append("".join(mutated))
            for umi in families:
                for _ in range(rng.randint(1, 5)):
                    reads.append((position, reverse, umi, rng.random() < 0.5))
    return reads


def to_sam(reads: list[tuple[int, bool, str, bool]]) -> str:
    rows = []
    for index, (position, reverse, umi, low_mapq) in enumerate(reads):
        flag = 16 if reverse else 0
        mapq = 30 if low_mapq else 60
        rows.append(
            f"r{index}_{umi}\t{flag}\tchr1\t{position}\t{mapq}\t10M\t*\t0\t0\t"
            f"{'A' * 10}\t{'I' * 10}\n"
        )
    return HEADER + "".join(rows)


def brute_force(reads: list[tuple[int, bool, str, bool]], method: str, threshold: int) -> int:
    """Independent reference: directed reachability (directional) or components."""
    counts: dict[tuple[int, bool, str], int] = {}
    for position, reverse, umi, _ in reads:
        counts[(position, reverse, umi)] = counts.get((position, reverse, umi), 0) + 1
    total = 0
    for position in {key[0] for key in counts}:
        for reverse in (False, True):
            entries = [(umi, count) for (pos, strand, umi), count in counts.items()
                       if pos == position and strand == reverse]
            if not entries:
                continue
            n = len(entries)
            if method == "cluster":
                parent = list(range(n))

                def find(index: int) -> int:
                    while parent[index] != index:
                        parent[index] = parent[parent[index]]
                        index = parent[index]
                    return index

                for i in range(n):
                    for j in range(i + 1, n):
                        if hamming(entries[i][0], entries[j][0]) <= threshold:
                            left, right = find(i), find(j)
                            if left != right:
                                parent[right] = left
                total += len({find(index) for index in range(n)})
                continue

            # Directional: an edge only points from the higher count to the
            # lower one, so groups are one-way reachability components seeded
            # from the highest count. This is not symmetric union-find.
            order = sorted(range(n), key=lambda index: -entries[index][1])
            visited = [False] * n
            for seed in order:
                if visited[seed]:
                    continue
                total += 1
                visited[seed] = True
                stack = [seed]
                while stack:
                    node = stack.pop()
                    for other in range(n):
                        if visited[other] or other == node:
                            continue
                        if hamming(entries[node][0], entries[other][0]) > threshold:
                            continue
                        if entries[node][1] + 1 >= 2 * entries[other][1]:
                            visited[other] = True
                            stack.append(other)
    return total


def decompressed(path: Path) -> bytes:
    with gzip.open(path, "rb") as handle:
        return handle.read()


def count_records(path: Path) -> int:
    result = subprocess.run(["samtools", "view", "-c", str(path)], capture_output=True, text=True)
    return int(result.stdout.strip() or 0)


def main() -> int:
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    rng = random.Random(int(os.environ.get("RANDOM_DIFF_SEED", "7")))
    trials = int(os.environ.get("RANDOM_DIFF_TRIALS", "12"))
    failures = 0
    fixtures = ROOT / "tests" / "fixtures"
    fixtures.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="umitools-rand-") as directory:
        work = Path(directory)
        for trial in range(trials):
            reads = generate(rng)
            method = rng.choice(METHODS)
            threshold = rng.choice([0, 1, 2])
            seed = rng.randrange(1, 1000)
            sam = work / f"trial{trial}.sam"
            bam = work / f"trial{trial}.bam"
            sam.write_text(to_sam(reads))
            subprocess.run(["samtools", "sort", "-o", str(bam), str(sam)], check=True,
                           capture_output=True)
            subprocess.run(["samtools", "index", str(bam)], check=True, capture_output=True)
            outputs = {}
            for label, tool in (("umi", "umi_tools"), ("rust", str(BINARY))):
                out = work / f"trial{trial}_{label}.bam"
                result = subprocess.run(
                    [tool, "dedup", "-I", str(bam), "-S", str(out), "--method", method,
                     "--edit-distance-threshold", str(threshold), "--random-seed", str(seed)],
                    capture_output=True, text=True,
                )
                if result.returncode != 0:
                    print(f"FAIL  trial {trial} {label} exit={result.returncode} "
                          f"{result.stderr.strip()[-160:]}")
                    failures += 1
                    sam.rename(fixtures / f"random_fail_{trial}.sam")
                    break
                outputs[label] = out
            else:
                if decompressed(outputs["umi"]) != decompressed(outputs["rust"]):
                    print(f"FAIL  trial {trial} method={method} threshold={threshold} "
                          f"seed={seed}: bytes differ")
                    failures += 1
                    sam.rename(fixtures / f"random_fail_{trial}.sam")
                    continue
                if method in ("directional", "cluster"):
                    expected = brute_force(reads, method, threshold)
                    for label, path in outputs.items():
                        found = count_records(path)
                        if found != expected:
                            print(f"FAIL  trial {trial} {label} {method}: brute={expected} "
                                  f"tool={found}")
                            failures += 1
                if failures == 0:
                    print(f"PASS  trial {trial} method={method} k={threshold} "
                          f"reads={len(reads)}")
    print(f"\n{'FAILED' if failures else 'random differential ok'}: {failures} failures "
          f"over {trials} trials")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
