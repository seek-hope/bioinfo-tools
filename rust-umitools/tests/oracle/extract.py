#!/usr/bin/env python3
"""extract checks: string pattern, 3', gzip, paired, quality filter/mask.

Each case runs UMI-tools and rust-umitools on the same FASTQ and compares the
output bytes. Upstream needs --quality-encoding phred33 whenever a quality
filter is active, so the comparison supplies it.
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
        ["umi_tools", "extract", *args, "--log2stderr"], capture_output=True, text=True
    )


def rust(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run([str(BINARY), "extract", *args], capture_output=True, text=True)


def same(left: Path, right: Path, gzip_output: bool = False) -> bool:
    opener = gzip.open if gzip_output else open
    with opener(left, "rb") as a, opener(right, "rb") as b:
        return a.read() == b.read()


def main() -> int:
    if shutil.which("umi_tools") is None:
        print("ERROR: umi_tools must be on PATH", file=sys.stderr)
        return 2
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="umitools-extract-") as directory:
        work = Path(directory)
        single = work / "single.fastq"
        single.write_text(
            "@r1 comment\nACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIII\n"
            "@r2\nTTTTGGGGTTTTGGGGTTTT\n+\nIIIIIIIIIIIIIIIIIIII\n"
        )
        cases = [
            ("string pattern", ["-p", "NNNN"], []),
            ("3 prime", ["-p", "NNNN", "--3prime"], []),
            (
                "quality threshold",
                ["-p", "NNNN", "--quality-filter-threshold", "20",
                 "--quality-encoding", "phred33"],
                ["--quality-encoding", "phred33"],
            ),
            (
                "quality mask",
                ["-p", "NNNN", "--quality-filter-mask", "20",
                 "--quality-encoding", "phred33"],
                ["--quality-encoding", "phred33"],
            ),
        ]
        for label, extra, upstream_extra in cases:
            reference = work / f"{label.replace(' ', '_')}_umi.fastq"
            subject = work / f"{label.replace(' ', '_')}_rust.fastq"
            ref = umi(["-I", str(single), "-S", str(reference), *extra, *upstream_extra])
            sub = rust(["-I", str(single), "-S", str(subject), *extra, "--quiet"])
            if ref.returncode != 0 or sub.returncode != 0:
                check(label, False,
                      f"exit umi={ref.returncode} rust={sub.returncode} "
                      f"{sub.stderr.strip()[-160:]}")
                continue
            check(label, same(reference, subject), "output bytes differ")

        # regex extraction operates on the sequence, not the name
        regex_pattern = "(?P<umi_1>.{4})(?P<discard_1>ACGT)"
        regex_ref = work / "regex_umi.fastq"
        regex_sub = work / "regex_rust.fastq"
        ref = umi(["-I", str(single), "-S", str(regex_ref),
                   "--extract-method", "regex", "-p", regex_pattern])
        sub = rust(["-I", str(single), "-S", str(regex_sub),
                    "--extract-method", "regex", "-p", regex_pattern, "--quiet"])
        check("regex extraction",
              ref.returncode == 0 and sub.returncode == 0 and same(regex_ref, regex_sub),
              f"exit umi={ref.returncode} rust={sub.returncode}")

        # a regex without a umi_ group is refused before writing
        target = work / "regex_bad.fastq"
        result = rust(["-I", str(single), "-S", str(target), "--extract-method", "regex",
                       "-p", "(?P<other>.{4})", "--quiet"])
        check("regex without umi group rejected",
              result.returncode == 2 and not target.exists(),
              f"exit={result.returncode}")

        # gzip in and out
        compressed = work / "single.fastq.gz"
        with gzip.open(compressed, "wb") as handle:
            handle.write(single.read_bytes())
        ref_gz, sub_gz = work / "umi.fastq.gz", work / "rust.fastq.gz"
        ref = umi(["-I", str(compressed), "-S", str(ref_gz), "-p", "NNNN"])
        sub = rust(["-I", str(compressed), "-S", str(sub_gz), "-p", "NNNN", "--quiet"])
        check("gzip input and output",
              ref.returncode == 0 and sub.returncode == 0 and same(ref_gz, sub_gz, True),
              f"exit umi={ref.returncode} rust={sub.returncode}")

        # paired
        r1, r2 = work / "r1.fastq", work / "r2.fastq"
        r1.write_text("@p1\nACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIII\n")
        r2.write_text("@p1\nGGGGTTTTGGGGTTTTGGGG\n+\nIIIIIIIIIIIIIIIIIIII\n")
        u1, u2 = work / "u1.fastq", work / "u2.fastq"
        v1, v2 = work / "v1.fastq", work / "v2.fastq"
        ref = umi(["-I", str(r1), "-S", str(u1), "-p", "NNNN",
                   "--read2-in", str(r2), "--read2-out", str(u2)])
        sub = rust(["-I", str(r1), "-S", str(v1), "-p", "NNNN",
                    "--read2-in", str(r2), "--read2-out", str(v2), "--quiet"])
        check("paired extraction",
              ref.returncode == 0 and sub.returncode == 0
              and same(u1, v1) and same(u2, v2),
              f"exit umi={ref.returncode} rust={sub.returncode}")

        # mismatched pair names must fail without producing output
        bad2 = work / "bad2.fastq"
        bad2.write_text("@other\nGGGGTTTTGGGGTTTTGGGG\n+\nIIIIIIIIIIIIIIIIIIII\n")
        target = work / "mismatch.fastq"
        result = rust(["-I", str(r1), "-S", str(target), "-p", "NNNN",
                       "--read2-in", str(bad2), "--read2-out", str(work / "m2.fastq"), "--quiet"])
        check("mismatched pair names rejected",
              result.returncode != 0 and not target.exists(),
              f"exit={result.returncode}")

        # corrupt FASTQ must fail before committing
        corrupt = work / "corrupt.fastq"
        corrupt.write_text("@bad\nACGT\n+\nII\n")
        target = work / "corrupt_out.fastq"
        result = rust(["-I", str(corrupt), "-S", str(target), "-p", "NNNN", "--quiet"])
        check("corrupt FASTQ rejected",
              result.returncode != 0 and not target.exists(),
              f"exit={result.returncode}")

        # phred64 is a real quality offset: the single fixture's 'I' qualities
        # are Q9, so a Q20 filter must discard every read under phred64.
        ref_target = work / "enc_umi.fastq"
        target = work / "enc_rust.fastq"
        ref = umi(["-I", str(single), "-S", str(ref_target), "-p", "NNNN",
                   "--quality-encoding", "phred64",
                   "--quality-filter-threshold", "20"])
        result = rust(["-I", str(single), "-S", str(target), "-p", "NNNN",
                       "--quality-encoding", "phred64",
                       "--quality-filter-threshold", "20", "--quiet"])
        check("phred64 quality filter",
              ref.returncode == 0 and result.returncode == 0
              and same(ref_target, target),
              f"exit umi={ref.returncode} rust={result.returncode}")

    print(f"\n{len(PASSED)} passed, {len(FAILED)} failed")
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main())
