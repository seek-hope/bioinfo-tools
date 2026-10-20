#!/usr/bin/env python3
"""W1-2 FASTX equivalence harness: pre-migration binary vs post-migration binary.

Why this exists on top of `tests/gate/m3.sh`
--------------------------------------------
`m3.sh` compares the current build against goldens recorded from the
**pre-W1-1** binaries. That is the right instrument for a refactor, but for W1-2
it has two blind spots:

1. it does not vary the pipeline chunk size, and the FASTX migration replaced the
   chunker;
2. it does not exercise every compression format on every path.

This harness closes both. It runs the *same* invocation against a reference
binary and a candidate binary and compares:

* the exit code (L0, always);
* decompressed output bytes for every output file (L1);
* stdout and stderr after a small, named set of normalisations (L2).

Usage
-----
    python3 tests/gate/w12_fastx_equivalence.py \\
        --reference .work/w1-2-baseline/bin/rust-cutadapt.pre-w12 \\
        --candidate target/release/rust-cutadapt \\
        --binary cutadapt \\
        --report /tmp/w12-report.json

Exit status: 0 when every case matches, 1 on a difference, 3 when a binary or a
required tool is missing (never silently skip).
"""

from __future__ import annotations

import argparse
import bz2
import gzip
import json
import lzma
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

# --------------------------------------------------------------------------- #
# case model
# --------------------------------------------------------------------------- #


@dataclass
class Case:
    """One invocation and the observables that must match."""

    name: str
    argv: list[str]
    guard: str
    # Output files to compare, as (label, path template).
    files: list[tuple[str, str]] = field(default_factory=list)
    # Extra environment for the child process.
    env: dict[str, str] = field(default_factory=dict)
    # Fixtures to stage in the working directory: (name, content bytes).
    fixtures: dict[str, bytes] = field(default_factory=dict)
    expect_exit: int | None = None


ADAPTER = "AGATCGGAAGAGC"


def fastq(records: list[tuple[str, str]], quality: str = "I") -> bytes:
    out = []
    for name, seq in records:
        out.append(f"@{name}\n{seq}\n+\n{quality * len(seq)}\n")
    return "".join(out).encode()


def gz(data: bytes) -> bytes:
    return gzip.compress(data)


def bz(data: bytes) -> bytes:
    return bz2.compress(data, 9)


def xz(data: bytes) -> bytes:
    return lzma.compress(data)


def zstd(data: bytes) -> bytes:
    """Compress with the `zstd` CLI when available; otherwise skip."""
    exe = shutil.which("zstd")
    if exe is None:
        return b""
    return subprocess.run(
        [exe, "-q", "-c", "-3"], input=data, capture_output=True, check=True
    ).stdout


def base_records() -> list[tuple[str, str]]:
    """Records covering full/internal/partial/mismatch/no adapter and short reads."""
    return [
        ("full", "GCTAAAGACAATTACATAACATACACGTCAGCACGAAACT" + ADAPTER),
        ("internal", "TGTTGGCCCAGTGTGAATCGCTTAAGGGTT" + ADAPTER + "AAGTAAGTGT"),
        ("partial", "GATGCATACGCCTTTACTTGCTGTGTCCACCCCATCGGACTGGCAAGATCG"),
        ("none", "TTTTTATTACACTCAGAAACAGAACTCGGGTAATTTTGACAGGTCACGCA"),
        ("mismatch", "GAGGCGCGCCCTCCTGAAGTGCGTGGACACTCGCTATGAAAGATCGTAAGAGC"),
        ("short", "ACGTA"),
    ]


# --------------------------------------------------------------------------- #
# normalisation
# --------------------------------------------------------------------------- #

TIMING = re.compile(r"^Finished in .*$", re.M)
CORES = re.compile(r"^(Processing .* on )\d+( cores? \.\.\.)$", re.M)
BANNER = re.compile(r"^This is \S+ [0-9.]+ with Python [0-9.]+$", re.M)
# clap renders the program name from argv[0]'s basename, so the reference binary
# (`...pre-w12`) and the candidate print different usage lines. Only the name is
# normalised; the rest of the usage text is compared byte-exactly.
PROGRAM_NAME = re.compile(r"\b(rust-cutadapt|rust-umitools)(\.pre-w12|\.before)?\b")
CMDLINE = re.compile(r"^Command line parameters:.*$", re.M)


def normalise(text: str) -> str:
    """Strip only the machine-dependent parts of cutadapt's report."""
    text = TIMING.sub("Finished in <TIME>", text)
    text = CORES.sub(r"\1<N>\2", text)
    text = BANNER.sub("This is <PROGRAM> <VERSION> with Python <PYVER>", text)
    text = CMDLINE.sub("Command line parameters: <CMD>", text)
    text = PROGRAM_NAME.sub(r"\1", text)
    return text


# --------------------------------------------------------------------------- #
# decompression: decompressed bytes are the contract (plan decision D4)
# --------------------------------------------------------------------------- #


def read_maybe_compressed(path: Path) -> tuple[bytes, str]:
    data = path.read_bytes()
    if data[:2] == b"\x1f\x8b":
        try:
            return gzip.decompress(data), "gzip"
        except OSError:
            return data, "gzip-broken"
    if data[:3] == b"BZh":
        try:
            return bz2.decompress(data), "bzip2"
        except OSError:
            return data, "bzip2-broken"
    if data[:6] == b"\xfd7zXZ\x00":
        try:
            return lzma.decompress(data), "xz"
        except OSError:
            return data, "xz-broken"
    if data[:4] == b"\x28\xb5\x2f\xfd":
        exe = shutil.which("zstd")
        if exe is None:
            return data, "zstd-unreadable"
        out = subprocess.run(
            [exe, "-d", "-q", "-c"], input=data, capture_output=True
        )
        return out.stdout, "zstd"
    return data, "plain"


# --------------------------------------------------------------------------- #
# running
# --------------------------------------------------------------------------- #


@dataclass
class Observation:
    exit_code: int
    stdout: str
    stderr: str
    files: dict[str, dict]


def run_case(case: Case, binary: Path, base: Path) -> Observation:
    work = base / case.name
    work.mkdir(parents=True, exist_ok=True)
    for name, content in case.fixtures.items():
        (work / name).write_bytes(content)

    env = dict(os.environ)
    # Knobs that would make output depend on the environment.
    for knob in ("RUST_CUTADAPT_CHUNK", "RUST_CUTADAPT_COMPRESSION_LEVEL"):
        env.pop(knob, None)
    env["LC_ALL"] = "C"
    env["TZ"] = "UTC"
    env["COLUMNS"] = "100"
    env.update(case.env)

    done = subprocess.run(
        [str(binary), *case.argv],
        cwd=work,
        env=env,
        capture_output=True,
        check=False,
    )

    files: dict[str, dict] = {}
    for label, template in case.files:
        path = work / template
        if not path.exists():
            files[label] = {"kind": "missing"}
            continue
        plain, kind = read_maybe_compressed(path)
        files[label] = {
            "kind": "decompressed",
            "container": kind,
            "bytes": len(plain),
            "sha256": __import__("hashlib").sha256(plain).hexdigest(),
        }

    return Observation(
        exit_code=done.returncode,
        stdout=normalise(done.stdout.decode("utf-8", "surrogateescape")),
        stderr=normalise(done.stderr.decode("utf-8", "surrogateescape")),
        files=files,
    )


def compare(case: Case, ref: Observation, cand: Observation) -> list[str]:
    problems: list[str] = []
    if ref.exit_code != cand.exit_code:
        problems.append(f"exit code {ref.exit_code} -> {cand.exit_code}")
    if ref.stdout != cand.stdout:
        problems.append(f"stdout differs: {first_diff(ref.stdout, cand.stdout)}")
    if ref.stderr != cand.stderr:
        problems.append(f"stderr differs: {first_diff(ref.stderr, cand.stderr)}")
    for label in sorted(set(ref.files) | set(cand.files)):
        a = ref.files.get(label, {"kind": "absent"})
        b = cand.files.get(label, {"kind": "absent"})
        if a.get("kind") == "missing" or b.get("kind") == "missing":
            if a.get("kind") != b.get("kind"):
                problems.append(f"{label}: presence changed {a.get('kind')} -> {b.get('kind')}")
            continue
        if a.get("sha256") != b.get("sha256"):
            problems.append(
                f"{label}: decompressed bytes differ "
                f"({a.get('bytes')} -> {b.get('bytes')} bytes)"
            )
    return problems


def first_diff(a: str, b: str) -> str:
    al, bl = a.splitlines(), b.splitlines()
    for i in range(max(len(al), len(bl))):
        x = al[i] if i < len(al) else "<missing>"
        y = bl[i] if i < len(bl) else "<missing>"
        if x != y:
            return f"line {i + 1}: {x!r} != {y!r}"
    return "trailing newline only"


# --------------------------------------------------------------------------- #
# the cutadapt matrix
# --------------------------------------------------------------------------- #


def cutadapt_cases() -> list[Case]:
    records = base_records()
    plain = fastq(records)
    r2 = fastq([(n, s[::-1]) for n, s in records])
    cases: list[Case] = []

    # --- every compression format, in and out --------------------------------
    inputs = {
        "plain": plain,
        "gz": gz(plain),
        "bz2": bz(plain),
        "xz": xz(plain),
    }
    for in_name, in_bytes in inputs.items():
        case = Case(
            f"in_{in_name}",
            ["--quiet", "-a", ADAPTER, "-m", "10", "-o", "out.fastq", f"in.{in_name}"],
            f"reads a {in_name}-encoded input",
            files=[("out", "out.fastq")],
            fixtures={f"in.{in_name}": in_bytes},
            expect_exit=0,
        )
        cases.append(case)

    for out_name, suffix in [
        ("plain", "fastq"),
        ("gz", "fastq.gz"),
        ("bz2", "fastq.bz2"),
        ("xz", "fastq.xz"),
    ]:
        cases.append(
            Case(
                f"out_{out_name}",
                ["--quiet", "-a", ADAPTER, "-m", "10", "-o", f"out.{suffix}", "in.fastq"],
                f"writes a {out_name}-encoded output",
                files=[("out", f"out.{suffix}")],
                fixtures={"in.fastq": plain},
                expect_exit=0,
            )
        )

    # --- -Z (fast levels) ----------------------------------------------------
    for suffix in ("fastq.gz", "fastq.bz2", "fastq.xz"):
        name = suffix.replace(".", "_")
        cases.append(
            Case(
                f"fast_{name}",
                ["--quiet", "-Z", "-a", ADAPTER, "-m", "10", "-o", f"out.{suffix}", "in.fastq"],
                f"-Z fast levels for {suffix}",
                files=[("out", f"out.{suffix}")],
                fixtures={"in.fastq": plain},
                expect_exit=0,
            )
        )

    # --- chunk-size variation (the migration replaced the chunker) -----------
    # Deliberately includes sizes below, at and far above one record, plus a
    # non-power-of-two, to exercise the record-boundary logic.
    big = fastq([(f"r{i}", "ACGTACGTACGTACGTACGT" + (ADAPTER if i % 3 == 0 else ""))
                 for i in range(4000)])
    for chunk in ("4096", "8192", "65536", "262144", "1000003"):
        cases.append(
            Case(
                f"chunk_{chunk}",
                ["--quiet", "-a", ADAPTER, "-m", "0", "-o", "out.fastq", "in.fastq"],
                f"record-aligned chunking at RUST_CUTADAPT_CHUNK={chunk}",
                files=[("out", "out.fastq")],
                fixtures={"in.fastq": big},
                env={"RUST_CUTADAPT_CHUNK": chunk},
                expect_exit=0,
            )
        )

    # --- worker-count determinism -------------------------------------------
    for threads in ("1", "2", "4", "8"):
        cases.append(
            Case(
                f"threads_{threads}",
                ["--quiet", "-j", threads, "-a", ADAPTER, "-m", "0", "-o", "out.fastq", "in.fastq"],
                f"output is independent of -j {threads}",
                files=[("out", "out.fastq")],
                fixtures={"in.fastq": big},
                expect_exit=0,
            )
        )

    # --- FASTQ shapes the parser must handle identically --------------------
    cases.append(
        Case(
            "crlf",
            ["--quiet", "-a", ADAPTER, "-m", "0", "-o", "out.fastq", "in.fastq"],
            "CRLF line endings are accepted and output is LF",
            files=[("out", "out.fastq")],
            fixtures={"in.fastq": plain.replace(b"\n", b"\r\n")},
            expect_exit=0,
        )
    )
    cases.append(
        Case(
            "empty",
            ["-a", ADAPTER, "-m", "10", "-o", "out.fastq", "in.fastq"],
            "empty input: exit 0 and 'No reads processed!'",
            files=[("out", "out.fastq")],
            fixtures={"in.fastq": b""},
            expect_exit=0,
        )
    )
    cases.append(
        Case(
            "no_trailing_newline",
            ["--quiet", "-a", ADAPTER, "-m", "0", "-o", "out.fastq", "in.fastq"],
            "a final record without a trailing newline is rejected identically",
            files=[("out", "out.fastq")],
            fixtures={"in.fastq": fastq(records).rstrip(b"\n")},
        )
    )
    cases.append(
        Case(
            "plus_line_with_name",
            ["--quiet", "-a", ADAPTER, "-m", "0", "-o", "out.fastq", "in.fastq"],
            "the separator line is echoed verbatim, not normalised to '+'",
            files=[("out", "out.fastq")],
            fixtures={
                "in.fastq": b"@r1 extra\nACGTACGTACGT\n+r1 extra\nIIIIIIIIIIII\n"
            },
            expect_exit=0,
        )
    )

    # --- malformed inputs: identical diagnostics ----------------------------
    malformed = {
        "bad_header": b"r1\nACGT\n+\nIIII\n",
        "bad_plus": b"@r1\nACGT\n-\nIIII\n",
        "length_mismatch": b"@r1\nACGTACGT\n+\nIII\n",
        "truncated": b"@r1\nACGT\n+\n",
        "non_ascii_qual": b"@r1\nACGT\n+\nII\xffI\n",
    }
    for name, data in malformed.items():
        cases.append(
            Case(
                f"malformed_{name}",
                ["-a", ADAPTER, "-m", "0", "-o", "out.fastq", "in.fastq"],
                f"malformed input ({name}) gives the same diagnostic and exit code",
                files=[("out", "out.fastq")],
                fixtures={"in.fastq": data},
            )
        )

    # --- FASTA output -------------------------------------------------------
    cases.append(
        Case(
            "fasta_out",
            ["--quiet", "--fasta", "-a", ADAPTER, "-m", "0", "-o", "out.fa", "in.fastq"],
            "--fasta relabels @ to > and drops qualities",
            files=[("out", "out.fa")],
            fixtures={"in.fastq": plain},
            expect_exit=0,
        )
    )

    # --- paired-end: both mates through the shared chunker ------------------
    for suffix in ("fastq", "fastq.gz", "fastq.xz"):
        cases.append(
            Case(
                f"paired_{suffix.replace('.', '_')}",
                ["--quiet", "-a", ADAPTER, "-A", "ACGT", "-m", "0",
                 "-o", f"o1.{suffix}", "-p", f"o2.{suffix}", "r1.fastq", "r2.fastq"],
                f"paired-end both mates through the shared chunker ({suffix})",
                files=[("o1", f"o1.{suffix}"), ("o2", f"o2.{suffix}")],
                fixtures={"r1.fastq": plain, "r2.fastq": r2},
                expect_exit=0,
            )
        )
    cases.append(
        Case(
            "paired_interleaved",
            ["--quiet", "-a", ADAPTER, "-A", "ACGT", "--interleaved", "-m", "0",
             "-o", "out.fastq", "in.fastq"],
            "interleaved paired-end",
            files=[("out", "out.fastq")],
            # cutadapt requires the two mates of an interleaved pair to carry the
            # SAME name (a `/1`-`/2` suffix pair is rejected: "Reads are
            # improperly paired"). Verified against the reference binary.
            fixtures={
                "in.fastq": b"".join(
                    fastq([records[i]]) + fastq([(records[i][0], records[i][1][::-1])])
                    for i in range(len(records))
                )
            },
            expect_exit=0,
        )
    )

    # --- auxiliary outputs also go through the shared writer ----------------
    cases.append(
        Case(
            "aux_outputs",
            ["--quiet", "-a", ADAPTER, "-m", "0",
             "--too-short-output", "short.fastq",
             "--untrimmed-output", "untrimmed.fastq",
             "--info-file", "info.txt",
             "-o", "out.fastq", "in.fastq"],
            "auxiliary outputs (too-short, untrimmed, info) are byte-identical",
            files=[("out", "out.fastq"), ("short", "short.fastq"),
                   ("untrimmed", "untrimmed.fastq"), ("info", "info.txt")],
            fixtures={"in.fastq": plain},
            expect_exit=0,
        )
    )

    return cases


# --------------------------------------------------------------------------- #
# the umitools matrix: the FASTQ paths W1-2 migrated
# --------------------------------------------------------------------------- #


def umitools_cases() -> list[Case]:
    """`extract` and `whitelist` are the two commands that read/write FASTQ.

    The BAM paths are untouched by W1-2, so they are not in this matrix; they
    remain covered by `tests/oracle/run_exact.sh` and `run_oracle.sh`.
    """
    records = base_records()
    single = fastq(records)
    cases: list[Case] = []

    # --- extract: plain and gzip, in and out --------------------------------
    for name, content, gz_arg, suffix in [
        ("plain", single, False, "fastq"),
        ("gzip", gz(single), False, "fastq.gz"),
    ]:
        cases.append(
            Case(
                f"extract_{name}",
                ["extract", "-I", f"in.{suffix}", "-S", f"out.{suffix}", "-p", "NNNN"]
                + (["--quiet"] if gz_arg else []),
                f"extract reads and writes {name} FASTQ byte-identically",
                files=[("out", f"out.{suffix}")],
                fixtures={f"in.{suffix}": content},
            )
        )

    # Streaming to stdout is a separate code path.
    cases.append(
        Case(
            "extract_stdout",
            ["extract", "-I", "in.fastq", "-S", "-", "-p", "NNNN", "--quiet"],
            "extract to stdout",
            fixtures={"in.fastq": single},
        )
    )

    # 3' pattern and a quality filter touch the record fields.
    cases.append(
        Case(
            "extract_3prime",
            ["extract", "-I", "in.fastq", "-S", "out.fastq", "-p", "NNNN", "--3prime", "--quiet"],
            "extract 3' pattern",
            files=[("out", "out.fastq")],
            fixtures={"in.fastq": single},
        )
    )
    cases.append(
        Case(
            "extract_quality_filter",
            ["extract", "-I", "in.fastq", "-S", "out.fastq", "-p", "NNNN",
             "--quality-filter-mask", "20", "--quiet"],
            "extract quality mask (rewrites UMI qualities in place)",
            files=[("out", "out.fastq")],
            # A low UMI base quality forces the masking branch to fire.
            fixtures={"in.fastq": b"@q1\nACGTACGTACGT\n+\n!!!!IIIIIIII\n"
                                 b"@q2\nTTTTACGTACGT\n+\nIIIIIIIIIIII\n"},
        )
    )

    # --- whitelist: reads FASTQ, writes a TSV -------------------------------
    cells = fastq(
        [
            ("c1", "AAAACCCCACGTACGT"),
            ("c2", "AAAACCCCACGTACGT"),
            ("c3", "AAAACCCCACGTACGT"),
            ("c4", "GGGGCCCCACGTACGT"),
            ("c5", "TTTTCCCCACGTACGT"),
        ]
    )
    for method in ("distance", "density"):
        cases.append(
            Case(
                f"whitelist_{method}",
                ["whitelist", "-I", "in.fastq", "-S", "out.tsv", "-p", "NNNNCCCC",
                 "--method", "reads", "--knee-method", method, "--quiet"],
                f"whitelist {method} knee reads FASTQ identically",
                files=[("out", "out.tsv")],
                fixtures={"in.fastq": cells},
            )
        )
    cases.append(
        Case(
            "whitelist_set_number",
            ["whitelist", "-I", "in.fastq", "-S", "out.tsv", "-p", "NNNNCCCC",
             "--set-cell-number", "2", "--quiet"],
            "whitelist with a fixed cell number",
            files=[("out", "out.tsv")],
            fixtures={"in.fastq": cells},
        )
    )

    # --- malformed FASTQ through each command -------------------------------
    for name, data in {
        "length_mismatch": b"@r1\nACGTACGT\n+\nIII\n",
        "bad_header": b"r1\nACGT\n+\nIIII\n",
        "truncated": b"@r1\nACGT\n+\n",
    }.items():
        cases.append(
            Case(
                f"umitools_malformed_{name}",
                ["extract", "-I", "in.fastq", "-S", "out.fastq", "-p", "NNNN"],
                f"malformed FASTQ ({name}) gives the same diagnostic and exit code",
                files=[("out", "out.fastq")],
                fixtures={"in.fastq": data},
            )
        )

    # --- empty input --------------------------------------------------------
    cases.append(
        Case(
            "umitools_empty",
            ["extract", "-I", "in.fastq", "-S", "out.fastq", "-p", "NNNN", "--quiet"],
            "empty input: same exit code and output",
            files=[("out", "out.fastq")],
            fixtures={"in.fastq": b""},
        )
    )

    return cases


# --------------------------------------------------------------------------- #
# main
# --------------------------------------------------------------------------- #


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, help="pre-migration binary")
    parser.add_argument("--candidate", required=True, help="post-migration binary")
    parser.add_argument("--binary", default="cutadapt", choices=["cutadapt", "umitools"])
    parser.add_argument("--report", help="write a JSON report here")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    reference = Path(args.reference).resolve()
    candidate = Path(args.candidate).resolve()
    missing = [str(p) for p in (reference, candidate) if not os.access(p, os.X_OK)]
    if missing:
        print("ERROR: binary not executable:", file=sys.stderr)
        for path in missing:
            print(f"  - {path}", file=sys.stderr)
        return 3

    cases = cutadapt_cases() if args.binary == "cutadapt" else umitools_cases()
    print(f"reference: {reference}")
    print(f"candidate: {candidate}")
    print(f"cases    : {len(cases)}\n")

    results = []
    failed = 0
    with tempfile.TemporaryDirectory(prefix="w12-equiv-") as tmp:
        base = Path(tmp)
        for case in cases:
            ref = run_case(case, reference, base / "ref")
            cand = run_case(case, candidate, base / "cand")
            problems = compare(case, ref, cand)
            if case.expect_exit is not None and ref.exit_code != case.expect_exit:
                problems.append(
                    f"reference exit {ref.exit_code} != modelled {case.expect_exit} "
                    f"(the reference binary itself disagrees with the model)"
                )
            status = "PASS" if not problems else "FAIL"
            if problems:
                failed += 1
            print(f"{status}  {case.name:26s} {case.guard}")
            for problem in problems:
                print(f"        {problem}")
            if args.verbose and not problems:
                print(f"        exit={ref.exit_code} files={sorted(ref.files)}")
            results.append(
                {
                    "name": case.name,
                    "guard": case.guard,
                    "argv": case.argv,
                    "reference_exit": ref.exit_code,
                    "candidate_exit": cand.exit_code,
                    "status": status,
                    "problems": problems,
                }
            )

    print(f"\n{len(cases) - failed}/{len(cases)} cases identical")
    if args.report:
        Path(args.report).write_text(
            json.dumps(
                {
                    "reference": str(reference),
                    "candidate": str(candidate),
                    "cases": results,
                    "total": len(cases),
                    "identical": len(cases) - failed,
                },
                indent=2,
            )
            + "\n"
        )
        print(f"report: {args.report}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
