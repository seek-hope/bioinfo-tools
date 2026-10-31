#!/usr/bin/env python3
"""FastQC 0.13.0 Adapter Content differential plus exact-length defect proof.

Compare the complete Adapter Content data block, its summary verdict and custom
list diagnostics. The older 22-fixture QC suite excludes this module and remains
unchanged. Approved boundary cases are additionally checked against independent
literal-substring counts, never treated as raw byte-equivalence passes.
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile

DEFAULTS = [
    ("Illumina Universal Adapter", "AGATCGGAAGAG"),
    ("Illumina Small RNA 3' Adapter", "TGGAATTCTCGG"),
    ("Illumina Small RNA 5' Adapter", "GATCGTCGGACT"),
    ("Nextera Transposase Sequence", "CTGTCTCTTATA"),
    ("PolyA", "AAAAAAAAAAAA"), ("PolyG", "GGGGGGGGGGGG"),
]


def adapter_block(text):
    marker = ">>Adapter Content\t"
    if marker not in text:
        return None
    suffix = text[text.index(marker):]
    if ">>END_MODULE\n" not in suffix:
        return None
    return suffix[:suffix.index(">>END_MODULE\n") + len(">>END_MODULE\n")]


def independent_small_block(sequences, adapters):
    """Direct count: read has a literal hit at/before each valid start position."""
    longest = max(len(sequence) for _, sequence in adapters)
    size = max(map(len, sequences), default=0)
    if size < longest:
        return ">>Adapter Content\twarn\n>>END_MODULE\n"
    assert size <= 74, "this independent model intentionally avoids grouped rows"
    lines = ["#Position\t" + "\t".join(name for name, _ in adapters)]
    worst = 0.0
    for position in range(size - longest + 1):
        values = []
        for _, adapter in adapters:
            hits = sum(0 <= sequence.upper().find(adapter) <= position for sequence in sequences)
            percent = 100.0 * hits / len(sequences)
            worst = max(worst, percent)
            values.append(str(percent))
        lines.append(str(position + 1) + "\t" + "\t".join(values))
    status = "fail" if worst > 10 else "warn" if worst > 5 else "pass"
    return f">>Adapter Content\t{status}\n" + "\n".join(lines) + "\n>>END_MODULE\n"


def config_diagnostics(stderr):
    return [line for line in stderr.splitlines()
            if line.startswith(("Expected 2 sections", "[Warning] You are using adapter sequences"))]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fastqc", default="tests/ci/env/bin/fastqc")
    parser.add_argument("--rust", default="target/debug/rust-fastqc")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    upstream, rust = [str(Path(path).resolve()) for path in (args.fastqc, args.rust)]
    version = subprocess.check_output([upstream, "--version"], text=True).strip()
    if version != "FastQC v0.13.0":
        raise SystemExit(f"wrong reference: {version}")
    records = []
    with tempfile.TemporaryDirectory(prefix="fastqc-adapters-") as tmp:
        root = Path(tmp)

        def case(label, sequences, config=None, definitions=None, options=(), boundary=False):
            directory = root / label
            directory.mkdir()
            infile = directory / "reads.fq"
            infile.write_text("".join(f"@r{i}\n{seq}\n+\n{'!' * len(seq)}\n" for i, seq in enumerate(sequences)))
            config_options = []
            if config is not None:
                path = directory / "adapters.tsv"
                path.write_text(config)
                config_options = ["--adapters", str(path)]
            observed = []
            for which, binary in (("upstream", upstream), ("rust", rust)):
                outdir = directory / which
                outdir.mkdir()
                run = subprocess.run([binary, "--quiet", "--extract", "--outdir", str(outdir),
                                      *config_options, *options, str(infile)], capture_output=True, text=True, timeout=30)
                report = outdir / "reads_fastqc" / "fastqc_data.txt"
                text = report.read_text() if report.exists() else ""
                block = adapter_block(text)
                summary_file = outdir / "reads_fastqc" / "summary.txt"
                summary = [line for line in summary_file.read_text().splitlines() if "\tAdapter Content\t" in line] if summary_file.exists() else []
                observed.append((run.returncode, block, summary, config_diagnostics(run.stderr)))
            reference, candidate = observed
            if reference[0] != 0 or candidate[0] != 0 or reference[1] is None or candidate[1] is None:
                raise AssertionError((label, observed))
            if max(map(len, sequences)) <= 74:
                expected = independent_small_block(sequences, definitions or DEFAULTS)
                if candidate[1] != expected:
                    raise AssertionError((label, "independent substring counts", candidate[1], expected))
            if reference[3] != candidate[3]:
                raise AssertionError((label, "config diagnostics", observed))
            if boundary:
                if reference[1] == expected:
                    raise AssertionError((label, "upstream counting defect not reproduced"))
                verdict = "approved-counting-difference"
            else:
                if reference != candidate:
                    raise AssertionError((label, observed))
                verdict = "exact"
            # The report and summary must agree, including exception cases.
            expected_status = candidate[1].splitlines()[0].split("\t")[1].upper()
            if candidate[2] != [expected_status + "\tAdapter Content\treads.fq"]:
                raise AssertionError((label, "summary/data disagreement", candidate))
            records.append({"case": label, "verdict": verdict, "upstream": reference[1], "rust": candidate[1]})
            print(f"PASS {label}: {verdict}", flush=True)

        for length in (1, 4, 11, 13, 14, 20, 74, 75, 80, 150):
            case(f"default-length-{length}", [("ACGT" * 50)[:length]])
        for length in (13, 20, 80, 150):
            case(f"default-hit-{length}", ["A" * length, "G" * length, "T" * length])
        for seq in ("AGATCGGAAGAG", "ACGTACGTACGT"):
            case("default-equal-" + seq, [seq], boundary=True)
        case("equal-before-long", ["AGATCGGAAGAG", "T" * 25], boundary=True)
        case("long-before-equal", ["T" * 25, "AGATCGGAAGAG"])
        before = next(row for row in records if row["case"] == "equal-before-long")
        after = next(row for row in records if row["case"] == "long-before-equal")
        if before["rust"] != after["rust"] or before["upstream"] == after["upstream"]:
            raise AssertionError("read-order invariant: Rust must agree; pinned upstream defect must reproduce")
        print("PASS read-order invariant: Rust is unchanged; upstream incorrectly depends on order", flush=True)
        case("nogroup-long", ["A" * 150, "T" * 150], options=("--nogroup",))
        case("small-custom", ["ACGTAC", "TTACGT", "CCCCCC"], "motif\tACG\n", [("motif", "ACG")])
        case("custom-short", ["AC", "TT"], "motif\tACG\n", [("motif", "ACG")])
        case("custom-equal-hit", ["ACG"], "motif\tACG\n", [("motif", "ACG")], boundary=True)
        case("custom-equal-none", ["TTT"], "motif\tACG\n", [("motif", "ACG")], boundary=True)
        case("custom-mixed-lengths", ["TTAACC", "CCCCAA"], "short\tAA\nlong\tCCCC\n", [("short", "AA"), ("long", "CCCC")])
        case("mixed-growing", ["TTTTAA", "TTTTTTTT"], "short\tAA\nlong\tCCCC\n", [("short", "AA"), ("long", "CCCC")], boundary=True)
        case("mixed-growing-reversed", ["TTTTTTTT", "TTTTAA"], "short\tAA\nlong\tCCCC\n", [("short", "AA"), ("long", "CCCC")])
        before = next(row for row in records if row["case"] == "mixed-growing")
        after = next(row for row in records if row["case"] == "mixed-growing-reversed")
        if before["rust"] != after["rust"] or before["upstream"] == after["upstream"]:
            raise AssertionError("mixed adapter lengths: Rust must preserve read-order invariance")
        case("short-late-hit-clipped", ["TTTTAA"], "short\tAA\nlong\tCCCC\n", [("short", "AA"), ("long", "CCCC")])
        case("case-sensitive-config", ["acgtac", "ACGTAC"], "lower\tacg\nupper\tACG\n", [("lower", "acg"), ("upper", "ACG")])
        case("literal-N", ["NNNACG", "AAAACG"], "amb\tNNN\n", [("amb", "NNN")])
        case("repeated-names", ["AAACCC", "CCCAAA"], "motif\tAAA\nmotif\tCCC\n", [("motif", "AAA"), ("motif", "CCC")])
        case("tabs-comments", ["AAACCC"], "# comment\n\nname\t\tAAA\t\n", [("name", "AAA")])
        case("cr-final", ["ACGTAC"], "motif\tACG\r", [("motif", "ACG")])
        case("cr-two-entries", ["AAACCC"], "first\tAAA\rsecond\tCCC\r", [("first", "AAA"), ("second", "CCC")])
        case("crlf-mixed", ["AAACCC"], "# comment\r\nfirst\tAAA\r\nsecond\tCCC\n", [("first", "AAA"), ("second", "CCC")])
        case("non-ascii-blank-row", ["AAACCC"], "\u00a0\nvalid\tAAA\n", [("valid", "AAA")])
        case("malformed-rows", ["AAACCC"], "bad\nwrong\tAAA\textra\nname\tAAA\n", [("name", "AAA")])
        case("empty-name", ["AAACCC"], "\tAAA\n", [("", "AAA")])
        case("config-spaces", ["AAACCC"], " spaced name \tAAA\n", [(" spaced name ", "AAA")])
        case("unicode-name", ["AAACCC"], "α接头 & motif\tAAA\n", [("α接头 & motif", "AAA")])
        case("custom-growth", ["AAACC", "AAACCCCCCC", "TTTTT"], "short\tAAA\n", [("short", "AAA")])
        case("custom-growth-reverse", ["TTTTT", "AAACCCCCCC", "AAACC"], "short\tAAA\n", [("short", "AAA")])

        # Invalid empty configuration: an explicit failure with no report is
        # required. The upstream instead throws while constructing the report.
        for label, text in (("empty", ""), ("comments-only", "# none\n"), ("invalid-only", "not-a-tab-separated-entry\n")):
            directory = root / ("invalid-" + label)
            directory.mkdir()
            infile = directory / "reads.fq"
            infile.write_text("@r\nACGTACGTACGTACGT\n+\n!!!!!!!!!!!!!!!!\n")
            config = directory / "adapters.tsv"
            config.write_text(text)
            results = []
            for which, binary in (("upstream", upstream), ("rust", rust)):
                outdir = directory / which
                outdir.mkdir()
                run = subprocess.run([binary, "--quiet", "--extract", "--outdir", str(outdir),
                                      "--adapters", str(config), str(infile)], capture_output=True, text=True, timeout=30)
                results.append((run, list(outdir.rglob("*"))))
            original, corrected = results
            if corrected[0].returncode != 3 or corrected[1] or "no usable name<TAB>sequence entries" not in corrected[0].stderr:
                raise AssertionError((label, corrected))
            if "ArrayIndexOutOfBoundsException" not in original[0].stderr:
                raise AssertionError((label, "upstream empty-list defect not reproduced", original[0]))
            records.append({"case": "invalid-" + label, "verdict": "approved-config-rejection",
                            "upstream_exit": original[0].returncode, "rust_exit": corrected[0].returncode})
            print(f"PASS invalid-{label}: approved-config-rejection", flush=True)
    counts = {key: sum(row["verdict"] == key for row in records)
              for key in ("exact", "approved-counting-difference", "approved-config-rejection")}
    print(f"PASS FastQC adapters: {len(records)} cases; {counts}")
    if args.report:
        args.report.write_text(json.dumps({"reference": version, "counts": counts, "read_order_invariant": True, "cases": records}, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
