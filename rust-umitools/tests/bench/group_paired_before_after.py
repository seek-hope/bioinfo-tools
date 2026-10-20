#!/usr/bin/env python3
"""Same-machine before/after measurements for the group and paired work.

Baselines are built from the commits immediately before each change:

  group  322dd1f  (parent of 9a29b22 "move group records instead of cloning")
  paired 558da69  (parent of 103e47e "R0: register mates after the representative")

Usage:
  python3 tests/bench/group_paired_before_after.py \
    --group-baseline /tmp/base-group-target/release/rust-umitools \
    --paired-baseline /tmp/base-paired-target/release/rust-umitools \
    --current "$CARGO_TARGET_DIR/release/rust-umitools" \
    --out docs/bench/group_paired_before_after.json

The script builds three fixtures with samtools, measures each binary with
/usr/bin/time -v (wall, user+system CPU, peak RSS), repeats the performance
cases three times, and records records/bytes for the paired cases. The JSON
report contains the method, fixture description, baseline commits and binary
checksums, per-run values and medians.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:10000000\n"
ALPHABET = "ACGT"


def umi_at(index: int) -> str:
    return "".join(ALPHABET[(index >> (2 * position)) & 3] for position in range(8))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def samtools_sort(work: Path, name: str, body: str) -> Path:
    sam = work / f"{name}.sam"
    bam = work / f"{name}.bam"
    sam.write_text(HEADER + body)
    subprocess.run(["samtools", "sort", "-o", str(bam), str(sam)], check=True, capture_output=True)
    subprocess.run(["samtools", "index", str(bam)], check=True, capture_output=True)
    return bam


def build_group_fixture(work: Path) -> Path:
    """200 positions x 20 UMIs x 50 reads; each UMI is deeply duplicated."""
    rows = []
    for position in range(200):
        base = 100 + position * 10
        for family in range(20):
            umi = umi_at(family)
            for _ in range(50):
                rows.append(
                    f"g{len(rows)}_{umi}\t0\tchr1\t{base}\t60\t10M\t*\t0\t0\t"
                    f"{'A' * 10}\t{'I' * 10}\n"
                )
    return samtools_sort(work, "group_fixture", "".join(rows))


def build_paired_fixture(work: Path, positions: int, duplicates: int) -> Path:
    """`duplicates` identical read pairs per position with the same UMI."""
    rows = []
    for index in range(positions):
        position = 100 + index * 10
        umi = "AAAAAAAA"
        for copy in range(duplicates):
            name = f"p{index}_{copy}_{umi}"
            rows.append(
                f"{name}\t99\tchr1\t{position}\t60\t10M\t=\t{position + 50}\t60\t"
                f"{'A' * 10}\t{'I' * 10}\n"
            )
            rows.append(
                f"{name}\t147\tchr1\t{position + 50}\t60\t10M\t=\t{position}\t-60\t"
                f"{'A' * 10}\t{'I' * 10}\n"
            )
    return samtools_sort(work, f"paired_{positions}_{duplicates}", "".join(rows))


def parse_time(text: str) -> dict:
    def grab(pattern: str, cast=float):
        match = re.search(pattern, text)
        return cast(match.group(1)) if match else None

    elapsed = re.search(r"Elapsed \(wall clock\) time.*?: (\d+):(\d+\.\d+)", text)
    if elapsed:
        wall = int(elapsed.group(1)) * 60 + float(elapsed.group(2))
    else:
        hours = re.search(r"Elapsed \(wall clock\) time.*?: (\d+):(\d+):(\d+\.\d+)", text)
        wall = (
            int(hours.group(1)) * 3600 + int(hours.group(2)) * 60 + float(hours.group(3))
            if hours
            else None
        )
    user = grab(r"User time \(seconds\): ([\d.]+)") or 0.0
    system = grab(r"System time \(seconds\): ([\d.]+)") or 0.0
    return {
        "wall_seconds": wall,
        "cpu_seconds": round(user + system, 4),
        "peak_rss_kb": grab(r"Maximum resident set size \(kbytes\): (\d+)", int),
    }


def supports_quiet(binary: Path) -> bool:
    """Older baselines predate --quiet; probe the help text once."""
    result = subprocess.run(
        [str(binary), "dedup", "--help"], capture_output=True, text=True, check=False
    )
    return "--quiet" in (result.stdout + result.stderr)


def measure(command: list[str], reps: int) -> list[dict]:
    runs = []
    for _ in range(reps):
        result = subprocess.run(
            ["/usr/bin/time", "-v", *command], capture_output=True, text=True, check=False
        )
        metrics = parse_time(result.stderr)
        metrics["exit"] = result.returncode
        runs.append(metrics)
    return runs


def summarize(runs: list[dict]) -> dict:
    successful = [run for run in runs if run.get("exit") == 0]
    if not successful:
        return {"exit": runs[0].get("exit"), "completed": False}
    summary = {"completed": True, "runs": len(successful)}
    for key in ("wall_seconds", "cpu_seconds", "peak_rss_kb"):
        values = [run[key] for run in successful if run.get(key) is not None]
        if values:
            summary[key] = round(statistics.median(values), 4)
            summary[key + "_all"] = values
    return summary


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--group-baseline", required=True, type=Path)
    parser.add_argument("--paired-baseline", required=True, type=Path)
    parser.add_argument("--current", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--reps", type=int, default=3)
    args = parser.parse_args()
    for binary in (args.group_baseline, args.paired_baseline, args.current):
        if not binary.exists():
            print(f"ERROR: {binary} not built", file=sys.stderr)
            return 2
    if shutil.which("samtools") is None:
        print("ERROR: samtools is required to build fixtures", file=sys.stderr)
        return 2

    with tempfile.TemporaryDirectory(prefix="umitools-before-after-") as directory:
        work = Path(directory)
        group_bam = build_group_fixture(work)
        paired_bam = build_paired_fixture(work, positions=20_000, duplicates=3)
        large_bam = build_paired_fixture(work, positions=150_000, duplicates=1)

        def run_group(binary: Path, label: str) -> dict:
            out_bam = work / f"group_{label}.bam"
            out_tsv = work / f"group_{label}.tsv"
            command = [
                str(binary), "group", "-I", str(group_bam), "-S", str(out_bam),
                "--output-bam", "--group-out", str(out_tsv),
            ]
            if supports_quiet(binary):
                command.append("--quiet")
            runs = measure(command, args.reps)
            summary = summarize(runs)
            if summary.get("completed"):
                summary["output_bytes"] = out_bam.stat().st_size + out_tsv.stat().st_size
                summary["rows"] = sum(1 for _ in out_tsv.open()) - 1
            return {"binary": str(binary), "sha256": sha256(binary), "command": " ".join(command), **summary}

        def run_paired(binary: Path, bam: Path, label: str, extra: list[str], reps: int) -> dict:
            out_bam = work / f"paired_{label}.bam"
            command = [
                str(binary), "dedup", "-I", str(bam), "-S", str(out_bam),
                "--paired", "--random-seed", "1", *extra,
            ]
            if supports_quiet(binary):
                command.append("--quiet")
            runs = measure(command, reps)
            summary = summarize(runs)
            if summary.get("completed"):
                summary["output_bytes"] = out_bam.stat().st_size
                records = subprocess.run(
                    ["samtools", "view", "-c", str(out_bam)], capture_output=True, text=True
                ).stdout.strip()
                summary["output_records"] = int(records or 0)
            return {"binary": str(binary), "sha256": sha256(binary), "command": " ".join(command), **summary}

        report = {
            "method": {
                "machine": "same machine as docs/BENCHMARK.md",
                "tool": "/usr/bin/time -v parsed for wall, user+system CPU and peak RSS",
                "repetitions": args.reps,
                "group_fixture": "200 positions x 20 UMIs x 50 reads (depth 50 per UMI)",
                "paired_fixture": "20,000 positions x 3 identical read pairs (same UMI)",
                "large_mate_fixture": "150,000 read pairs with --memory-limit 64MiB",
                "group_baseline_commit": "322dd1f (parent of 9a29b22, group cloned records)",
                "paired_baseline_commit": "558da69 (parent of 103e47e, mates registered before clustering)",
                "current_commit": subprocess.run(
                    ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True
                ).stdout.strip(),
            },
            "cases": [
                {
                    "name": "group_move_records",
                    "baseline": run_group(args.group_baseline, "baseline"),
                    "current": run_group(args.current, "current"),
                },
                {
                    "name": "paired_mate_registration",
                    "baseline": run_paired(args.paired_baseline, paired_bam, "baseline", [], args.reps),
                    "current": run_paired(args.current, paired_bam, "current", [], args.reps),
                },
                {
                    "name": "paired_large_mate_budget",
                    "baseline": run_paired(
                        args.paired_baseline, large_bam, "baseline_large",
                        ["--memory-limit", "64MiB"], 1,
                    ),
                    "current": run_paired(
                        args.current, large_bam, "current_large",
                        ["--memory-limit", "64MiB"], 1,
                    ),
                },
            ],
        }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    for case in report["cases"]:
        print(f"{case['name']}: baseline={case['baseline']} current={case['current']}")
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
