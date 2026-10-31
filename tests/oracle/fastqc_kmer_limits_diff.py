#!/usr/bin/env python3
"""FastQC 0.13.0 limits comparisons and independently calculated k-mer proofs.

No fixtures or prior comparison expectations are changed. The mathematical
model sums binomial masses with Decimal, independently of Rust's log recurrence.
Raw blocks from both executables are retained in the optional JSON report.
"""
import argparse
from collections import Counter, defaultdict
from decimal import Decimal, localcontext
import gzip
from fractions import Fraction
from html import unescape
import hashlib
import json
import math
import random
from pathlib import Path
import struct
import subprocess
import tempfile
import zipfile

from fastqc_tile_diff import blocks, groups

NAME = "Kmer Content"
HEADER = "#Sequence\tCount\tPValue\tObs/Exp Max\tMax Obs/Exp Position"
KEYS = {
    "quality_base": "Per base sequence quality",
    "tile": "Per tile sequence quality",
    "quality_sequence": "Per sequence quality scores",
    "sequence": "Per base sequence content",
    "gc_sequence": "Per sequence GC content",
    "n_content": "Per base N content",
    "sequence_length": "Sequence Length Distribution",
    "duplication": "Sequence Duplication Levels",
    "overrepresented": "Overrepresented sequences",
    "adapter": "Adapter Content",
    "kmer": NAME,
}


def f32(value):
    return struct.unpack("f", struct.pack("f", float(value)))[0]


def binomial(n, hits, p, exclusive=False):
    """Sum the PMFs directly; the observed outcome is included by default."""
    with localcontext() as ctx:
        ctx.prec = 80
        q = 1 - p
        return sum((Decimal(math.comb(n, j)) * p**j * q**(n-j)
                    for j in range(hits + int(exclusive), n + 1)), Decimal(0))


def model(sequences, nogroup=False, warn=2, error=5, buggy=False, limit=20):
    sampled = [s[:500].upper() for s in sequences[49::50]]
    longest = max(map(len, sampled), default=0)
    if longest < 7:
        return "pass", []
    bins = groups(longest - 6, nogroup)
    words = Counter()
    positions = defaultdict(Counter)
    opportunities = Counter()
    extent = {}
    for seq in sampled:
        for i in range(len(seq) - 6):
            word = seq[i:i+7]
            if "N" in word:
                continue
            opportunities[i] += 1
            words[word] += 1
            positions[word][i] += 1
            extent[word] = max(extent.get(word, len(seq)-6), i+1)
    total = sum(opportunities.values())
    rows = []
    for word, count in words.items():
        with localcontext() as ctx:
            ctx.prec = 80
            p = Decimal(count) / total
            if buggy:
                p = Decimal(f32(p))
            best, location, smallest = Decimal(0), None, Decimal(1)
            for start, end in bins:
                stop = min(end, extent[word]) if buggy else end
                n = sum(opportunities[i] for i in range(start-1, stop))
                hits = sum(positions[word][i] for i in range(start-1, stop))
                if not n:
                    continue
                expected = p * n
                if buggy:
                    expected = Decimal(f32(expected))
                if not expected:
                    continue
                ratio = Decimal(f32(Decimal(hits) / expected)) if buggy else Fraction(hits * total, count * n)
                if ratio > best:
                    best, location = ratio, str(start) if start == end else f"{start}-{end}"
                if ratio <= 5:
                    continue
                tail = binomial(n, hits, p, exclusive=buggy)
                if buggy:
                    # Java forms 1-CDF in binary64, then casts the correction to
                    # float. Permit its documented cancellation in this model.
                    tail = Decimal(1.0 - float(1-tail))
                corrected = min(Decimal(1), tail * 4**7)
                if corrected < Decimal("0.01"):
                    smallest = min(smallest, corrected)
            if smallest < Decimal("0.01"):
                rows.append((word, count * (5 if buggy else 50), smallest, best, location))
    most_significant = min((row[2] for row in rows), default=None)
    rows.sort(key=lambda row: (-row[3], row[0]))
    if limit is not None:
        rows = rows[:limit]
    # Fixtures keep ties at the cutoff out of upstream comparisons. Corrected
    # native ties are lexical and checked explicitly.
    with localcontext() as ctx:
        ctx.prec = 80
        selected_p = rows[0][2] if buggy and rows else most_significant
        significance = -selected_p.log10() if selected_p is not None else None
        status = "fail" if significance is not None and significance > error else "warn" if significance is not None and significance > warn else "pass"
    return status, rows


def check_model(actual, expected, ordered=True):
    status, lines = actual
    want_status, expected_rows = expected
    assert status == want_status, ("status", status, want_status)
    if not expected_rows:
        assert lines == [], ("unexpected rows", lines)
        return
    assert lines[0] == HEADER
    actual_rows = [line.split("\t") for line in lines[1:]]
    assert all(len(row) == 5 for row in actual_rows)
    if not ordered:
        actual_rows.sort(key=lambda row: row[0])
        expected_rows = sorted(expected_rows)
    assert len(actual_rows) == len(expected_rows), ("row count", actual_rows, expected_rows)
    for row, want in zip(actual_rows, expected_rows):
        word, count, p, ratio, position = want
        assert row[0] == word and int(row[1]) == count and row[4] == position, (row, want)
        # Report fields are binary32, as in FastQC. Verdicts retain the full
        # log probability, so a displayed 0.0 does not force infinite significance.
        assert math.isclose(float(row[2]), f32(p), rel_tol=3e-5, abs_tol=2e-45 if ordered else 2e-11), (row, want)
        assert math.isclose(float(row[3]), f32(ratio), rel_tol=2e-7), (row, want)
        for value in row[2:4]:
            assert value not in ("NaN", "Infinity", "-Infinity")
            assert "." in value and "e" not in value and "+" not in value


def execute(binary, path, directory, limits, options=(), native=False):
    directory.mkdir()
    args = [binary, "-q", "--extract", "--outdir", str(directory)]
    if limits is not None:
        args += ["-l", str(limits)]
    run = subprocess.run(args + list(options) + [str(path)], capture_output=True, text=True, timeout=60)
    assert run.returncode == 0, (binary, run.returncode, run.stderr)
    files = list(directory.glob("*/fastqc_data.txt"))
    assert len(files) == 1, (binary, list(directory.rglob("*")))
    data = files[0].read_text()
    parsed = blocks(data)
    summary = (files[0].parent / "summary.txt").read_text()
    assert summary == "".join(f"{status.upper()}\t{name}\t{path.name}\n" for name, (status, _) in parsed.items())
    archives = list(directory.glob("*.zip"))
    if "--delete" in options:
        assert not archives
    else:
        assert len(archives) == 1
        with zipfile.ZipFile(archives[0]) as archive:
            assert archive.read(files[0].parent.name + "/fastqc_data.txt").decode() == data
            assert archive.read(files[0].parent.name + "/summary.txt").decode() == summary
    html_files = list(directory.glob("*.html"))
    assert len(html_files) == 1
    if native:
        html = unescape(html_files[0].read_text())
        for name, (status, lines) in parsed.items():
            assert f'<h2>{name}</h2>\n<p>Status: <span class="{status}">{status.upper()}</span></p>\n<pre>' in html
            assert "<pre>" + "".join(line+"\n" for line in lines) + "</pre>" in html
    return parsed, run.stderr


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fastqc", default="tests/ci/env/bin/fastqc")
    parser.add_argument("--rust", default="target/release/rust-fastqc")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--only-case", help="run only labels containing this substring (targeted debugging)")
    args = parser.parse_args()
    if args.report:
        args.report.unlink(missing_ok=True)
    upstream, rust = str(Path(args.fastqc).resolve()), str(Path(args.rust).resolve())
    version = subprocess.check_output([upstream, "--version"], text=True).strip()
    assert version == "FastQC v0.13.0", version
    cases = []
    with tempfile.TemporaryDirectory(prefix="fastqc-kmer-limits-") as tmp:
        root = Path(tmp)

        def case(label, seqs, config="kmer ignore 1\n", modules=(), options=(), quality=None, kmer=False, warn=2, error=5, compressed=False, tiles=False):
            if args.only_case and args.only_case not in label:
                return {}, {}
            folder = root / label
            folder.mkdir()
            text = "".join(f"@I:1:F:1:{1+i%2 if tiles else 1}:0:{i}\n{s}\n+\n{quality[i] if quality else '5'*len(s)}\n" for i, s in enumerate(seqs))
            path = folder / ("reads.fq.gz" if compressed else "reads.fq")
            path.write_bytes(gzip.compress(text.encode()) if compressed else text.encode())
            limits = folder / "limits.txt" if config is not None else None
            if limits is not None:
                limits.write_text(config)
            ref, ref_err = execute(upstream, path, folder / "ref", limits, options)
            got, got_err = execute(rust, path, folder / "rust", limits, options, native=True)
            assert list(ref) == list(got), (label, "module order/presence", list(ref), list(got))
            assert ref_err == got_err, (label, "diagnostics", ref_err, got_err)
            for name in modules:
                assert ref[name] == got[name], (label, name, ref[name], got[name])
            verdict = "exact"
            if kmer:
                selected = seqs
                if "--min_length" in options:
                    selected = [s for s in selected if len(s) >= int(options[options.index("--min_length")+1])]
                if "--max_length" in options:
                    selected = [s for s in selected if len(s) <= int(options[options.index("--max_length")+1])]
                correct = model(selected, "--nogroup" in options, warn, error)
                check_model(got[NAME], correct)
                _, eligible = model(selected, "--nogroup" in options, warn, error, buggy=True, limit=None)
                lookup = {row[0]: row for row in eligible}
                selected_rows = [lookup[line.split("\t")[0]] for line in ref[NAME][1][1:]] if eligible else []
                assert len(selected_rows) == min(20, len(eligible))
                assert len({r[0] for r in selected_rows}) == len(selected_rows)
                if selected_rows:
                    assert all(row[3] >= eligible[min(19,len(eligible)-1)][3] for row in selected_rows)
                    assert all(a[3] >= b[3] for a,b in zip(selected_rows,selected_rows[1:]))
                    first_p = f32(selected_rows[0][2])
                    significance = -math.log10(first_p) if first_p else math.inf
                    status = "fail" if significance > error else "warn" if significance > warn else "pass"
                else:
                    status = "pass"
                check_model(ref[NAME], (status, selected_rows), ordered=False)
                if ref[NAME] != got[NAME]:
                    verdict = "proved-kmer-correction"
            else:
                assert {k:v[0] for k,v in ref.items()} == {k:v[0] for k,v in got.items()}, (label, "verdicts", ref, got)
            cases.append({"case": label, "verdict": verdict, "input_sha256": hashlib.sha256(text.encode()).hexdigest(), "limits": config, "options": list(options),
                          "upstream": ref.get(NAME) if kmer else {name:ref[name] for name in modules},
                          "rust": got.get(NAME) if kmer else {name:got[name] for name in modules}})
            print(f"PASS {label}: {verdict}", flush=True)
            return ref, got

        base = ["ACGT"*25]*102
        override_file = root / "override-limits.txt"
        override_file.write_text("kmer ignore 1\nsequence ignore 1\n")
        case("repeated-limits-option", base, "broken\n", options=("-l", str(override_file)))
        case("empty-limits-option", base, "broken\n", options=("--limits", ""))
        case("all-filtered-negative-limits", base, "kmer ignore 0\nn_content error -1\nsequence error -1\n", ["Basic Statistics", KEYS["duplication"], KEYS["n_content"], KEYS["sequence"]], options=("--min_length", "200"), kmer=True)
        case("all-n-negative-limits", ["N"*100]*100, "kmer ignore 1\nsequence error -1\n", [KEYS["sequence"]])
        for key, title in KEYS.items():
            ref, got = case("ignore-"+key, base, f"kmer ignore 1\n{key} ignore 1\n")
            assert title not in got
        case("ignore-all", base, "\n".join(f"{k} ignore 1" for k in KEYS))
        for value in ("0", "-1", "NaN", "0.1"):
            case("ignore-value-"+value, base, f"kmer ignore 1\nsequence ignore {value}\n")
        case("last-override", base, "kmer ignore 1\nsequence ignore 1\nsequence ignore 0\n", [KEYS["sequence"]])
        case("empty-custom-defaults", base, "", [KEYS["quality_sequence"]], kmer=True)
        case("bundled-defaults", base, None, [KEYS["quality_sequence"]])
        for label, config in (
            ("cr-lines", "kmer ignore 1\rsequence warn 100\rsequence error 100\r"),
            ("tabs-and-comments", "# comment\n\tkmer ignore 1\nkmer\tignore\t1\n\nsequence warn 100\n"),
            ("unknown-key", "kmer ignore 1\nfuture warn 1\nfuture ignore 1\n"),
            ("invalid-level", "kmer ignore 1\nsequence warning 0\n"),
            ("short-invalid-level", "kmer ignore 1\nsequence warning\n"),
            ("invalid-number", "kmer ignore 1\nsequence warn nope\n"),
            ("extra-field", "kmer ignore 1\nsequence warn 100 extra\n"),
            ("float-suffix", "kmer ignore 1\nsequence warn 100f\nsequence error 100D\n"),
            ("hex-zero", "kmer ignore 1\nn_content warn 100\nn_content error 0x0p2048\n"),
            ("hex-fraction", "kmer ignore 1\nn_content warn 0x1.8p4\nn_content error 0x1.ap4\n"),
            ("special-values", "kmer ignore 1\nsequence warn NaN\nsequence error Infinity\n"),
        ):
            case(label, ["ACGN"*25]*102, config, [KEYS["sequence"], KEYS["n_content"]])
        targets = {
            "quality_base_lower": (base, KEYS["quality_base"], 20),
            "quality_base_median": (base, KEYS["quality_base"], 20),
            "quality_sequence": (base, KEYS["quality_sequence"], 20),
            "n_content": (["ACGN"*25]*102, KEYS["n_content"], 100),
            "sequence": (["A"*100]*102, KEYS["sequence"], 100),
            "gc_sequence": (base, KEYS["gc_sequence"], 30),
            "duplication": (base, KEYS["duplication"], 100/102),
            "overrepresented": (base, KEYS["overrepresented"], 100),
            "adapter": (["AGATCGGAAGAG"+"T"*88]*102, KEYS["adapter"], 100),
            "tile": (base, KEYS["tile"], 10),
        }
        for key, (seqs, name, boundary) in targets.items():
            for delta in (-1, 0, 1):
                for level in ("warn", "error"):
                    neutral = "quality_base_lower warn -1\nquality_base_lower error -1\nquality_base_median warn -1\nquality_base_median error -1\n" if key.startswith("quality_base_") else ""
                    low = key.startswith("quality_") or key == "duplication"
                    neutral += f"{key} warn {'-1' if low else '1000'}\n{key} error {'-1' if low else '1000'}\n"
                    quality = [("+" if i%2 == 0 else "?")*100 for i in range(len(seqs))] if key == "tile" else None
                    case(f"limit-{key}-{level}-{delta}", seqs, "kmer ignore 1\n"+neutral+f"{key} {level} {boundary+delta}\n", [name], quality=quality, tiles=(key == "tile"))
        for level, expected_status in (("warn", "warn"), ("error", "fail")):
            _, got = case("gc-explicit-"+level, base,
                          f"kmer ignore 1\ngc_sequence warn 1000\ngc_sequence error 1000\ngc_sequence {level} 0\n", [KEYS["gc_sequence"]])
            if got:
                assert got[KEYS["gc_sequence"]][0] == expected_status
        for value in ("0", "1", "-1", "NaN"):
            case("length-switch-"+value, ["ACGT"*20, "ACGT"*21], f"kmer ignore 1\nsequence_length warn {value}\n", [KEYS["sequence_length"], "Basic Statistics"])
        for value in ("0", "1", "-1", "NaN"):
            case("empty-length-error-"+value, ["", "ACGT"*25], f"kmer ignore 1\nsequence_length error {value}\n", [KEYS["sequence_length"], KEYS["gc_sequence"], "Basic Statistics"])
        # The short-read count threshold masks one failing position in the old
        # native implementation; upstream includes only observed percentiles.
        seqs = ["ACGTACGTAC"]*101 + ["ACGT"*20]*100
        quality = ["I"*9+"!"+"I"*(len(s)-10) for s in seqs]
        _, got = case("grouped-quality-observations", seqs, "kmer ignore 1\n", [KEYS["quality_base"]], quality=quality)
        if got:
            assert got[KEYS["quality_base"]][0] == "fail"
            assert "10-11\t20.0\t0.0\t0.0\t0.0\t0.0\t0.0" in got[KEYS["quality_base"]][1]

        case("strict-fivefold-boundary", ["AAAAAAACCCC"]*900+["TTTTTTTCCCC"]*100, "", kmer=True)
        tie_a = "A"*7+"C"+"G"*7+"C"*91
        tie_t = "T"*7+"C"+"G"*7+"C"*91
        case("rational-ties-different-counts", [tie_a]*350+[tie_t]*150, "", kmer=True)
        common = ("A"*7+"T"*28)*2
        rng = random.Random(0)
        rare = "".join("A"*7+"T"*6+"".join(rng.choice("ACG") for _ in range(16))+"T"*6 for _ in range(2))
        _, got = case("significance-before-top20", [common]*2300+[rare]*200, "", kmer=True)
        if got:
            assert got[NAME][0] == "fail" and len(got[NAME][1]) == 21
            assert all(float(row.split("\t")[2]) > 1e-5 for row in got[NAME][1][1:]), "fixture must exclude the failing word from display"
        motif = "A"*7 + "C"*99
        for samples in (0, 1, 2, 3, 4, 8):
            case(f"sample-{samples}", [motif]*max(1,samples*50), "kmer ignore 0\n", kmer=True)
        case("sample-49", [motif]*49, "", kmer=True)
        case("sample-99", [motif]*99, "", kmer=True)
        case("sample-only-fiftieth", ["T"*106]*49+[motif]+["G"*106]*49, "", kmer=True)
        case("constant-no-enrichment", ["A"*100]*200, "kmer warn -2\nkmer error -1\n", kmer=True, warn=-2, error=-1)
        case("all-n", ["N"*100]*200, "", kmer=True)
        case("lowercase-n", ["nnn"+motif.lower()]*200, "", kmer=True)
        case("shorter-than-word", ["ACGTAC"]*200, "", kmer=True)
        for length in (81, 82, 500, 501, 660):
            case(f"prefix-{length}", [(motif+"G"*660)[:length]]*200, "", kmer=True)
        rng = random.Random(731)
        varied = "".join(rng.choice("ACGT") for _ in range(70))
        case("top-20-ties", [varied]*200, "", kmer=True)
        case("nogroup", [motif]*200, "", options=("--nogroup",), kmer=True)
        case("gzip", [motif]*200, "", compressed=True, kmer=True)
        case("extract-delete", [motif]*200, "", options=("--delete",), kmer=True)
        case("filter-before-sampling", ["A"]*49+[motif]*200, "", options=("--min_length","100"), kmer=True)
        case("max-filter-before-sampling", ["T"*200]*49+[motif]*200, "", options=("--max_length","106"), kmer=True)
        short, long = "C"*9+"A"*7, "C"*9+"A"*7+"G"*90
        forward = case("opportunities-forward", [short]*200+[long]*200, "", kmer=True)
        reverse = case("opportunities-reverse", [long]*200+[short]*200, "", kmer=True)
        if forward[1] and reverse[1]:
            assert forward[1][NAME] == reverse[1][NAME], "candidate opportunity denominator depends on order"
            assert forward[0][NAME] != reverse[0][NAME], "upstream opportunity defect did not reproduce"
            assert forward[0][NAME][0] == "warn" and forward[1][NAME][0] == "fail", "most-significant verdict proof did not reproduce"
        for warn, error in ((2,5), (4,5), (1,2), (320,323)):
            samples = 163 if warn == 320 else 4
            case(f"kmer-limits-{warn}-{error}", [motif]*(samples*50), f"kmer warn {warn}\nkmer error {error}\n", kmer=True, warn=warn, error=error)
        # Early rejection is supported by an explicit partial-configuration
        # reproduction: Java silently ignores the requested final disable.
        folder = root / "partial-config"
        folder.mkdir()
        path = folder / "reads.fq"
        path.write_text("@r\nACGTACGTACGTACGT\n+\n5555555555555555\n")
        limits = folder / "limits.txt"
        limits.write_text("sequence ignore 1\nkmer ignore\nadapter ignore 1\n")
        ref, err = execute(upstream, path, folder/"ref", limits)
        assert "ArrayIndexOutOfBoundsException" in err
        assert KEYS["sequence"] not in ref and KEYS["adapter"] in ref
        out = folder / "rust"
        out.mkdir()
        run = subprocess.run([rust, "-l", str(limits), "-o", str(out), str(path)], capture_output=True, text=True)
        assert run.returncode == 3 and "three fields" in run.stderr and not list(out.iterdir())
        cases.append({"case":"partial-config", "verdict":"proved-config-rejection", "upstream_stderr":err, "rust_stderr":run.stderr})
        print("PASS partial-config: proved-config-rejection", flush=True)
        for label, config_path in (("missing-config", folder / "missing-limits.txt"), ("directory-config", folder)):
            observations = []
            for binary in (upstream, rust):
                outdir = folder / (label + ("-ref" if binary == upstream else "-rust"))
                outdir.mkdir()
                run = subprocess.run([binary, "-l", str(config_path), "--outdir", str(outdir), str(path)], capture_output=True, text=True)
                assert run.returncode == 1 and not run.stdout and not list(outdir.iterdir())
                observations.append(run.stderr)
            assert observations[0] == observations[1], (label, observations)
            cases.append({"case":label, "verdict":"exact-preflight-rejection", "stderr":observations[0]})
            print(f"PASS {label}: exact-preflight-rejection", flush=True)
    counts = dict(Counter(c["verdict"] for c in cases))
    print(f"PASS FastQC limits/kmer: {len(cases)} cases; {counts}")
    if args.report:
        args.report.write_text(json.dumps({"reference":version,"counts":counts,"cases":cases}, indent=2)+"\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
