#!/usr/bin/env python3
"""Pinned FastQC tile comparisons and independent observed-quality checks.

Missing observations are independently represented as NaN; they are not Q0.
All original oracle scripts and fixtures remain unchanged. This suite generates
its own inputs, preserves raw tile blocks, and checks summary/ZIP data as well.
"""
import argparse
from collections import defaultdict
from decimal import Decimal
from fractions import Fraction
from html import unescape
import gzip
import json
import math
from pathlib import Path
import subprocess
import tempfile
import zipfile

NAME = "Per tile sequence quality"


def blocks(text):
    chunks = text.split(">>")
    if chunks[0] != "##FastQC\t0.13.0\n" or len(chunks) % 2 != 1:
        raise AssertionError("invalid report envelope")
    result = {}
    for i in range(1, len(chunks), 2):
        fragment = chunks[i]
        if chunks[i + 1] != "END_MODULE\n":
            raise AssertionError("invalid module terminator")
        title, status = fragment.splitlines()[0].split("\t", 1)
        if title in result:
            raise AssertionError("duplicate module")
        result[title] = (status, fragment.splitlines()[1:])
    return result


def tile_block(text):
    marker = f">>{NAME}\t"
    if marker not in text:
        return None
    start = text.index(marker)
    return text[start:text.index(">>END_MODULE\n", start) + len(">>END_MODULE\n")]


def groups(length, nogroup):
    if nogroup or length <= 75:
        return [(i, i) for i in range(1, length + 1)]
    for step in (2, 5, 10, 20, 50, 100, 200, 500, 1000):
        if 9 + math.ceil((length - 9) / step) < 75:
            singles = [(i, i) for i in range(1, 10)]
            prefix = [(10, step - 1)] if step > 10 else []
            return singles + prefix + [(i, min(i + step - 1, length)) for i in range(max(10, step), length + 1, step)]
    raise AssertionError("fixture too long")


def record(tile, length=4, score=20, legacy=False):
    header = f"I:1:{tile}:100:100#0/1" if legacy else f"I:1:F:1:{tile}:100:100 1:N:0:ACGT"
    quality = chr(33 + score) * length if isinstance(score, int) else score
    return header, quality, int(tile)


def independent(records, options):
    """Typed tile labels and exact rational means; no candidate parsing helpers."""
    minimum = int(options[options.index("--min_length") + 1]) if "--min_length" in options else 0
    maximum = int(options[options.index("--max_length") + 1]) if "--max_length" in options else math.inf
    offset = 64 if "--phred64" in options else 33
    sampled = []
    count = 0
    for header, quality, tile in records:
        if not minimum <= len(quality) <= maximum:
            continue
        count += 1
        if count <= 10000 or count % 10 == 0:
            if tile is None:
                return None
            sampled.append((tile, quality))
    tile_ids = sorted({tile for tile, _ in sampled})
    if not sampled or len(tile_ids) > 2501:
        return None
    positions = defaultdict(lambda: defaultdict(list))
    for tile, quality in sampled:
        for i, char in enumerate(quality, 1):
            positions[tile][i].append(ord(char) - offset)
    bins = groups(max(len(quality) for _, quality in sampled), "--nogroup" in options)
    expected = []
    worst = Fraction(0)
    means = {}
    for start, end in bins:
        observed = {}
        for tile in tile_ids:
            values = [Fraction(sum(positions[tile][i]), len(positions[tile][i]))
                      for i in range(start, end + 1) if positions[tile][i]]
            if values:
                observed[tile] = sum(values) / len(values)
        average = sum(observed.values()) / len(observed)
        for tile in tile_ids:
            value = observed[tile] - average if tile in observed else None
            means[tile, start] = value
            if value is not None:
                worst = max(worst, abs(value))
    for tile in tile_ids:
        for start, end in bins:
            expected.append((str(tile), str(start) if start == end else f"{start}-{end}", means[tile, start]))
    return ("fail" if worst > 10 else "warn" if worst > 5 else "pass"), expected


def java_number(value):
    if math.isnan(value):
        return "NaN"
    if math.isinf(value):
        return "Infinity" if value > 0 else "-Infinity"
    if value == 0 or 1e-3 <= abs(value) < 1e7:
        return repr(value)
    decimal = Decimal(repr(abs(value)))
    exponent = decimal.adjusted()
    mantissa = format(decimal.scaleb(-exponent), "f").rstrip("0").rstrip(".")
    if "." not in mantissa:
        mantissa += ".0"
    return ("-" if value < 0 else "") + mantissa + "E" + str(exponent)


def check_model(label, actual, expected):
    if expected is None:
        if actual is not None:
            raise AssertionError((label, "unexpected tile module", actual))
        return
    if actual is None or actual[0] != expected[0] or actual[1][0] != "#Tile\tBase\tMean":
        raise AssertionError((label, "module presence/status/header", actual, expected[0]))
    rows = [line.split("\t") for line in actual[1][1:]]
    if len(rows) != len(expected[1]):
        raise AssertionError((label, "row count", len(rows), len(expected[1])))
    for row, (tile, group, value) in zip(rows, expected[1]):
        if len(row) != 3 or row[:2] != [tile, group]:
            raise AssertionError((label, "row identity", row, tile, group))
        observed = float(row[2])
        if row[2] != java_number(observed):
            raise AssertionError((label, "noncanonical quality value", row[2]))
        if value is None:
            valid = row[2] == "NaN"
        else:
            valid = math.isfinite(observed) and abs(observed - float(value)) <= 1e-12
        if not valid:
            raise AssertionError((label, "independent mean", row, value))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fastqc", default="tests/ci/env/bin/fastqc")
    parser.add_argument("--rust", default="target/debug/rust-fastqc")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    upstream, rust = [str(Path(path).resolve()) for path in (args.fastqc, args.rust)]
    if subprocess.check_output([upstream, "--version"], text=True).strip() != "FastQC v0.13.0":
        raise SystemExit("FastQC 0.13.0 required")
    cases = []
    with tempfile.TemporaryDirectory(prefix="fastqc-tiles-") as tmp:
        root = Path(tmp)

        def case(label, records, options=(), corrected=False, compressed=False, check_groups=False):
            directory = root / label
            directory.mkdir()
            infile = directory / ("reads.fq.gz" if compressed else "reads.fq")
            raw = "".join(f"@{header}\n{('ACGT' * (len(quality) // 4 + 1))[:len(quality)]}\n+\n{quality}\n" for header, quality, _ in records).encode()
            infile.write_bytes(gzip.compress(raw) if compressed else raw)
            observed = []
            for which, binary in (("upstream", upstream), ("rust", rust)):
                outdir = directory / which
                outdir.mkdir()
                run = subprocess.run([binary, "--quiet", "--extract", "--outdir", str(outdir), *options, str(infile)], capture_output=True, text=True, timeout=60)
                if run.returncode:
                    raise AssertionError((label, which, run.returncode, run.stderr))
                data = (outdir / "reads_fastqc" / "fastqc_data.txt").read_bytes().decode()
                modules = blocks(data)
                summary = (outdir / "reads_fastqc" / "summary.txt").read_text()
                archive_path = outdir / "reads_fastqc.zip"
                if "--delete" in options:
                    if archive_path.exists():
                        raise AssertionError((label, which, "--delete kept the ZIP"))
                else:
                    with zipfile.ZipFile(archive_path) as archive:
                        if archive.read("reads_fastqc/fastqc_data.txt").decode() != data:
                            raise AssertionError((label, which, "ZIP/data mismatch"))
                        if archive.read("reads_fastqc/summary.txt").decode() != summary:
                            raise AssertionError((label, which, "ZIP/summary mismatch"))
                        if which == "rust" and archive.read("reads_fastqc/fastqc_report.html") != (outdir / "reads_fastqc" / "fastqc_report.html").read_bytes():
                            raise AssertionError((label, "ZIP/HTML mismatch"))
                tile = modules.get(NAME)
                if which == "rust":
                    html = (outdir / "reads_fastqc" / "fastqc_report.html").read_text()
                    marker = f"<h2>{NAME}</h2>"
                    if html != (outdir / "reads_fastqc.html").read_text() or (marker in html) != (tile is not None):
                        raise AssertionError((label, "native HTML module mismatch"))
                    if tile is not None:
                        body = html.split(marker, 1)[1].split("<pre>", 1)[1].split("</pre>", 1)[0]
                        if unescape(body) != "\n".join(tile[1]) + "\n":
                            raise AssertionError((label, "native HTML lost tile table values"))
                expected_summary = [] if tile is None else [f"{tile[0].upper()}\t{NAME}\t{infile.name}"]
                if [line for line in summary.splitlines() if f"\t{NAME}\t" in line] != expected_summary:
                    raise AssertionError((label, which, "summary mismatch"))
                if tile is not None:
                    names = list(modules)
                    if not names.index("Per base sequence quality") < names.index(NAME) < names.index("Per sequence quality scores"):
                        raise AssertionError((label, "module order"))
                warnings = [line for line in run.stderr.splitlines() if line.startswith("Too many tiles (>2500)")]
                observed.append((tile, modules, warnings, tile_block(data)))
            reference, candidate = observed
            check_model(label, candidate[0], independent(records, options))
            if reference[2] != candidate[2]:
                raise AssertionError((label, "tile-limit warning mismatch"))
            if corrected:
                # This particular upstream defect is zero-imputation of missing
                # observations, not a broad exemption for arbitrary differences.
                raw_means = defaultdict(list)
                for header, quality, tile in records:
                    raw_means[tile].append(quality)
                expected_imputed = []
                tile_ids = sorted(raw_means)
                for start, end in groups(max(len(q) for _, q, _ in records), "--nogroup" in options):
                    values = []
                    for tile in tile_ids:
                        means = [sum(ord(q[i-1])-33 for q in raw_means[tile] if len(q) >= i) / sum(len(q) >= i for q in raw_means[tile])
                                 for i in range(start, end+1) if any(len(q) >= i for q in raw_means[tile])]
                        values.append(sum(means) / len(means) if means else 0.0)
                    average = sum(values) / len(values)
                    for tile, value in zip(tile_ids, values):
                        expected_imputed.append((str(tile), str(start) if start == end else f"{start}-{end}", value-average))
                expected_imputed.sort(key=lambda row: (int(row[0]), int(row[1].split('-')[0])))
                worst = max(abs(value) for _, _, value in expected_imputed)
                buggy = ("fail" if worst > 10 else "warn" if worst > 5 else "pass", expected_imputed)
                check_model(label + "/upstream-zero-imputation", reference[0], buggy)
                if reference[0] == candidate[0]:
                    raise AssertionError((label, "missing-data defect did not reproduce"))
            elif reference[3] != candidate[3]:
                raise AssertionError((label, "raw tile difference", reference[0], candidate[0]))
            if check_groups:
                for name in ("Per base sequence quality", "Per base sequence content", "Per base N content", "Adapter Content"):
                    if reference[1][name] != candidate[1][name]:
                        raise AssertionError((label, "shared grouping", name, reference[1][name], candidate[1][name]))
            cases.append({"case": label, "verdict": "corrected-missing-observation" if corrected else "exact", "upstream": reference[0], "rust": candidate[0]})
            print(f"PASS {label}: {cases[-1]['verdict']}", flush=True)

        case("modern-headers", [record(1102, score=30), record(1101, score=10)])
        case("legacy-headers", [record(2, score=25, legacy=True), record(1, score=15, legacy=True)])
        case("single-tile", [record(1, score=0), record(1, score=93)])
        case("no-tile", [("ordinary", "!!!!", None)])
        case("invalid-late", [record(1), ("not-an-illumina-header", "!!!!", None)])
        case("overflow-id", [("I:1:F:1:2147483648:0:0", "!!!!", None)])
        case("modern-to-legacy", [record(1), (record(2, legacy=True)[0], "5555", None)])
        case("legacy-to-modern", [record(1, legacy=True), (record(2)[0], "5555", None)])
        case("trailing-empty-fields", [("I:1:F:1:11:0:", "5555", None)])
        case("signed-ids", [record(-2, score=0), record("+0003", score=10)])
        case("extra-fields", [("I:1:F:1:11:0:0:extra", "5555", 11)])
        case("unicode-instrument", [("仪器:1:F:1:11:0:0", "5555", 11)])
        case("unicode-arabic-digits", [record("١٢", score=10), record("-٢", score=30)])
        case("unicode-fullwidth-digits", [record("１２", score=10), record("１3", score=30)])
        case("unicode-supplementary-digit", [("I:1:F:1:𝟙:0:0", "5555", None)])
        for difference in (9, 10, 11, 19, 20, 21):
            case(f"threshold-{difference}", [record(1, score=0), record(2, score=difference)])
        case("unequal-tile-counts", [record(1, score=10)] * 103 + [record(2, score=30)])
        case("fractional-means", [record(1, score=10), record(1, score=20), record(1, score=20), record(2, score=16), record(3, score=17)])
        case("tiny-roundoff", [r for tile in (1, 2, 3) for r in [record(tile, score=0)] * 9 + [record(tile, score=1)]])
        for length in (74, 75, 76, 86, 87, 150):
            case(f"group-{length}", [record(1, length), record(2, length)], check_groups=True)
        for length, interval in ((660, 20), (1400, 50), (4000, 100)):
            quality = "5" * 9 + "!" * (interval - 10) + "5" * (length - interval + 1)
            case(f"long-group-{length}", [record(1, length, quality), record(2, length)], check_groups=True)
        case("nogroup", [record(1, 150), record(2, 150, 30)], options=("--nogroup",))
        case("phred64", [record(1, score=40), record(2, score=60)], options=("--phred64",))
        case("gzip", [record(1), record(2, score=40)], compressed=True)
        case("missing-tail", [record(1, 4), record(2, 8)], corrected=True)
        case("missing-tail-reversed", [record(2, 8), record(1, 4)], corrected=True)
        case("observed-zero", [record(1, 8, 0), record(2, 8, 20)])
        case("missing-group", [record(1, 10), record(2, 150)], corrected=True)
        case("missing-long-group", [record(1, 15), record(2, 660)], corrected=True)
        case("extract-delete", [record(1), record(2, score=30)], options=("--delete",))
        case("min-length-before-header", [("invalid", "!", None), record(1)], options=("--min_length", "4"))
        case("max-length-before-header", [("invalid", "!" * 8, None), record(1)], options=("--max_length", "4"))
        case("all-filtered", [record(1)], options=("--min_length", "10"))
        first = [record(1, 1)] * 10_000
        case("sampling-unsampled-header", first + [("invalid", "!", None)] * 9 + [record(2, 1, 0)])
        case("sampling-invalid-header", first + [record(2, 1, 0)] * 9 + [("invalid", "!", None)])
        case("sampling-unsampled-length", first + [record(1, 150, 0)] * 9 + [record(2, 1, 0)])
        case("sampling-10000", first + [record(2, 1, 0)] * 9)
        case("sampling-10010", first + [record(2, 1, 0)] * 10)
        case("tile-cap-2501", [record(tile, 1) for tile in range(2501)])
        case("tile-cap-2502", [record(tile, 1) for tile in range(2502)])
        invalid = root / "invalid-phred64.fq"
        invalid.write_text("@I:1:F:1:1:0:0\nACGT\n+\n!!!!\n")
        exits = []
        for label, binary in (("reference", upstream), ("candidate", rust)):
            outdir = root / (label + "-invalid-phred64")
            outdir.mkdir()
            run = subprocess.run([binary, "--quiet", "--extract", "--phred64", "--outdir", str(outdir), str(invalid)], capture_output=True, text=True, timeout=30)
            if run.returncode != 1 or "Phred64" not in run.stderr:
                raise AssertionError((label, "invalid Phred64 was not rejected", run.returncode, run.stderr))
            if label == "candidate" and list(outdir.iterdir()):
                raise AssertionError("native invalid Phred64 created reports")
            exits.append(run.returncode)
        cases.append({"case": "invalid-phred64", "verdict": "invalid-quality-rejected", "exits": exits})
        print("PASS invalid-phred64: invalid-quality-rejected", flush=True)
    counts = {verdict: sum(case["verdict"] == verdict for case in cases) for verdict in ("exact", "corrected-missing-observation", "invalid-quality-rejected")}
    print(f"PASS FastQC tiles: {len(cases)} cases; {counts}")
    if args.report:
        args.report.write_text(json.dumps({"reference": "FastQC 0.13.0", "counts": counts, "cases": cases}, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
