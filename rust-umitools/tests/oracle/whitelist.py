#!/usr/bin/env python3
"""whitelist checks: distance/density knee, set-cell-number and error mapping.

Each case runs UMI-tools and rust-umitools on the same FASTQ and compares the
whitelist TSV bytes. Unsupported modes must be rejected before writing.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"

PASSED: list[str] = []
FAILED: list[tuple[str, str]] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    if ok:
        PASSED.append(name)
        print(f"PASS  {name}")
    else:
        FAILED.append((name, detail))
        print(f"FAIL  {name}\n        {detail}")


def umi(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["umi_tools", "whitelist", *args, "--log2stderr"], capture_output=True, text=True
    )


def rust(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run([str(BINARY), "whitelist", *args], capture_output=True, text=True)


def write_fixture(path: Path, barcodes: list[str]) -> None:
    with path.open("w") as handle:
        for index, barcode in enumerate(barcodes):
            handle.write(f"@r{index}\nGGGG{barcode}ACGTAC\n+\n{'I' * 14}\n")


def compare(name: str, work: Path, fixture: Path, extra: list[str]) -> None:
    reference = work / f"{name.replace(' ', '_')}_umi.tsv"
    subject = work / f"{name.replace(' ', '_')}_rust.tsv"
    ref = umi(["-I", str(fixture), "-S", str(reference), "-p", "NNNNCCCC", *extra])
    sub = rust(["-I", str(fixture), "-S", str(subject), "-p", "NNNNCCCC", *extra, "--quiet"])
    if ref.returncode != 0 or sub.returncode != 0:
        check(name, False,
              f"exit umi={ref.returncode} rust={sub.returncode} {sub.stderr.strip()[-160:]}")
        return
    check(name, reference.read_bytes() == subject.read_bytes(),
          f"umi={reference.read_text()!r} rust={subject.read_text()!r}")


def main() -> int:
    if shutil.which("umi_tools") is None:
        print("ERROR: umi_tools must be on PATH", file=sys.stderr)
        return 2
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="umitools-whitelist-") as directory:
        work = Path(directory)
        fixture = work / "reads.fastq"
        write_fixture(
            fixture,
            ["AAAA"] * 100 + ["CCCC"] * 60 + ["GGGG"] * 30 + ["TTTT"] * 10 + ["ACGT"] * 5,
        )
        compare("distance knee", work, fixture, ["--method", "reads", "--knee-method", "distance"])
        compare("density knee", work, fixture, ["--method", "reads", "--knee-method", "density"])
        compare("set cell number", work, fixture, ["--set-cell-number", "2"])

        errors = work / "errors.fastq"
        write_fixture(
            errors, ["AAAA"] * 100 + ["CCCC"] * 60 + ["CCCA"] * 40 + ["GGGG"] * 20 + ["TTTT"] * 5
        )
        compare("correct above threshold", work, errors,
                ["--set-cell-number", "3", "--ed-above-threshold", "correct"])
        compare("discard above threshold", work, errors,
                ["--set-cell-number", "3", "--ed-above-threshold", "discard"])

        for label, args in [
            ("method umis rejected", ["--method", "umis"]),
            ("paired input rejected", ["--read2-in", str(fixture)]),
            ("knee method rejected", ["--knee-method", "bogus"]),
        ]:
            target = work / f"{label.replace(' ', '_')}.tsv"
            result = rust(["-I", str(fixture), "-S", str(target), "-p", "NNNNCCCC",
                           *args, "--quiet"])
            check(label, result.returncode == 2 and not target.exists(),
                  f"exit={result.returncode}")

        target = work / "no_cell.tsv"
        result = rust(["-I", str(fixture), "-S", str(target), "-p", "NNNN", "--quiet"])
        check("pattern without cell bases rejected",
              result.returncode == 2 and not target.exists(), f"exit={result.returncode}")

    print(f"\n{len(PASSED)} passed, {len(FAILED)} failed")
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main())
