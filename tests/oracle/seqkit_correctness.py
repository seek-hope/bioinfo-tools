#!/usr/bin/env python3
"""Independent correctness checks for approved SeqKit 2.14.0 exceptions.

The reference is kept unmodified. Strand checks compare negative translation
with positive translation of an independently complemented input. ORF checks
project protein spans onto the source sequence. Range checks select from typed
input records and require per-file composition and quality provenance. Raw
upstream differences are reported separately from correctness assertions.
"""

import argparse
from collections import Counter
import itertools
import json
from pathlib import Path
import re
import subprocess
import tempfile

DNA = b"ACGTRYSWKMBDHVN"
COMPLEMENT = bytes.maketrans(b"ACGTRYSWKMBDHVNUacgtryswkmbdhvnu", b"TGCAYRSWMKVHDBNAtgcayrswmkvhdbna")
TABLES = (1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14, 16, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31)

if not __debug__:
    raise SystemExit("correctness checks require assertions; unset PYTHONOPTIMIZE and do not use -O")


def rc(sequence):
    return sequence.translate(COMPLEMENT)[::-1]


def fasta_records(payload):
    records = []
    for line in payload.splitlines():
        if line.startswith(b">"):
            records.append([line[1:], b""])
        elif records:
            records[-1][1] += line
        else:
            raise AssertionError(f"sequence before header: {line!r}")
    return records


def fasta(records):
    return b"".join(b">" + name + b"\n" + sequence + b"\n" for name, sequence in records)


def wrapped(sequence, width):
    return (b"\n".join(sequence[i:i + width] for i in range(0, len(sequence), width))
            if width else sequence) + b"\n"


def render(records, width):
    output = bytearray()
    for name, sequence, quality in records:
        output += (b">" if quality is None else b"@") + name + b"\n" + wrapped(sequence, width)
        if quality is not None:
            assert len(quality) == len(sequence)
            output += b"+\n" + wrapped(quality, width)
    return bytes(output)


def select(records, region):
    start, end = map(int, region.split(":"))
    lo = start - 1 if start > 0 else len(records) + start
    hi = end if end > 0 else len(records) + end + 1
    # Cases here are in bounds: no upstream short-input circular-tail rule.
    assert 0 <= lo <= hi <= len(records)
    return records[lo:hi]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seqkit", default="tests/ci/env/bin/seqkit")
    parser.add_argument("--rust", default="target/debug/rust-fx")
    parser.add_argument("--fq", default="tests/ci/env/bin/fq")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    reference, rust, fq = [str(Path(p).resolve()) for p in (args.seqkit, args.rust, args.fq)]
    version = subprocess.check_output([reference, "version"]).decode().strip()
    assert version == "seqkit v2.14.0", version
    fq_version = subprocess.check_output([fq, "--version"]).decode().strip()
    assert fq_version.startswith("fq v0.12.0"), fq_version
    counts = Counter()
    examples = []
    range_violations = 0
    copied_quality_observations = 0

    def run(binary, command, options=(), data=None, files=()):
        result = subprocess.run([binary, command, *options, *map(str, files)],
                                input=data, capture_output=True, timeout=30)
        assert result.returncode == 0, (binary, command, options, result.returncode, result.stderr)
        return result.stdout

    def check(category, actual, expected):
        if actual != expected:
            first = next((i for i, (a, b) in enumerate(zip(actual, expected)) if a != b), min(len(actual), len(expected)))
            raise AssertionError((category, f"first differing byte {first}",
                                  actual[max(0, first - 30):first + 60], expected[max(0, first - 30):first + 60]))
        counts[category] += 1

    # Algebraic check of the independent IUPAC complement: two complements
    # recover DNA, while RNA is equivalent after U->T normalization.
    assert rc(rc(DNA)) == DNA
    assert rc(b"UUG") == rc(b"TTG") == b"CAA"
    codons = [bytes(c) for c in itertools.product(DNA, repeat=3)]
    for table in TABLES:
        for frame in (1, 2, 3):
            records = [(str(i).encode(), c + b"AC"[:frame - 1]) for i, c in enumerate(codons)]
            for init in ((), ("-M",)):
                common = ("-T", str(table), *init, "-w", "0")
                actual = run(rust, "translate", (*common, "-f", str(-frame)), fasta(records))
                expected = run(reference, "translate", (*common, "-f", str(frame)),
                               fasta([(name, rc(seq)) for name, seq in records]))
                check("strand/table-frame-init", actual, expected)
    for sequence in (b"UUG", b"AUGUUGUAA", b"uugcaugac", b"CUNGGRAAY"):
        for frame in (1, 2, 3):
            actual = run(rust, "translate", ("-f", str(-frame)), fasta([(b"rna", sequence)]))
            expected = run(reference, "translate", ("-f", str(frame)), fasta([(b"rna", rc(sequence))]))
            check("strand/RNA", actual, expected)

    # Each canonical expansion of YTC independently translates to E on the
    # negative strand. A correct ambiguity translation therefore cannot be D.
    expanded = run(reference, "translate", (), b">CTC\nGAG\n>TTC\nGAA\n")
    assert [protein for _, protein in fasta_records(expanded)] == [b"E", b"E"]
    for name, options, data in (
        ("ambiguous-complement", ("-f", "-1"), b">r\nYTC\n"),
        ("RNA-complement", ("-f", "-1"), b">r\nUUG\n"),
        ("reverse-ORF-coordinate", ("-s", "-f", "-1"), b">r\nTTAGGGCAT\n"),
        ("right-trim", ("--trim",), b">r\nATGNNNCCC\n"),
        ("description-loss", ("-F",), b">r  description\nATG\n"),
    ):
        upstream = run(reference, "translate", options, data)
        corrected = run(rust, "translate", options, data)
        assert upstream != corrected, (name, upstream)
        examples.append({"defect": name, "command": ["translate", *options],
                         "input": data.decode(), "upstream": upstream.decode(),
                         "rust": corrected.decode()})

    # Build ORFs from positive translation of each independently oriented input,
    # then project a half-open nucleotide span back to 1-based inclusive input
    # coordinates. Compare the complete record set, not just valid-looking ends.
    sequences = [b"TTAGGGCAT", b"ATGTAATTGCCCNNNTAG", b"TAATAAATGTAA", b"CCCATGGGGTAA"]
    sequences += [(b"ATGCCCTAATTG" * 8)[:size] for size in range(3, 70)]
    for frame in (-3, -2, -1, 1, 2, 3):
        records = [(f"r{i}".encode(), seq) for i, seq in enumerate(sequences)]
        oriented = [(name, rc(seq) if frame < 0 else seq) for name, seq in records]
        proteins = fasta_records(run(reference, "translate", ("-f", str(abs(frame)), "-w", "0"), fasta(oriented)))
        expected = []
        for (name, seq), (_, protein) in zip(records, proteins):
            for match in re.finditer(rb"[^*]+", protein):
                offset = abs(frame) - 1
                lo, hi = offset + match.start() * 3, offset + match.end() * 3
                start, end = (len(seq) - hi + 1, len(seq) - lo) if frame < 0 else (lo + 1, hi)
                assert end - start + 1 == len(match.group()) * 3
                expected.append((name + f"_frame={frame}_begin={start}_end={end} ".encode(), match.group()))
        actual = run(rust, "translate", ("-s", "-f", str(frame), "-w", "0"), fasta(records))
        check("ORF/source-projection", actual, fasta(expected))

    trim_records = [(b"internal-X", b"ATGNNNCCC"), (b"internal-stop", b"ATGTAACCC"),
                    (b"tail", b"ATGNNNTAANNN"), (b"all-tail", b"TAANNN")]
    for table in TABLES:
        for clean in ((), ("--clean",)):
            options = ("-T", str(table), *clean)
            baseline = fasta_records(run(reference, "translate", options, fasta(trim_records)))
            expected = fasta([(name, seq.rstrip(b"X*")) for name, seq in baseline])
            actual = run(rust, "translate", (*options, "--trim"), fasta(trim_records))
            check("trim/right-end-only", actual, expected)
    for separator in (b" ", b"  ", b"\t", b"\t \t", b"     "):
        for mode in (("-F",), ("-s",), ("-e", "-F")):
            data = b">r" + separator + b"description\nATG\n"
            expected = run(reference, "translate", mode, b">r description\nATG\n")
            check("header/description-preserved", run(rust, "translate", mode, data), expected)

    with tempfile.TemporaryDirectory(prefix="seqkit-correctness-") as directory:
        root = Path(directory)
        qpath, fpath = root / "reads.fq", root / "reads.fa"
        for quality in (b"JKLM", b"!!!!"):
            for sequence in (b"TGCA", b"TGCAAC"):
                qrecords = [(b"q1", b"ACGT", quality), (b"q2", b"GCAA", quality), (b"q3", b"TGCT", quality)]
                frecords = [(b"f1", sequence, None), (b"f2", sequence, None), (b"f3", sequence, None)]
                qpath.write_bytes(render(qrecords, 0))
                fpath.write_bytes(render(frecords, 0))
                for region in ("1:-1", "2:2", "-2:-1", "-1:-1"):
                    for width in (0, 2, 60):
                        for inputs, records in (((qpath, fpath), (qrecords, frecords)),
                                                ((fpath, qpath), (frecords, qrecords))):
                            options = ("-r", region, "-w", str(width))
                            expected = b"".join(render(select(rs, region), width) for rs in records)
                            actual = run(rust, "range", options, files=inputs)
                            check("range/record-provenance", actual, expected)
                            separate = b"".join(run(rust, "range", options, files=[path]) for path in inputs)
                            check("range/per-file-composition", actual, separate)
                            upstream = run(reference, "range", options, files=inputs)
                            range_violations += upstream != expected
                            if inputs[0] == qpath and width == 0:
                                marker = b"@f1\n" + sequence + b"\n+\n" + quality + b"\n"
                                copied_quality_observations += marker in upstream
        # Repeat a minimal malformed-output example. Pool reuse is not promised:
        # empty qualities are also invalid for the six-base FASTA record.
        qpath.write_bytes(b"@q\nACGT\n+\nJKLM\n")
        fpath.write_bytes(b">f\nTGCAAC\n")
        valid = subprocess.run([fq, "lint", str(qpath)], capture_output=True, timeout=20)
        assert valid.returncode == 0, (valid.stdout, valid.stderr)
        counts["range/independent-fq-valid-control"] += 1
        for repeat in range(8):
            options = ("-r", "1:-1", "-w", "0")
            output = run(reference, "range", options, files=(qpath, fpath))
            invalid = root / "upstream.fastq"
            invalid.write_bytes(output)
            lint = subprocess.run([fq, "lint", str(invalid)], capture_output=True, timeout=20)
            lines = output.splitlines()
            assert len(lines) == 8 and lines[4:7] == [b"@f", b"TGCAAC", b"+"], output
            assert lines[7] in (b"JKLM", b""), output
            violation = b"[S005]" if lines[7] else b"[S004]"
            assert lint.returncode != 0 and violation in lint.stderr, (output, lint.stdout, lint.stderr)
            counts["range/independent-fq-rejection"] += 1
            expected = b"@q\nACGT\n+\nJKLM\n>f\nTGCAAC\n"
            corrected = run(rust, "range", options, files=(qpath, fpath))
            check("range/repeat-correctness", corrected, expected)
            if repeat == 0:
                examples.append({"defect": "quality-without-source", "command": ["range", *options],
                                 "inputs": [qpath.read_text(), fpath.read_text()],
                                 "upstream": output.decode(), "rust": corrected.decode(),
                                 "independent_validator": fq_version, "validator_exit": lint.returncode,
                                 "validator_stderr": lint.stderr.decode(errors="replace")})
    assert range_violations > 0, "the pinned upstream defect was not reproduced"
    summary = {"reference": version, "independent_validator": fq_version,
               "checks": dict(counts), "total": sum(counts.values()),
               "upstream_range_invariant_violations": range_violations,
               "observed_stale_quality_copies": copied_quality_observations,
               "examples": examples}
    print(f"PASS SeqKit corrected semantics: {summary['total']} checks")
    print(f"Upstream range: {range_violations} invariant violations; {copied_quality_observations} stale-quality copies observed")
    for example in examples:
        print(example["defect"], "upstream=", repr(example["upstream"]), "rust=", repr(example["rust"]))
    if args.report:
        args.report.write_text(json.dumps(summary, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
