#!/usr/bin/env python3
"""Bounded-memory checks: forced partitioning must not change the output.

The bundler closes its window at position boundaries when the live UMI count
crosses a threshold, so memory stays bounded without spilling. This suite
forces a tiny threshold and compares the decompressed BAM with the default run.
A single oversized position cannot be split and must fail with a resource error
before writing.
"""

from __future__ import annotations

import gzip
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10000000\n"
ALPHABET = "ACGT"


def record(name: str, umi: str, pos: int) -> str:
    return f"{name}_{umi}\t0\tchr1\t{pos}\t60\t10M\t*\t0\t0\t{'A' * 10}\t{'I' * 10}\n"


def build(work: Path, name: str, body: str) -> Path:
    sam = work / f"{name}.sam"
    bam = work / f"{name}.bam"
    sam.write_text(HEADER + body)
    subprocess.run(["samtools", "sort", "-o", str(bam), str(sam)], check=True, capture_output=True)
    subprocess.run(["samtools", "index", str(bam)], check=True, capture_output=True)
    return bam


def run(args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(BINARY), "dedup", *args], capture_output=True, text=True,
        env={**os.environ, **env}, check=False,
    )


def equal(left: Path, right: Path) -> bool:
    with gzip.open(left, "rb") as a, gzip.open(right, "rb") as b:
        return a.read() == b.read()


def main() -> int:
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    failures = 0
    with tempfile.TemporaryDirectory(prefix="umitools-spill-") as directory:
        work = Path(directory)
        rows = []
        for position in range(0, 3000, 10):
            for family in range(5):
                umi = "".join(ALPHABET[(family + position // 10) % 4] for _ in range(8))
                for _ in range(20):
                    rows.append(record(f"r{len(rows)}", umi, position + 1))
        wide = build(work, "wide", "".join(rows))

        base = work / "base.bam"
        result = run(["-I", str(wide), "-S", str(base), "--random-seed", "1", "--quiet"], {})
        if result.returncode != 0:
            print(f"FAIL  baseline exit={result.returncode} {result.stderr[-160:]}")
            return 1

        for label, env in [
            ("partition 64", {"RUST_UMITOOLS_PARTITION_UMIS": "64"}),
            ("partition 64, low memory", {
                "RUST_UMITOOLS_PARTITION_UMIS": "64",
                "RUST_UMITOOLS_MAX_PENDING_UMIS": "4096",
            }),
        ]:
            out = work / f"bounded_{abs(hash(label))}.bam"
            result = run(["-I", str(wide), "-S", str(out), "--random-seed", "1", "--quiet"], env)
            if result.returncode != 0:
                print(f"FAIL  {label} exit={result.returncode} {result.stderr[-160:]}")
                failures += 1
                continue
            if equal(base, out):
                print(f"PASS  {label} matches the default path")
            else:
                print(f"FAIL  {label} differs from the default path")
                failures += 1

        # Paired mate requests spill to disk and re-join in chunks.
        pair_rows = []
        for index in range(200):
            position = 100 + index * 10
            pair_rows.append(
                f"p{index}_AAAAAAAA\t99\tchr1\t{position}\t60\t10M\t=\t{position + 50}\t60\t"
                f"{'A' * 10}\t{'I' * 10}\n"
            )
            pair_rows.append(
                f"p{index}_AAAAAAAA\t147\tchr1\t{position + 50}\t60\t10M\t=\t{position}\t-60\t"
                f"{'A' * 10}\t{'I' * 10}\n"
            )
        paired = build(work, "paired", "".join(pair_rows))
        pair_base = work / "pair_base.bam"
        result = run(["-I", str(paired), "-S", str(pair_base), "--paired",
                      "--random-seed", "1", "--quiet"], {})
        pair_spill = work / "pair_spill.bam"
        result = run(["-I", str(paired), "-S", str(pair_spill), "--paired",
                      "--random-seed", "1", "--quiet"],
                     {"RUST_UMITOOLS_MAX_MATES": "8"})
        if result.returncode == 0 and equal(pair_base, pair_spill):
            print("PASS  spilled mate join matches the default path")
        else:
            print(f"FAIL  spilled mate join exit={result.returncode}")
            failures += 1

        nosort = work / "pair_nosort.bam"
        result = run(["-I", str(paired), "-S", str(nosort), "--paired", "--random-seed", "1",
                      "--no-sort-output", "--quiet"], {"RUST_UMITOOLS_MAX_MATES": "8"})
        if result.returncode == 2 and not nosort.exists():
            print("PASS  no-sort with spilled mates is refused")
        else:
            print(f"FAIL  no-sort with spilled mates exit={result.returncode}")
            failures += 1

        # count gene aggregation falls back to gene partitions on overflow
        gene_rows = []
        position = 100
        for gene in range(100):
            for umi_index in range(20):
                umi = "".join(ALPHABET[(gene + umi_index + i) % 4] for i in range(8))
                for copy in range(2):
                    gene_rows.append(
                        f"g{gene}_{umi_index}_{copy}\t0\tchr1\t{position}\t60\t10M\t*\t0\t0\t"
                        f"{'A' * 10}\t{'I' * 10}\tGX:Z:G{gene}\n"
                    )
                position += 1
        genes = build(work, "genes", "".join(gene_rows))
        base_tsv = work / "count_base.tsv"
        result = subprocess.run(
            [str(BINARY), "count", "-I", str(genes), "-S", str(base_tsv),
             "--per-gene", "--gene-tag", "GX", "--quiet"],
            capture_output=True, text=True, check=False,
        )
        if result.returncode != 0:
            print(f"FAIL  count baseline exit={result.returncode}")
            failures += 1
        else:
            fallback_tsv = work / "count_fallback.tsv"
            result = subprocess.run(
                [str(BINARY), "count", "-I", str(genes), "-S", str(fallback_tsv),
                 "--per-gene", "--gene-tag", "GX", "--quiet"],
                capture_output=True, text=True, check=False,
                env={**os.environ, "RUST_UMITOOLS_MAX_PENDING_UMIS": "500"},
            )
            if result.returncode == 0 and base_tsv.read_bytes() == fallback_tsv.read_bytes():
                print("PASS  gene partition fallback matches the default path")
            else:
                print(f"FAIL  gene partition fallback exit={result.returncode}")
                failures += 1

        # oversized-bundle disk path matches the in-memory clustering path
        disk_rows = []
        for index in range(400):
            umi = "".join(ALPHABET[(index >> (2 * position)) & 3] for position in range(8))
            disk_rows.append(
                f"x{index}_{umi}\t0\tchr1\t100\t60\t10M\t*\t0\t0\t{'A' * 10}\t{'I' * 10}\n"
            )
        disk_bam = build(work, "disk_bundle", "".join(disk_rows))
        for method in ("cluster", "directional", "adjacency"):
            base = work / f"disk_{method}_base.bam"
            run(["-I", str(disk_bam), "-S", str(base), "--random-seed", "1",
                 "--method", method, "--quiet"], {})
            forced = work / f"disk_{method}_forced.bam"
            result = run(["-I", str(disk_bam), "-S", str(forced), "--random-seed", "1",
                          "--method", method, "--quiet"],
                         {"RUST_UMITOOLS_DISK_UMIS": "64"})
            if result.returncode == 0 and equal(base, forced):
                print(f"PASS  disk bundle ({method}) matches the in-memory path")
            else:
                print(f"FAIL  disk bundle ({method}) exit={result.returncode}")
                failures += 1

        # One position with more UMIs than the window cannot be split.
        huge_rows = [record(f"h{i}", f"{i:08d}"[:8], 100) for i in range(2000)]
        huge = build(work, "huge", "".join(huge_rows))
        target = work / "huge_out.bam"
        result = run(
            ["-I", str(huge), "-S", str(target), "--random-seed", "1", "--quiet"],
            {"RUST_UMITOOLS_MAX_PENDING_UMIS": "1000"},
        )
        if result.returncode == 4 and not target.exists():
            print("PASS  oversized single bundle reports a resource error")
        else:
            print(f"FAIL  oversized bundle exit={result.returncode} exists={target.exists()}")
            failures += 1

    print(f"\n{'FAILED' if failures else 'bounded memory ok'}: {failures} failures")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
