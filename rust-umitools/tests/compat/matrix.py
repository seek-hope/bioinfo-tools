#!/usr/bin/env python3
"""Machine readable compatibility matrix and its sync check.

`docs/compatibility.tsv` is generated from the rows below. Regenerate after a
behaviour change:

    python3 tests/compat/matrix.py --write docs/compatibility.tsv

CI runs `--check`, which fails when the committed file and the generator drift.

Status values:
  verified     the behaviour has an executable comparison against the oracle
  experimental implemented and unit tested, no per-option upstream comparison
  unsupported  rejected before any output file is created
  extension    intentionally beyond UMI-tools
"""

from __future__ import annotations

import argparse
import csv
import io
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
ORACLE = "1.1.6"

COLUMNS = ["command", "option", "status", "oracle", "fixture", "comparison", "note"]

ROWS = [
    ("dedup", "--method=unique|percentile|cluster|adjacency|directional", "verified", ORACLE,
     "families, paired-repeat", "decompressed bytes", "five methods"),
    ("dedup", "--edit-distance-threshold=0|1|2|at-length", "verified", ORACLE, "edge cases",
     "decompressed bytes", ""),
    ("dedup", "--paired", "verified", ORACLE, "paired, paired-repeat", "decompressed bytes",
     "includes repeated pairs and mate recovery"),
    ("dedup", "--extract-umi-method=read_id", "verified", ORACLE, "families", "decompressed bytes", ""),
    ("dedup", "--extract-umi-method=tag", "verified", ORACLE, "tagged", "decompressed bytes", ""),
    ("dedup", "--extract-umi-method=umis", "experimental", ORACLE, "unit tests", "member sets", ""),
    ("dedup", "--random-seed", "verified", ORACLE, "all exact cases", "decompressed bytes",
     "CPython MT19937 reproduction"),
    ("dedup", "default coordinate sort", "verified", ORACLE, "exact cases + oracle B04",
     "decompressed bytes", ""),
    ("dedup", "--no-sort-output", "verified", ORACLE, "single-nosort, paired-nosort",
     "decompressed bytes", ""),
    ("dedup", "--mapping-quality", "verified", ORACLE, "edge cases", "decompressed bytes", ""),
    ("dedup", "--multimapping-detection-method=NH|X0|XT", "verified", ORACLE, "edge cases",
     "decompressed bytes", "tie draw consumes the same RNG values as upstream"),
    ("dedup", "--spliced-is-unique", "verified", ORACLE, "edge cases", "decompressed bytes", ""),
    ("dedup", "--ignore-tlen", "verified", ORACLE, "edge cases", "decompressed bytes", ""),
    ("dedup", "--read-length", "verified", ORACLE, "edge cases", "decompressed bytes", ""),
    ("dedup", "--representative=upstream", "verified", ORACLE, "exact cases + oracle C3",
     "decompressed bytes", "representative tie handling"),
    ("dedup", "--representative=stable", "experimental", None, "unit tests", "molecule set",
     "project RNG, not CPython"),
    ("dedup", "--subset P", "verified", ORACLE, "edge cases", "decompressed bytes",
     "shared RNG with the tie draw"),
    ("dedup", "--chrom NAME", "verified", ORACLE, "edge cases", "decompressed bytes",
     "whole-contig filter, unknown name refused"),
    ("dedup", "--per-gene --gene-tag", "verified", ORACLE, "edge cases", "decompressed bytes", ""),
    ("dedup", "--per-cell --cell-tag", "verified", ORACLE, "edge cases", "decompressed bytes", ""),
    ("dedup", "bounded paths: window, mate join, disk bundle", "verified", None,
     "spill suite", "decompressed bytes", "K9 forced paths"),
    ("dedup", "--output-stats", "unsupported", None, "n/a", "rejected", "null model not implemented"),
    ("dedup", "--filter-umi [--umi-whitelist|--umi-whitelist-paired]", "verified", ORACLE,
     "edge cases", "decompressed bytes", "single and paired whitelists"),
    ("dedup", "--buffer-whole-contig", "verified", ORACLE, "edge cases", "decompressed bytes",
     "contig-wide bundle window"),
    ("dedup", "--gene-transcript-map", "unsupported", None, "n/a", "rejected",
     "count implements the map"),
    ("dedup", "--compresslevel 0-9", "verified", ORACLE, "edge cases", "decompressed bytes",
     "0 stores uncompressed blocks; default 1, 6 matches upstream size"),
    ("dedup", "--add-pg", "extension", None, "n/a", "not compared", "changes output bytes"),
    ("group", "--group-out", "verified", ORACLE, "families x five methods", "bam bytes + tsv bytes", ""),
    ("group", "--output-bam", "verified", ORACLE, "families x five methods", "bam bytes", ""),
    ("group", "UG and BX tags", "verified", ORACLE, "families x five methods", "bam bytes", "UG uses C/S/I"),
    ("group", "--per-gene [--per-cell]", "verified", ORACLE, "edge cases", "bam bytes + tsv bytes", ""),
    ("count", "--per-gene --gene-tag", "verified", ORACLE, "tagged", "tsv bytes", ""),
    ("count", "--per-cell", "verified", ORACLE, "edge cases", "tsv bytes", "sparse triplet form"),
    ("count", "--wide", "verified", ORACLE, "edge cases", "tsv bytes", "dense gene x cell matrix"),
    ("count", "--per-contig", "experimental", ORACLE, "unit tests", "tsv bytes", ""),
    ("count", "--partition-genes", "verified", None, "spill suite", "TSV bytes",
     "automatic gene partition fallback"),
    ("count", "--gene-transcript-map", "verified", ORACLE, "edge cases", "tsv bytes",
     "requires --per-contig"),
    ("count_tab", "default", "verified", ORACLE, "two column table", "tsv bytes", ""),
    ("count_tab", "--per-cell", "verified", ORACLE, "cell table", "tsv bytes", ""),
    ("extract", "--extract-method=string with -p", "verified", ORACLE, "extract cases", "FASTQ bytes",
     "single/paired, 3prime, gzip, quality filter/mask"),
    ("extract", "--extract-method=regex", "verified", ORACLE, "extract cases", "FASTQ bytes",
     "named umi_* and discard_* groups"),
    ("extract", "other methods, phred64", "unsupported", None, "n/a", "rejected", ""),
    ("whitelist", "--knee-method=distance|density, --set-cell-number", "verified", ORACLE,
     "whitelist cases", "TSV bytes", ""),
    ("whitelist", "--ed-above-threshold=discard|correct", "verified", ORACLE,
     "whitelist cases", "TSV bytes", ""),
    ("whitelist", "--method=umis, --read2-in", "unsupported", None, "n/a", "rejected", ""),
    ("all", "--threads", "verified", None, "determinism suite", "decompressed bytes", "1/2/8/16"),
    ("all", "--memory-limit", "verified", None, "spill and determinism suites",
     "exit code + bytes", "64 MiB minimum working set"),
    ("all", "--quiet, --progress, --summary-json", "verified", None, "formats suite",
     "stream contents", ""),
    ("all", "-I - / -S - (stdin/stdout)", "verified", None, "formats suite",
     "decompressed bytes", "two-pass processing spools stdin"),
    ("all", "CRAM input/output with --reference", "verified", None, "formats suite",
     "core fields", "derived MD/NM tags may be added when decoding"),
]


def render() -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, delimiter="\t", lineterminator="\n")
    writer.writerow(COLUMNS)
    for row in sorted(ROWS, key=lambda value: (value[0], value[1])):
        writer.writerow(row)
    # csv.writer emits the trailing delimiter even when the final field is empty.
    # Strip line-end whitespace so the generated TSV is git-clean.
    return "\n".join(line.rstrip() for line in buffer.getvalue().splitlines()) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", metavar="PATH", help="write the generated matrix")
    parser.add_argument("--check", metavar="PATH", help="fail when the file is out of sync")
    args = parser.parse_args()
    if not args.write and not args.check:
        parser.error("use --write PATH or --check PATH")
    generated = render()
    if args.write:
        Path(args.write).write_text(generated)
        print(f"wrote {args.write} ({len(ROWS)} rows)")
    if args.check:
        path = Path(args.check)
        if not path.exists():
            print(f"ERROR: {path} is missing; run tests/compat/matrix.py --write", file=sys.stderr)
            return 2
        if path.read_text() != generated:
            print(
                f"ERROR: {path} is out of sync; run tests/compat/matrix.py --write {path}",
                file=sys.stderr,
            )
            return 2
        print(f"compatibility matrix in sync ({len(ROWS)} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
