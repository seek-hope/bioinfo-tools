#!/usr/bin/env python3
"""Deterministic benchmark runner for the five bioinfo-tools CLIs.

Every timed command is preceded by input generation outside the measurement and is
followed by a correctness check on the same input. Results include provenance so a
number cannot be detached from the binary, input, hardware or command that made it.

This runner uses only the Python standard library and /usr/bin/time -v.
"""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_SIZES = [1, 10]
DEFAULT_REPEATS = 2


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_fastq(path: Path, records: int, seed: int) -> None:
    rng = random.Random(seed)
    adapter = "AGATCGGAAGAGC"
    bases = "ACGT"
    with path.open("w", encoding="ascii") as handle:
        for index in range(records):
            sequence = "".join(rng.choice(bases) for _ in range(100))
            if index % 10 == 0:
                sequence = sequence[:50] + adapter + sequence[50 + len(adapter):]
            handle.write(f"@read{index:07d}\n{sequence}\n+\n{'I' * len(sequence)}\n")


def write_sam(path: Path, records: int, seed: int) -> None:
    rng = random.Random(seed)
    bases = "ACGT"
    with path.open("w", encoding="ascii") as handle:
        handle.write("@HD\tVN:1.6\tSO:unsorted\n")
        handle.write("@SQ\tSN:chr1\tLN:1000000\n@SQ\tSN:chr2\tLN:1000000\n")
        for index in range(records):
            reference = "chr1" if index % 2 else "chr2"
            position = index // 3 + 1
            flag = 16 if index % 3 == 0 else 0
            umi = f"UMI{index % 3:05d}"
            sequence = "".join(rng.choice(bases) for _ in range(50))
            handle.write(
                f"r{index:07d}_{umi}\t{flag}\t{reference}\t{position}\t60\t50M\t*\t0\t0\t"
                f"{sequence}\t{'I' * 50}\tUB:Z:{umi}\n"
            )


def run_checked(command: list[str], cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True, check=False)


def prepare(workdir: Path, records: int, bins: dict[str, Path]) -> dict[str, Path]:
    fastq = workdir / "reads.fastq"
    sam = workdir / "reads.sam"
    write_fastq(fastq, records, 20261023)
    write_sam(sam, records, 20261023)

    unsorted = workdir / "unsorted.bam"
    result = run_checked([str(bins["bam"]), "view", "-b", "-o", str(unsorted), str(sam)], workdir)
    if result.returncode != 0:
        raise RuntimeError(f"rust-bam view failed: {result.stderr}")

    sorted_bam = workdir / "sorted.bam"
    result = run_checked([str(bins["bam"]), "sort", "-o", str(sorted_bam), str(unsorted)], workdir)
    if result.returncode != 0:
        raise RuntimeError(f"rust-bam sort failed: {result.stderr}")

    return {
        "fastq": fastq,
        "sam": sam,
        "unsorted": unsorted,
        "sorted": sorted_bam,
    }


class DiskSampler:
    """Sample the bytes under one temp directory while a command runs."""

    def __init__(self, path: Path):
        self.path = path
        self.peak = 0
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)

    def _size(self) -> int:
        total = 0
        for root, _, files in os.walk(self.path):
            for name in files:
                try:
                    total += (Path(root) / name).stat().st_size
                except OSError:
                    pass
        return total

    def _run(self) -> None:
        while not self.stop.is_set():
            self.peak = max(self.peak, self._size())
            time.sleep(0.02)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.stop.set()
        self.thread.join(timeout=2)
        return False


def parse_time(path: Path) -> dict:
    values = {"wall_s": None, "user_s": None, "sys_s": None, "max_rss_kb": None}
    if not path.is_file():
        return values
    for line in path.read_text(errors="replace").splitlines():
        if "Elapsed (wall clock) time" in line:
            raw = line.split("):", 1)[1].strip()
            parts = [float(part) for part in raw.split(":")]
            values["wall_s"] = parts[-1] + (parts[-2] * 60 if len(parts) > 1 else 0) + (
                parts[-3] * 3600 if len(parts) > 2 else 0
            )
        elif "User time (seconds):" in line:
            values["user_s"] = float(line.split(":", 1)[1])
        elif "System time (seconds):" in line:
            values["sys_s"] = float(line.split(":", 1)[1])
        elif "Maximum resident set size (kbytes):" in line:
            values["max_rss_kb"] = int(line.split(":", 1)[1])
    return values


def run_timed(
    command: list[str], workdir: Path, tempdir: Path, repeat: int
) -> dict:
    tempdir.mkdir(parents=True, exist_ok=True)
    stats = workdir / f"time-{repeat}.txt"
    stdout = workdir / f"stdout-{repeat}.txt"
    stderr = workdir / f"stderr-{repeat}.txt"
    env = dict(os.environ, TMPDIR=str(tempdir))
    with DiskSampler(tempdir) as sampler, stdout.open("wb") as out_handle, stderr.open("wb") as err_handle:
        started = time.monotonic()
        result = subprocess.run(
            ["/usr/bin/time", "-v", "-o", str(stats), *command],
            cwd=workdir,
            env=env,
            stdout=out_handle,
            stderr=err_handle,
            check=False,
        )
        elapsed = time.monotonic() - started
    sampled = parse_time(stats)
    sampled["exit_code"] = result.returncode
    sampled["runner_wall_s"] = elapsed
    sampled["temp_peak_bytes"] = sampler.peak
    sampled["stdout_bytes"] = stdout.stat().st_size if stdout.exists() else 0
    sampled["stderr_bytes"] = stderr.stat().st_size if stderr.exists() else 0
    return sampled


def check_case(name: str, workdir: Path, bins: dict[str, Path], records: int) -> tuple[bool, str]:
    try:
        if name.startswith("cutadapt"):
            output = workdir / "out.fastq.gz"
            if not output.is_file():
                return False, "missing out.fastq.gz"
            with gzip.open(output, "rb") as handle:
                count = sum(1 for line in handle if line.startswith(b"@"))
            return count <= records, f"records={count} <= {records}"
        if name == "fastqc":
            report = workdir / "out" / "reads_fastqc" / "fastqc_data.txt"
            if not report.is_file():
                return False, "missing fastqc_data.txt"
            text = report.read_text(errors="replace")
            return f"Total Sequences\t{records}\n" in text, "Total Sequences matches"
        if name == "fx_stats":
            output = workdir / "stats.tsv"
            if not output.is_file():
                return False, "missing stats.tsv"
            rows = output.read_text().splitlines()
            fields = rows[1].split("\t") if len(rows) > 1 else []
            return len(fields) > 3 and fields[3] == str(records), f"num_seqs={fields[3] if len(fields) > 3 else '?'}"
        if name.startswith("bam_sort") or name == "umi_dedup":
            output = workdir / "out.bam"
            if not output.is_file():
                return False, "missing out.bam"
            quick = run_checked([str(bins["bam"]), "quickcheck", str(output)], workdir)
            count = run_checked([str(bins["bam"]), "view", "-c", str(output)], workdir)
            if quick.returncode != 0 or count.returncode != 0:
                return False, f"quickcheck={quick.returncode}, count={count.returncode}"
            value = int(count.stdout.strip() or "0")
            upper = records if name.startswith("bam_sort") else records
            return 0 < value <= upper, f"records={value} <= {upper}"
        return False, "unknown case"
    except Exception as error:  # benchmark checks must report, not crash the matrix
        return False, f"check error: {error}"


def binary_version(path: Path) -> str:
    result = run_checked([str(path), "--version"], path.parent)
    return (result.stdout or result.stderr).strip().splitlines()[0] if result.returncode == 0 else "unknown"


def hardware() -> dict:
    model = "unknown"
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                model = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    mem_kb = None
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                mem_kb = int(line.split()[1])
                break
    except OSError:
        pass
    return {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "cpu_model": model,
        "logical_cpus": os.cpu_count(),
        "memory_kb": mem_kb,
    }


def git_state() -> dict:
    try:
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        status = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True)
        return {"commit": commit, "dirty": bool(status.strip()), "dirty_paths": status.splitlines()}
    except (OSError, subprocess.CalledProcessError) as error:
        return {"commit": None, "dirty": None, "error": str(error)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sizes", default="1,10,100", help="comma-separated read multipliers")
    parser.add_argument("--repeats", type=int, default=DEFAULT_REPEATS)
    parser.add_argument("--out", default=str(ROOT / "bench/results/latest.json"))
    parser.add_argument("--workdir", default=None)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument(
        "--tools",
        default="cutadapt,fastqc,fx,bam,umi",
        help="comma-separated: cutadapt,fastqc,fx,bam,umi",
    )
    args = parser.parse_args()

    bins = {
        "cutadapt": ROOT / "target/release/rust-cutadapt",
        "fastqc": ROOT / "target/release/rust-fastqc",
        "fx": ROOT / "target/release/rust-fx",
        "bam": ROOT / "target/release/rust-bam",
        "umi": ROOT / "target/release/rust-umitools",
    }
    for name, path in bins.items():
        if not path.is_file():
            print(f"ERROR: missing {name} binary at {path}", file=sys.stderr)
            return 3

    sizes = [int(value) for value in args.sizes.split(",") if value.strip()]
    tools = {value.strip() for value in args.tools.split(",") if value.strip()}
    base = 1000
    work = Path(args.workdir) if args.workdir else Path(tempfile.mkdtemp(prefix="bioinfo-bench-"))
    work.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        for multiplier in sizes:
            records = base * multiplier
            input_dir = work / f"size-{multiplier}"
            input_dir.mkdir(parents=True, exist_ok=True)
            inputs = prepare(input_dir, records, bins)
            for case in ["cutadapt", "cutadapt_j1", "fastqc", "fx_stats", "bam_sort", "bam_sort_lowmem", "umi_dedup"]:
                if case.startswith("cutadapt") and "cutadapt" not in tools:
                    continue
                if case == "fastqc" and "fastqc" not in tools:
                    continue
                if case == "fx_stats" and "fx" not in tools:
                    continue
                if case.startswith("bam_sort") and "bam" not in tools:
                    continue
                if case == "umi_dedup" and "umi" not in tools:
                    continue
                case_dir = input_dir / case
                case_dir.mkdir(parents=True, exist_ok=True)
                if case == "fastqc":
                    (case_dir / "out").mkdir(exist_ok=True)
                if case.startswith("cutadapt"):
                    command = [
                        str(bins["cutadapt"]), "--quiet", "-a", "AGATCGGAAGAGC", "-m", "30",
                        "-j", "1" if case.endswith("_j1") else "4",
                        "-o", str(case_dir / "out.fastq.gz"), str(inputs["fastq"]),
                    ]
                elif case == "fastqc":
                    command = [str(bins["fastqc"]), "-o", str(case_dir / "out"), str(inputs["fastq"])]
                elif case == "fx_stats":
                    command = [str(bins["fx"]), "stats", "-a", "-T", "-o", str(case_dir / "stats.tsv"), str(inputs["fastq"])]
                elif case == "bam_sort":
                    command = [str(bins["bam"]), "sort", "-o", str(case_dir / "out.bam"), str(inputs["unsorted"])]
                elif case == "bam_sort_lowmem":
                    command = [
                        str(bins["bam"]), "sort", "--max-memory", "1MiB", "--max-open-files", "8",
                        "-o", str(case_dir / "out.bam"), str(inputs["unsorted"]),
                    ]
                else:
                    command = [
                        str(bins["umi"]), "dedup", "-I", str(inputs["sorted"]), "-S", str(case_dir / "out.bam"),
                        "--extract-umi-method=tag", "--umi-tag=UB",
                    ]
                samples = []
                check_ok = True
                check_note = ""
                for repeat in range(args.repeats):
                    sample = run_timed(command, case_dir, case_dir / "tmp", repeat)
                    samples.append(sample)
                    if sample["exit_code"] != 0:
                        check_ok, check_note = False, f"exit={sample['exit_code']}"
                if check_ok:
                    check_ok, check_note = check_case(case, case_dir, bins, records)
                walls = [s["wall_s"] for s in samples if s["wall_s"] is not None]
                entries = [s for s in samples if s["wall_s"] is not None]
                results.append(
                    {
                        "case": case,
                        "size_multiplier": multiplier,
                        "records": records,
                        "command": command,
                        "cwd": str(case_dir),
                        "threads": 1 if case.endswith("_j1") else (4 if case.startswith("cutadapt") else None),
                        "compression": "gzip" if case.startswith("cutadapt") else ("bgzf" if case.startswith("bam") or case == "umi_dedup" else "none"),
                        "repeats": args.repeats,
                        "samples": samples,
                        "summary": {
                            "wall_mean_s": statistics.fmean(walls) if walls else None,
                            "wall_median_s": statistics.median(walls) if walls else None,
                            "wall_min_s": min(walls) if walls else None,
                            "wall_max_s": max(walls) if walls else None,
                            "cpu_mean_s": statistics.fmean(
                                [(s["user_s"] or 0) + (s["sys_s"] or 0) for s in entries]
                            ) if entries else None,
                            "max_rss_kb": max((s["max_rss_kb"] or 0) for s in entries) if entries else None,
                            "temp_peak_bytes": max(s["temp_peak_bytes"] for s in samples),
                            "output_bytes": sum(s["stdout_bytes"] for s in samples),
                        },
                        "correctness": {"passed": check_ok, "note": check_note},
                        "input_sha256": {
                            key: sha256(value) for key, value in inputs.items()
                        },
                    }
                )
                print(f"{case:18s} size={multiplier:3d} {check_note}", flush=True)
    finally:
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)

    document = {
        "schema_version": 1,
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "git": git_state(),
        "hardware": hardware(),
        "toolchain": {
            "rustc": run_checked(["rustc", "--version"], ROOT).stdout.strip(),
            "cargo": run_checked(["cargo", "--version"], ROOT).stdout.strip(),
        },
        "binaries": {
            name: {"path": str(path), "sha256": sha256(path), "version": binary_version(path)}
            for name, path in bins.items()
        },
        "parameters": {"sizes": sizes, "repeats": args.repeats, "record_base": base},
        "results": results,
        "limitations": [
            "No oracle comparison is performed for every case; correctness checks are structural/record-count based.",
            "BAM sort temp-disk peak is sampled, not a hard process filesystem limit.",
            "hosted runner/CI performance is not measured here.",
        ],
    }
    output = Path(args.out)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(document, indent=2) + "\n")
    print(f"wrote {output}")
    return 0 if all(result["correctness"]["passed"] for result in results) else 1


if __name__ == "__main__":
    sys.exit(main())
