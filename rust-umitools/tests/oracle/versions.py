#!/usr/bin/env python3
"""Pin the oracle environment so a comparison run is reproducible.

The compatibility contract is defined against one UMI-tools release and one set
of numerical dependencies. If any of them silently changes, a passing diff says
nothing, so this check fails before the oracle harness starts.

Override for deliberate upgrades:
  UMI_TOOLS_EXPECTED=1.2.0 samtools python3 tests/oracle/versions.py
Set ORACLE_ALLOW_MISMATCH=1 only for exploratory runs; CI must not set it.
"""

from __future__ import annotations

import os
import platform
import re
import shutil
import subprocess
import sys

PINNED = {
    "umi_tools": "1.1.6",
    "samtools": "1.24",
    "numpy": "2.5.3",
    "pysam": "0.24.1",
    "python": "3.12.14",
}


def output(command: list[str]) -> str:
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    return (result.stdout + result.stderr).strip()


def umi_version() -> str | None:
    text = output(["umi_tools", "--version"])
    match = re.search(r"(\d+\.\d+\.\d+)", text)
    return match.group(1) if match else None


def samtools_version() -> str | None:
    text = output(["samtools", "--version"])
    match = re.search(r"samtools\s+(\d+\.\d+(?:\.\d+)?)", text)
    return match.group(1) if match else None


def module_version(name: str) -> str | None:
    text = output([sys.executable, "-c", f"import {name}; print({name}.__version__)"])
    match = re.search(r"(\d+\.\d+\.\d+)", text)
    return match.group(1) if match else None


def main() -> int:
    actual: dict[str, str | None] = {
        "umi_tools": umi_version(),
        "samtools": samtools_version(),
        "numpy": module_version("numpy"),
        "pysam": module_version("pysam"),
        "python": platform.python_version(),
    }
    expected = {
        name: os.environ.get(f"{name.upper()}_EXPECTED", pinned)
        for name, pinned in PINNED.items()
    }
    problems = []
    for name, wanted in expected.items():
        found = actual[name]
        if found != wanted:
            problems.append(f"{name}: expected {wanted}, found {found}")
    if shutil.which("umi_tools") is None:
        problems.append("umi_tools is not on PATH")
    if shutil.which("samtools") is None:
        problems.append("samtools is not on PATH")

    summary = ", ".join(f"{name}={value}" for name, value in actual.items())
    if problems:
        if os.environ.get("ORACLE_ALLOW_MISMATCH") == "1":
            print(f"WARNING: oracle mismatch tolerated: {'; '.join(problems)}")
            print(f"oracle versions: {summary}")
            return 0
        print("ERROR: oracle environment does not match the pinned baseline:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print(f"  found: {summary}", file=sys.stderr)
        print(
            "  install the pinned versions or set the matching *_EXPECTED variables",
            file=sys.stderr,
        )
        return 2
    print(f"oracle versions: {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
