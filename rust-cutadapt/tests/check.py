#!/usr/bin/env python3
"""Compare rust-cutadapt output against the cutadapt 4.6 golden records."""
import argparse
import gzip
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent
FIXTURES = ROOT / "fixtures"
ADAPTER = "AGATCGGAAGAGC"


def read_fastq(path: pathlib.Path) -> bytes:
    if path.suffix == ".gz":
        with gzip.open(path, "rb") as handle:
            return handle.read()
    return path.read_bytes()


def run_case(binary: str, source: pathlib.Path, output_suffix: str, expected: pathlib.Path) -> None:
    with tempfile.TemporaryDirectory(prefix="ta-cutadapt-check-") as tmp:
        output = pathlib.Path(tmp) / ("out" + output_suffix)
        command = [binary, "-a", ADAPTER, "-m", "10", "-o", str(output), str(source)]
        completed = subprocess.run(command, capture_output=True, text=True)
        if completed.returncode != 0:
            raise SystemExit(
                f"FAIL {source.name}: exit {completed.returncode}\n"
                f"stdout: {completed.stdout}\nstderr: {completed.stderr}"
            )
        actual = read_fastq(output)
        want = read_fastq(expected)
        if actual != want:
            raise SystemExit(
                f"FAIL {source.name}: output differs from {expected.name}\n"
                f"first 400 actual bytes: {actual[:400]!r}\n"
                f"first 400 expected bytes: {want[:400]!r}"
            )
        print(f"PASS {source.name} -> {output_suffix}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", default="target/release/rust-cutadapt")
    args = parser.parse_args()
    binary = str(pathlib.Path(args.bin).resolve())
    run_case(binary, FIXTURES / "simple.fastq", ".fastq", FIXTURES / "simple.expected.fastq")
    run_case(binary, FIXTURES / "simple.fastq.gz", ".fastq", FIXTURES / "simple.expected.fastq")
    run_case(binary, FIXTURES / "simple.fastq.gz", ".fastq.gz", FIXTURES / "simple.expected.fastq")
    print("all visible conformance cases passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
