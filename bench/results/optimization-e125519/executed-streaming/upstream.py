#!/usr/bin/env python3
"""Correctness-gated benchmark: rust CLIs versus pinned upstream tools.

This complements ``bench/run.py``. Every case runs the rust binary and the
matching upstream binary on the same generated input, with the same arguments,
and compares an observable output. Wall time, CPU time and peak RSS come from
``/usr/bin/time -v``; temporary-disk peak is sampled by the shared runner.

The FastQC case explicitly uses ``--extract`` for both binaries and compares
``fastqc_data.txt`` byte-for-byte. HTML and ZIP artifacts are outside this
runner's comparison scope. Any data difference is recorded as a failed
equivalence, not hidden. The other cases use exact bytes or a normalized BAM
payload (records plus header without @PG provenance lines).
"""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
RUN_PATH = ROOT / "bench" / "run.py"
_spec = importlib.util.spec_from_file_location("bioinfo_bench_run", RUN_PATH)
assert _spec and _spec.loader
bench = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bench)


def find_upstream(name: str) -> Path:
    env_name = {
        "cutadapt": "CUTADAPT_ORACLE",
        "umi_tools": "UMI_TOOLS_ORACLE",
        "samtools": "SAMTOOLS_ORACLE",
        "fastqc": "FASTQC_ORACLE",
        "seqkit": "SEQKIT_ORACLE",
    }[name]
    env = os.environ.get(env_name)
    candidates = []
    if env:
        candidates.append(Path(env))
    candidates.append(ROOT / "tests" / "ci" / "env" / "bin" / name)
    found = shutil.which(name)
    if found:
        candidates.append(Path(found))
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    raise SystemExit(
        f"ERROR: {name} not found. Set {env_name} or install the pinned oracle."
    )


def version(path: Path) -> str:
    result = bench.run_checked([str(path), "--version"], path.parent)
    text = (result.stdout or result.stderr).strip().splitlines()
    return text[0] if text else "unknown"


def gzip_bytes(path: Path) -> bytes:
    with gzip.open(path, "rb") as handle:
        return handle.read()


def bam_payload(path: Path, samtools: Path) -> bytes:
    header = bench.run_checked([str(samtools), "view", "-H", str(path)], path.parent)
    records = subprocess.run(
        [str(samtools), "view", str(path)],
        cwd=path.parent,
        capture_output=True,
        check=False,
    )
    if header.returncode != 0 or records.returncode != 0:
        raise RuntimeError(f"cannot decode BAM {path}")
    clean_header = "\n".join(
        line for line in header.stdout.splitlines() if not line.startswith("@PG")
    )
    return clean_header.encode() + b"\n" + records.stdout


def fastqc_data(outdir: Path, sample: str) -> Path:
    return outdir / f"{sample}_fastqc" / "fastqc_data.txt"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def summarize(samples: list[dict]) -> dict:
    walls = [s["wall_s"] for s in samples if s["wall_s"] is not None]
    return {
        "wall_mean_s": statistics.fmean(walls) if walls else None,
        "wall_median_s": statistics.median(walls) if walls else None,
        "wall_min_s": min(walls) if walls else None,
        "wall_max_s": max(walls) if walls else None,
        "max_rss_kb": max((s["max_rss_kb"] or 0) for s in samples) if samples else None,
        "temp_peak_bytes": max((s["temp_peak_bytes"] for s in samples), default=0),
        "exit_codes": [s["exit_code"] for s in samples],
    }


def prepare_upstream_temp(tempdir: Path) -> None:
    """Expose the already-extracted toolchain under the case TMPDIR.

    The CI wrappers use ``$TMPDIR/bioinfo-ci-toolchain``. Without this symlink,
    every timed upstream command would re-extract the 2.6GB toolchain into its
    case directory, dominating disk/time measurements. ``os.walk`` does not
    follow symlinked directories, so the DiskSampler still measures the
    command's own temp files.
    """
    cache = Path(os.environ.get("BIOINFO_TOOLCHAIN_CACHE", "/tmp/bioinfo-ci-toolchain"))
    if not (cache / ".ready").is_file():
        return
    tempdir.mkdir(parents=True, exist_ok=True)
    link = tempdir / "bioinfo-ci-toolchain"
    if not link.exists() and not link.is_symlink():
        link.symlink_to(cache, target_is_directory=True)


def case_commands(
    case: str,
    rust: dict[str, Path],
    upstream: dict[str, Path],
    inputs: dict[str, Path],
    rust_dir: Path,
    upstream_dir: Path,
) -> tuple[list[str], list[str], str]:
    if case == "cutadapt":
        return (
            [
                str(rust["cutadapt"]), "--quiet", "-a", "AGATCGGAAGAGC", "-m", "30",
                "-j", "4", "-o", str(rust_dir / "out.fastq.gz"), str(inputs["fastq"]),
            ],
            [
                str(upstream["cutadapt"]), "--quiet", "-a", "AGATCGGAAGAGC", "-m", "30",
                "-j", "4", "-o", str(upstream_dir / "out.fastq.gz"), str(inputs["fastq"]),
            ],
            "gzip",
        )
    if case == "fastqc":
        return (
            [
                str(rust["fastqc"]), "--extract", "--quiet", "-o", str(rust_dir),
                str(inputs["fastq"]),
            ],
            [
                str(upstream["fastqc"]), "--extract", "--quiet", "--outdir",
                str(upstream_dir), str(inputs["fastq"]),
            ],
            "none",
        )
    if case == "fx_stats":
        return (
            [
                str(rust["fx"]), "stats", "-a", "-T", "-o", str(rust_dir / "stats.tsv"),
                str(inputs["fastq"]),
            ],
            [
                str(upstream["seqkit"]), "stats", "-a", "-T", "-o",
                str(upstream_dir / "stats.tsv"), str(inputs["fastq"]),
            ],
            "none",
        )
    if case == "bam_sort":
        return (
            [str(rust["bam"]), "sort", "-o", str(rust_dir / "out.bam"), str(inputs["unsorted"])],
            [str(upstream["samtools"]), "sort", "-o", str(upstream_dir / "out.bam"), str(inputs["unsorted"])],
            "bgzf",
        )
    if case == "umi_dedup":
        args = [
            "dedup", "-I", str(inputs["sorted"]), "-S", "", "--random-seed", "1",
            "--extract-umi-method=tag", "--umi-tag=UB",
        ]
        rust_args = args.copy()
        rust_args[rust_args.index("")] = str(rust_dir / "out.bam")
        upstream_args = args.copy()
        upstream_args[upstream_args.index("")] = str(upstream_dir / "out.bam")
        return (
            [str(rust["umi"]), *rust_args],
            [str(upstream["umi_tools"]), *upstream_args],
            "bgzf",
        )
    raise ValueError(case)


def equivalence(
    case: str,
    rust_dir: Path,
    upstream_dir: Path,
    inputs: dict[str, Path],
    upstream: dict[str, Path],
) -> tuple[bool, str, dict[str, str]]:
    if case == "cutadapt":
        a = gzip_bytes(rust_dir / "out.fastq.gz")
        b = gzip_bytes(upstream_dir / "out.fastq.gz")
        return a == b, f"decompressed FASTQ bytes ({len(a)} vs {len(b)})", {
            "rust": digest(a), "upstream": digest(b)
        }
    if case == "fx_stats":
        a = (rust_dir / "stats.tsv").read_bytes()
        b = (upstream_dir / "stats.tsv").read_bytes()
        return a == b, f"stats TSV bytes ({len(a)} vs {len(b)})", {
            "rust": digest(a), "upstream": digest(b)
        }
    if case in {"bam_sort", "umi_dedup"}:
        a = bam_payload(rust_dir / "out.bam", upstream["samtools"])
        b = bam_payload(upstream_dir / "out.bam", upstream["samtools"])
        return a == b, f"normalized BAM payload ({len(a)} vs {len(b)})", {
            "rust": digest(a), "upstream": digest(b)
        }
    if case == "fastqc":
        sample = inputs["fastq"].stem
        a = fastqc_data(rust_dir, sample)
        b = fastqc_data(upstream_dir, sample)
        if not a.is_file() or not b.is_file():
            return False, "missing fastqc_data.txt", {"rust": "", "upstream": ""}
        left = a.read_bytes()
        right = b.read_bytes()
        detail = "fastqc_data.txt differs" if left != right else "fastqc_data.txt bytes"
        return left == right, f"{detail} ({len(left)} vs {len(right)})", {
            "rust": digest(left), "upstream": digest(right)
        }
    raise ValueError(case)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", default="1,10", help="comma-separated read multipliers")
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("--out", default=str(ROOT / "bench/results/upstream-current.json"))
    parser.add_argument("--workdir", default=None)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--tools", default="cutadapt,fastqc,fx,bam,umi")
    args = parser.parse_args()

    rust = {
        "cutadapt": ROOT / "target/release/rust-cutadapt",
        "fastqc": ROOT / "target/release/rust-fastqc",
        "fx": ROOT / "target/release/rust-fx",
        "bam": ROOT / "target/release/rust-bam",
        "umi": ROOT / "target/release/rust-umitools",
    }
    for name, path in rust.items():
        if not path.is_file():
            print(f"ERROR: missing rust {name} at {path}; build release binaries first", file=sys.stderr)
            return 3
    upstream = {
        "cutadapt": find_upstream("cutadapt"),
        "fastqc": find_upstream("fastqc"),
        "seqkit": find_upstream("seqkit"),
        "samtools": find_upstream("samtools"),
        "umi_tools": find_upstream("umi_tools"),
    }
    sizes = [int(value) for value in args.sizes.split(",") if value.strip()]
    tools = {value.strip() for value in args.tools.split(",") if value.strip()}
    work = Path(args.workdir) if args.workdir else Path(tempfile.mkdtemp(prefix="bioinfo-upstream-bench-"))
    work.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        for multiplier in sizes:
            records = 1000 * multiplier
            input_dir = work / f"size-{multiplier}"
            input_dir.mkdir(parents=True, exist_ok=True)
            inputs = bench.prepare(input_dir, records, rust)
            # UMI-tools' fetch path requires an index; this is input preparation,
            # outside every timed section.
            indexed = bench.run_checked(
                [str(upstream["samtools"]), "index", str(inputs["sorted"])],
                input_dir,
            )
            if indexed.returncode != 0:
                raise RuntimeError(f"cannot index sorted BAM: {indexed.stderr}")
            for case in ["cutadapt", "fastqc", "fx_stats", "bam_sort", "umi_dedup"]:
                if case == "cutadapt" and "cutadapt" not in tools:
                    continue
                if case == "fastqc" and "fastqc" not in tools:
                    continue
                if case == "fx_stats" and "fx" not in tools:
                    continue
                if case == "bam_sort" and "bam" not in tools:
                    continue
                if case == "umi_dedup" and "umi" not in tools:
                    continue
                case_dir = input_dir / case
                rust_dir = case_dir / "rust"
                upstream_dir = case_dir / "upstream"
                rust_dir.mkdir(parents=True, exist_ok=True)
                upstream_dir.mkdir(parents=True, exist_ok=True)
                rust_cmd, upstream_cmd, compression = case_commands(
                    case, rust, upstream, inputs, rust_dir, upstream_dir
                )
                rust_samples = [
                    bench.run_timed(rust_cmd, rust_dir, rust_dir / "tmp", repeat)
                    for repeat in range(args.repeats)
                ]
                upstream_temp = upstream_dir / "tmp"
                prepare_upstream_temp(upstream_temp)
                upstream_samples = [
                    bench.run_timed(upstream_cmd, upstream_dir, upstream_temp, repeat)
                    for repeat in range(args.repeats)
                ]
                rust_ok = all(sample["exit_code"] == 0 for sample in rust_samples)
                upstream_ok = all(sample["exit_code"] == 0 for sample in upstream_samples)
                same = False
                note = "not compared because a command failed"
                hashes: dict[str, str] = {}
                if rust_ok and upstream_ok:
                    try:
                        same, note, hashes = equivalence(
                            case, rust_dir, upstream_dir, inputs, upstream
                        )
                    except Exception as error:
                        note = f"output comparison failed: {error}"
                elif not upstream_ok:
                    note = "upstream command failed; see upstream samples/stderr"
                elif not rust_ok:
                    note = "rust command failed; see rust samples/stderr"
                passed = rust_ok and upstream_ok and same
                results.append(
                    {
                        "case": case,
                        "size_multiplier": multiplier,
                        "records": records,
                        "rust_command": rust_cmd,
                        "upstream_command": upstream_cmd,
                        "threads": 4 if case == "cutadapt" else None,
                        "compression": compression,
                        "repeats": args.repeats,
                        "rust": {
                            "samples": rust_samples,
                            "summary": summarize(rust_samples),
                        },
                        "upstream": {
                            "samples": upstream_samples,
                            "summary": summarize(upstream_samples),
                        },
                        "equivalence": {
                            "passed": same,
                            "note": note,
                            "sha256": hashes,
                        },
                        "correctness": {
                            "passed": passed,
                            "rust_exit_ok": rust_ok,
                            "upstream_exit_ok": upstream_ok,
                        },
                        "input_sha256": {name: bench.sha256(path) for name, path in inputs.items()},
                    }
                )
                speed = None
                rust_walls = [s["runner_wall_s"] for s in rust_samples if s.get("runner_wall_s") is not None]
                upstream_walls = [s["runner_wall_s"] for s in upstream_samples if s.get("runner_wall_s") is not None]
                if rust_walls and upstream_walls:
                    rw = statistics.fmean(rust_walls)
                    uw = statistics.fmean(upstream_walls)
                    speed = round(uw / rw, 3) if rw else None
                print(
                    f"{case:12s} size={multiplier:3d} equivalent={same} "
                    f"rust_exit={rust_ok} upstream_exit={upstream_ok} "
                    f"speedup={speed} {note}",
                    flush=True,
                )
    finally:
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)

    document = {
        "schema_version": 1,
        "kind": "upstream-correctness-benchmark",
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "git": bench.git_state(),
        "hardware": bench.hardware(),
        "toolchain": {
            "rustc": bench.run_checked(["rustc", "--version"], ROOT).stdout.strip(),
            "cargo": bench.run_checked(["cargo", "--version"], ROOT).stdout.strip(),
        },
        "binaries": {
            "rust": {
                name: {"path": str(path), "sha256": bench.sha256(path), "version": bench.binary_version(path)}
                for name, path in rust.items()
            },
            "upstream": {
                name: {"path": str(path), "sha256": bench.sha256(path), "version": version(path)}
                for name, path in upstream.items()
            },
        },
        "parameters": {"sizes": sizes, "repeats": args.repeats, "record_base": 1000},
        "results": results,
        "limitations": [
            "Synthetic inputs; this is a throughput/resource benchmark, not an alignment benchmark.",
            "FastQC full data equivalence is currently false due to the known overrepresented-sequence gap (T6).",
            "BAM comparisons normalize away @PG provenance lines; record payloads and other header lines must match.",
            "Wall-time ratios are specific to this CPU, binary, input and compression; they are not general speed claims.",
        ],
    }
    output = Path(args.out)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(document, indent=2) + "\n")
    print(f"wrote {output}")
    return 0 if all(row["correctness"]["passed"] for row in results) else 1


if __name__ == "__main__":
    sys.exit(main())
