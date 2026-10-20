#!/usr/bin/env python3
"""Benchmark suite with provenance and resource metrics.

Usage:
  python3 tests/bench/bench.py --smoke          # fast CI smoke (one repetition)
  python3 tests/bench/bench.py --out docs/bench/latest.json

The suite generates its own fixtures with samtools, measures wall time, child
CPU time, peak RSS, peak thread count, temporary-directory peak and output size,
and parses the tool's own stage timings. Every run records provenance (commit,
binary checksum, toolchain, hardware) so a number can be traced back.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:100000000\n"
ALPHABET = "ACGT"


def record(name: str, umi: str, pos: int, gene: str | None, seq: str = "A" * 60) -> str:
    extra = f"\tGX:Z:{gene}" if gene else ""
    return (
        f"{name}_{umi}\t0\tchr1\t{pos}\t60\t60M\t*\t0\t0\t{seq}\t{'I' * len(seq)}"
        f"{extra}\n"
    )


def umi_at(index: int) -> str:
    return "".join(ALPHABET[(index >> (2 * (7 - position))) & 3] for position in range(8))


def build_fixtures(work: Path, reads: int) -> tuple[Path, Path, Path]:
    per_position = 40
    rows = []
    tagged = []
    position = 0
    while len(rows) < reads:
        for family in range(4):
            umi = umi_at(family + (position // 10) % 5)
            gene = f"G{position % 50}"
            for _ in range(per_position // 4):
                rows.append(record(f"r{len(rows)}", umi, position + 1, None))
                tagged.append(record(f"t{len(tagged)}", umi, position + 1, gene))
        position += 10
    plain_sam, plain_bam = work / "plain.sam", work / "plain.bam"
    plain_sam.write_text(HEADER + "".join(rows))
    subprocess.run(["samtools", "sort", "-o", str(plain_bam), str(plain_sam)],
                   check=True, capture_output=True)
    subprocess.run(["samtools", "index", str(plain_bam)], check=True, capture_output=True)
    tagged_sam, tagged_bam = work / "tagged.sam", work / "tagged.bam"
    tagged_sam.write_text(HEADER + "".join(tagged))
    subprocess.run(["samtools", "sort", "-o", str(tagged_bam), str(tagged_sam)],
                   check=True, capture_output=True)
    subprocess.run(["samtools", "index", str(tagged_bam)], check=True, capture_output=True)
    table = work / "reads.tsv"
    table.write_text("".join(f"read{i}_{umi_at(i % 7)}\tG{i % 50}\n" for i in range(reads // 4)))
    return plain_bam, tagged_bam, table


class Sampler(threading.Thread):
    """Poll RSS, thread count and temporary-directory size while a child runs."""

    def __init__(self, pid: int, tmpdir: Path) -> None:
        super().__init__(daemon=True)
        self.pid = pid
        self.tmpdir = tmpdir
        self.peak_rss_kb = 0
        self.peak_threads = 0
        self.peak_tmp = 0
        self.cpu_ticks = 0
        self.clk_tck = os.sysconf("SC_CLK_TCK")
        self._halt = threading.Event()

    def run(self) -> None:
        status = Path(f"/proc/{self.pid}/status")
        stat = Path(f"/proc/{self.pid}/stat")
        while not self._halt.is_set():
            try:
                text = status.read_text()
                rss = re.search(r"VmRSS:\s+(\d+)", text)
                threads = re.search(r"Threads:\s+(\d+)", text)
                if rss:
                    self.peak_rss_kb = max(self.peak_rss_kb, int(rss.group(1)))
                if threads:
                    self.peak_threads = max(self.peak_threads, int(threads.group(1)))
            except (FileNotFoundError, ProcessLookupError):
                pass
            try:
                text = stat.read_text()
                fields = text[text.rfind(")") + 2 :].split()
                ticks = int(fields[11]) + int(fields[12])
                self.cpu_ticks = max(self.cpu_ticks, ticks)
            except (FileNotFoundError, ProcessLookupError, IndexError, ValueError):
                pass
            used = 0
            for child in self.tmpdir.rglob("*"):
                try:
                    used += child.stat().st_size
                except OSError:
                    pass
            self.peak_tmp = max(self.peak_tmp, used)
            time.sleep(0.02)

    def stop(self) -> None:
        self._halt.set()
        self.join(timeout=1)


def measure(args: list[str], tmpdir: Path) -> dict:
    process = subprocess.Popen(
        [str(BINARY), *args], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True
    )
    sampler = Sampler(process.pid, tmpdir)
    sampler.start()
    started = time.perf_counter()
    _, stderr = process.communicate()
    wall = time.perf_counter() - started
    sampler.stop()
    if process.returncode != 0:
        raise RuntimeError(f"{args} failed: {stderr[-400:]}")
    stages = {}
    for name, value in re.findall(r"^(parse\+cluster|sort\+commit)\s*:\s*([0-9.]+)s", stderr, re.M):
        stages[name.replace("+", "_")] = float(value)
    return {
        "wall_seconds": round(wall, 4),
        "peak_rss_kb": sampler.peak_rss_kb,
        "threads_peak": sampler.peak_threads,
        "tmp_bytes_peak": sampler.peak_tmp,
        "cpu_seconds": round(sampler.cpu_ticks / sampler.clk_tck, 4),
        "stages": stages,
    }


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def tool_version(command: list[str]) -> str:
    try:
        result = subprocess.run(command, capture_output=True, text=True, check=False)
        return (result.stdout + result.stderr).strip().splitlines()[0][:120]
    except (FileNotFoundError, IndexError):
        return "unavailable"


def provenance() -> dict:
    try:
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=False
        ).stdout.strip()
    except OSError:
        commit = "unknown"
    cpu_model = "unknown"
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu_model = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    return {
        "commit": commit,
        "binary": str(BINARY),
        "binary_sha256": sha256(BINARY) if BINARY.exists() else "missing",
        "profile": "release",
        "rustc": tool_version(["rustc", "--version"]),
        "umi_tools": tool_version(["umi_tools", "--version"]),
        "samtools": tool_version(["samtools", "--version"]),
        "cpu_model": cpu_model,
        "logical_cpus": os.cpu_count(),
        "platform": platform.platform(),
        "python": platform.python_version(),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--smoke", action="store_true", help="one repetition, small input")
    parser.add_argument("--out", type=Path, help="write the JSON report here")
    parser.add_argument("--reps", type=int, default=None, help="repetitions per case")
    parser.add_argument("--hyperfine", action="store_true", help="also export hyperfine JSON per case")
    parser.add_argument("--big", type=Path, help="extra real BAM measured with dedup")
    args = parser.parse_args()
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    if shutil.which("samtools") is None:
        print("ERROR: samtools is required to build fixtures", file=sys.stderr)
        return 2

    reads = 20_000 if args.smoke else 200_000
    repetitions = 1 if args.smoke else (args.reps or 3)
    report: dict = {"provenance": provenance(), "cases": []}
    with tempfile.TemporaryDirectory(prefix="umitools-bench-") as directory:
        work = Path(directory)
        plain, tagged, table = build_fixtures(work, reads)
        tmpdir = work / "tmp"
        tmpdir.mkdir()
        dedup_out = work / "dedup.bam"
        group_bam = work / "group.bam"
        group_tsv = work / "group.tsv"
        count_out = work / "count.tsv"
        tab_out = work / "count_tab.tsv"
        cases = [
            ("dedup", ["dedup", "-I", str(plain), "-S", str(dedup_out), "--random-seed", "1"], [dedup_out]),
            ("group", ["group", "-I", str(plain), "-S", str(group_bam), "--output-bam",
                       "--group-out", str(group_tsv)], [group_bam, group_tsv]),
            ("count", ["count", "-I", str(tagged), "-S", str(count_out),
                       "--per-gene", "--gene-tag", "GX"], [count_out]),
            ("count_tab", ["count_tab", "-I", str(table), "-S", str(tab_out)], [tab_out]),
        ]
        if args.big:
            big_out = work / "big_dedup.bam"
            cases.append(("dedup_big", ["dedup", "-I", str(args.big), "-S", str(big_out),
                                        "--random-seed", "1"], [big_out]))
            big_count = work / "big_count.tsv"
            cases.append(("count_big", ["count", "-I", str(args.big), "-S", str(big_count),
                                        "--per-contig"], [big_count]))
            big_group = work / "big_group.bam"
            big_tsv = work / "big_group.tsv"
            cases.append(("group_big", ["group", "-I", str(args.big), "-S", str(big_group),
                                        "--output-bam", "--group-out", str(big_tsv)],
                          [big_group, big_tsv]))
        for strategy in ("low-memory", "high-throughput"):
            strategy_out = work / f"dedup_{strategy}.bam"
            cases.append(
                (
                    f"dedup_{strategy}",
                    ["dedup", "-I", str(plain), "-S", str(strategy_out),
                     "--random-seed", "1", "--sort-strategy", strategy],
                    [strategy_out],
                )
            )
        for name, command, outputs in cases:
            for output in outputs:
                output.unlink(missing_ok=True)
            runs = []
            for _ in range(repetitions):
                metrics = measure([*command, "--tmpdir", str(tmpdir)], tmpdir)
                for output in outputs:
                    if output.exists():
                        metrics.setdefault("output_bytes", 0)
                        metrics["output_bytes"] += output.stat().st_size
                runs.append(metrics)
                for child in tmpdir.iterdir():
                    if child.is_file():
                        child.unlink()
            best = min(run["wall_seconds"] for run in runs)
            if args.hyperfine and shutil.which("hyperfine"):
                export_dir = (args.out.parent / "hyperfine") if args.out else work
                export_dir.mkdir(parents=True, exist_ok=True)
                export = export_dir / f"{name}.json"
                subprocess.run(
                    ["hyperfine", "--warmup", "0", "--min-runs", "3", "--export-json", str(export),
                     " ".join(str(part) for part in [BINARY, *command, "--tmpdir", str(tmpdir)])],
                    check=False, capture_output=True,
                )
                if export.exists():
                    report.setdefault("hyperfine", {})[name] = str(export)
            report["cases"].append(
                {
                    "name": name,
                    "input_reads": reads,
                    "repetitions": repetitions,
                    "wall_seconds_min": best,
                    "wall_seconds_all": [run["wall_seconds"] for run in runs],
                    "cpu_seconds": round(
                        sum(run["cpu_seconds"] for run in runs) / len(runs), 4
                    ),
                    "peak_rss_kb": max(run["peak_rss_kb"] for run in runs),
                    "threads_peak": max(run["threads_peak"] for run in runs),
                    "tmp_bytes_peak": max(run["tmp_bytes_peak"] for run in runs),
                    "output_bytes": runs[0].get("output_bytes", 0),
                    "stages": runs[0]["stages"],
                }
            )
            print(
                f"{name:10s} wall={best:7.3f}s cpu={report['cases'][-1]['cpu_seconds']:7.3f}s "
                f"rss={report['cases'][-1]['peak_rss_kb']:8d}kB "
                f"threads={report['cases'][-1]['threads_peak']:3d} "
                f"tmp={report['cases'][-1]['tmp_bytes_peak']:10d}B"
            )

    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
        print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
