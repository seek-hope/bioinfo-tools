#!/usr/bin/env python3
"""Format and pipe checks: stdin/stdout, CRAM, summary JSON.

CRAM is reference based, so reading it back may add derived MD/NM tags
depending on the htslib decode setting; the comparison therefore uses the core
alignment fields and ignores those derived tags.
"""

from __future__ import annotations

import gzip
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"
HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:5000\n"
DERIVED = re.compile(r"\t(?:MD:Z:\S+|NM:i:\d+)")


def record(name: str, umi: str, pos: int) -> str:
    return (
        f"{name}_{umi}\t0\tchr1\t{pos}\t60\t10M\t*\t0\t0\t{'A' * 10}\t{'I' * 10}\n"
    )


def build(work: Path) -> tuple[Path, Path]:
    rows = []
    for position in (100, 200):
        for family, umi in enumerate(("AAAAAAAA", "CCCCCCCC", "GGGGGGGG")):
            for copy in range(3):
                rows.append(record(f"r{len(rows)}", umi, position))
    sam = work / "in.sam"
    bam = work / "in.bam"
    sam.write_text(HEADER + "".join(rows))
    subprocess.run(["samtools", "sort", "-o", str(bam), str(sam)], check=True, capture_output=True)
    subprocess.run(["samtools", "index", str(bam)], check=True, capture_output=True)
    reference = work / "ref.fa"
    reference.write_text(">chr1\n" + "A" * 5000 + "\n")
    subprocess.run(["samtools", "faidx", str(reference)], check=True, capture_output=True)
    return bam, reference


def run(args: list[str], stdin: bytes | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(BINARY), *args], input=stdin, capture_output=True, check=False
    )


def record_lines(path: Path, reference: Path | None = None) -> list[str]:
    command = ["samtools", "view", str(path)]
    if reference is not None:
        command += ["-T", str(reference)]
    text = subprocess.run(command, capture_output=True, text=True, check=True).stdout
    return [DERIVED.sub("", line) for line in text.splitlines() if line]


def main() -> int:
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    failures = 0

    def check(name: str, ok: bool, detail: str = "") -> None:
        nonlocal failures
        print(f"{'PASS' if ok else 'FAIL'}  {name}{'' if ok else '  ' + detail}")
        if not ok:
            failures += 1

    with tempfile.TemporaryDirectory(prefix="umitools-formats-") as directory:
        work = Path(directory)
        bam, reference = build(work)

        baseline = work / "baseline.bam"
        result = run(["dedup", "-I", str(bam), "-S", str(baseline), "--random-seed", "1", "--quiet"])
        check("baseline dedup", result.returncode == 0, result.stderr.decode()[-160:])
        baseline_records = record_lines(baseline)

        # stdin -> file
        piped = work / "piped.bam"
        data = bam.read_bytes()
        result = run(["dedup", "-I", "-", "-S", str(piped), "--random-seed", "1", "--quiet"], data)
        check("stdin to file", result.returncode == 0 and record_lines(piped) == baseline_records,
              result.stderr.decode()[-160:])

        # file -> stdout
        result = run(["dedup", "-I", str(bam), "-S", "-", "--random-seed", "1", "--quiet"])
        stdout_path = work / "stdout.bam"
        stdout_path.write_bytes(result.stdout)
        check("file to stdout",
              result.returncode == 0 and record_lines(stdout_path) == baseline_records,
              result.stderr.decode()[-160:])

        # SIGPIPE: a consumer that closes early must not fail the run
        process = subprocess.Popen(
            [str(BINARY), "dedup", "-I", str(bam), "-S", "-", "--random-seed", "1",
             "--quiet"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        process.stdout.read(64)
        process.stdout.close()
        rc = process.wait(timeout=120)
        check("closed stdout pipe is not an error", rc == 0, f"exit={rc}")

        # Backpressure: a chunked producer on stdin must complete
        target = work / "backpressure.bam"
        process = subprocess.Popen(
            [str(BINARY), "dedup", "-I", "-", "-S", str(target), "--random-seed", "1",
             "--quiet"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        data = bam.read_bytes()
        for offset in range(0, len(data), 512):
            process.stdin.write(data[offset : offset + 512])
            process.stdin.flush()
            time.sleep(0.001)
        process.stdin.close()
        rc = process.wait(timeout=120)
        check("chunked stdin backpressure completes",
              rc == 0 and target.exists(), f"exit={rc}")

        # CRAM output, then read it back
        cram = work / "out.cram"
        result = run(["dedup", "-I", str(bam), "-S", str(cram), "--reference", str(reference),
                      "--random-seed", "1", "--quiet"])
        check("CRAM output with reference", result.returncode == 0, result.stderr.decode()[-160:])
        if cram.exists():
            count = subprocess.run(["samtools", "view", "-c", str(cram), "-T", str(reference)],
                                   capture_output=True, text=True).stdout.strip()
            check("CRAM decodes", count == str(len(baseline_records)), f"count={count}")

            back = work / "back.bam"
            result = run(["dedup", "-I", str(cram), "-S", str(back), "--reference", str(reference),
                          "--random-seed", "1", "--quiet"])
            check("CRAM input", result.returncode == 0 and record_lines(back) == baseline_records,
                  result.stderr.decode()[-160:])

        # CRAM output without a reference must be refused before writing
        missing = work / "missing.cram"
        result = run(["dedup", "-I", str(bam), "-S", str(missing), "--quiet"])
        check("CRAM output without reference is refused",
              result.returncode == 2 and not missing.exists(),
              f"exit={result.returncode}")

        # summary JSON
        summary = work / "summary.json"
        result = run(["dedup", "-I", str(bam), "-S", str(work / "s.bam"), "--random-seed", "1",
                      "--quiet", "--summary-json", str(summary)])
        ok = result.returncode == 0 and summary.exists()
        if ok:
            data = json.loads(summary.read_text())
            ok = (
                data.get("program") == "rust-umitools"
                and data.get("command") == "dedup"
                and "counters" in data
                and "resources" in data
            )
        check("summary JSON schema", ok)

    print(f"\n{'FAILED' if failures else 'formats ok'}: {failures} failures")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
