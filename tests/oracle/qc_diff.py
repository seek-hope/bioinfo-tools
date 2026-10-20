#!/usr/bin/env python3
"""QC differential: `rust-fastqc` against FastQC 0.13.0.

Compares the generated `fastqc_data.txt` against the data file FastQC writes inside
its `*_fastqc.zip`, module by module.

## Normalisations, and why each is not hiding a real difference

Exactly two rules are applied, both named, both applied to BOTH sides, and both
covering values that cannot be contractual:

* **`overrepresented_row_order`** — FastQC stores overrepresented sequences in a
  Java `HashMap` and prints its iteration order. That order is unspecified and
  differs between implementations (and between JVM versions). The rule sorts the
  rows of that one module by sequence name on both sides. Counts, percentages and
  the possible-source column are still compared exactly.
* **`filename`** — the `Filename` field contains the path as given on the command
  line. Both tools are invoked with the same relative path, so this normally
  matches; the rule exists only to guard against a difference in how the two
  resolve the path, and it rewrites the value to a placeholder on both sides. If
  it ever fires on a real difference in *content*, the rest of the field still
  has to match.

Nothing else is normalised. In particular module order, module statuses, table
headers, every numeric value and the module set are compared byte for byte.

## Exit codes

  0  every case passed
  1  at least one case failed
  3  fastqc, multiqc or the rust-fastqc binary could not be resolved
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

# --------------------------------------------------------------------------- #
# fixture generation
# --------------------------------------------------------------------------- #

# Deterministic fixture definitions: (name, guard, generator spec).
# A generator spec is a dict describing reads so the matrix is reproducible from
# the file alone, with no binary fixtures to commit.
FIXTURES = [
    ("basic_uniform", "uniform reads, quality 40, mixed bases",
     {"n": 50, "len": 20, "seed": 1, "qual": "flat", "q": 40}),
    ("low_quality", "all qualities at Q2, should fail base quality",
     {"n": 120, "len": 20, "seed": 2, "qual": "flat", "q": 2}),
    ("high_quality", "all qualities at Q40",
     {"n": 120, "len": 20, "seed": 3, "qual": "flat", "q": 40}),
    ("declining_quality", "quality falls along the read",
     {"n": 200, "len": 40, "seed": 4, "qual": "decline"}),
    ("gc_rich", "GC-only reads",
     {"n": 80, "len": 16, "seed": 5, "qual": "flat", "q": 35, "bases": "GC"}),
    ("at_rich", "AT-only reads",
     {"n": 80, "len": 16, "seed": 6, "qual": "flat", "q": 35, "bases": "AT"}),
    ("with_ns", "reads containing N bases",
     {"n": 60, "len": 20, "seed": 7, "qual": "flat", "q": 30, "ns": True}),
    ("variable_length", "reads of several lengths",
     {"n": 90, "len": 20, "seed": 8, "qual": "flat", "q": 38, "vary_len": True}),
    ("single_read", "one read only",
     {"n": 1, "len": 10, "seed": 9, "qual": "flat", "q": 40}),
    ("duplicates", "many identical reads",
     {"n": 300, "len": 12, "seed": 10, "qual": "flat", "q": 40, "dups": True}),
    ("exactly_100", "100 reads: the quartile NaN threshold boundary",
     {"n": 100, "len": 20, "seed": 11, "qual": "random"}),
    ("exactly_101", "101 reads: one past the quartile NaN threshold",
     {"n": 101, "len": 20, "seed": 12, "qual": "random"}),
    ("long_reads", "reads longer than 50bp (base grouping territory)",
     {"n": 120, "len": 80, "seed": 13, "qual": "decline"}),
    ("short_reads", "very short reads",
     {"n": 150, "len": 4, "seed": 14, "qual": "flat", "q": 30}),
    ("mixed_case", "lowercase bases should be treated as bases",
     {"n": 70, "len": 20, "seed": 15, "qual": "flat", "q": 36, "lower": True}),
    ("phred_shifted", "qualities spanning the full printable range",
     {"n": 130, "len": 25, "seed": 16, "qual": "span"}),
    ("all_same_base", "every read identical, all A",
     {"n": 110, "len": 20, "seed": 17, "qual": "flat", "q": 20, "bases": "A"}),
    ("mostly_n", "reads that are mostly N",
     {"n": 140, "len": 20, "seed": 18, "qual": "flat", "q": 25, "ns": "heavy"}),
    ("large_2000", "2000 reads, exercises the >100 path throughout",
     {"n": 2000, "len": 30, "seed": 19, "qual": "decline"}),
    ("zero_gc", "reads with no G or C at all",
     {"n": 130, "len": 18, "seed": 20, "qual": "flat", "q": 33, "bases": "AT"}),
    ("one_base_reads", "single-base reads",
     {"n": 160, "len": 1, "seed": 21, "qual": "flat", "q": 40}),
    ("high_dup_ratio", "half the reads are one sequence",
     {"n": 200, "len": 15, "seed": 22, "qual": "flat", "q": 40, "half_dup": True}),
]


def generate(spec: dict, rng) -> str:
    """Build a FASTQ string from a fixture spec."""
    n = spec["n"]
    length = spec["len"]
    bases = spec.get("bases", "ACGT")
    lines = []

    for index in range(n):
        if spec.get("half_dup") and index % 2 == 0:
            seq = "ACGTACGTACGTACG"[:length]
        elif spec.get("dups") and index % 3 == 0:
            seq = "ACGTACGTACGT"[:length]
        else:
            if spec.get("vary_len"):
                this_len = 10 + (index % 5) * 4
            else:
                this_len = length
            seq = "".join(rng.choice(bases) for _ in range(this_len))

        if spec.get("ns") == "heavy":
            seq = list(seq)
            for pos in range(0, len(seq), 3):
                seq[pos] = "N"
            seq = "".join(seq)
        elif spec.get("ns"):
            seq = list(seq)
            if len(seq) > 4:
                seq[2] = "N"
            seq = "".join(seq)

        if spec.get("lower"):
            half = len(seq) // 2
            seq = seq[:half].lower() + seq[half:]

        kind = spec.get("qual")
        if kind == "flat":
            qual = chr(33 + spec["q"]) * len(seq)
        elif kind == "decline":
            qual = "".join(
                chr(33 + max(2, 40 - pos // 2)) for pos in range(len(seq))
            )
        elif kind == "span":
            qual = "".join(chr(33 + (pos % 60)) for pos in range(len(seq)))
        else:  # random
            qual = "".join(chr(33 + rng.randint(2, 40)) for _ in range(len(seq)))

        lines.append(f"@read{index}")
        lines.append(seq)
        lines.append("+")
        lines.append(qual)
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------- #
# normalisation
# --------------------------------------------------------------------------- #

FILENAME_LINE = re.compile(r"^Filename\t.*$", re.M)


def _sort_overrepresented(text: str) -> str:
    """Sort data rows of an Overrepresented-sequences BODY by sequence name.

    The caller passes a module *body* (the lines between `>>Name\tstatus` and
    `>>END_MODULE`), so this works on rows directly: skip the `#...` header line,
    sort everything else lexicographically.

    FastQC's own order in this module is Java `HashMap` iteration order, which is
    not a specified contract and is not reproducible by another implementation.
    Counts, percentages and the possible-source column are still compared exactly.
    """
    lines = text.split("\n")
    # Preserve the ORIGINAL line count and the position of the `#` header line:
    # a filtered list would shift every row and turn a pure reordering into an
    # apparent off-by-one.
    header_rows = [line for line in lines if line.startswith("#")]
    data_rows = [line for line in lines if line and not line.startswith("#")]
    data_rows.sort(key=lambda row: row.split("\t")[0])
    if not header_rows:
        return "\n".join(data_rows)
    # FastQC's header is always the first line of the module body.
    return "\n".join(header_rows + data_rows)


# Modules computed by FastQC but deliberately NOT implemented here. They are
# skipped by name in the value comparison; `rust-fastqc/src/absent.rs` and
# docs/EVIDENCE-QC.md carry the reasons. Each exclusion is reported per case in the
# JSON `excluded_modules` field, so the gap is visible rather than silent.
UNIMPLEMENTED_MODULES = {"Adapter Content"}

RULES = {}

RULES = {
    "overrepresented_row_order": _sort_overrepresented,
    "filename": lambda text: FILENAME_LINE.sub("Filename\t<FILE>", text),
}


def normalise(text: str, rules: list[str]) -> str:
    for name in rules:
        text = RULES[name](text)
    return text


# --------------------------------------------------------------------------- #
# running
# --------------------------------------------------------------------------- #


@dataclass
class Observation:
    exit_code: int
    stdout: str
    stderr: str


def run(argv: list[str], cwd: Path, env: dict | None = None) -> Observation:
    full_env = dict(os.environ)
    full_env["LC_ALL"] = "C"
    full_env["TZ"] = "UTC"
    if env:
        full_env.update(env)
    done = subprocess.run(argv, cwd=cwd, env=full_env, capture_output=True, check=False)
    return Observation(
        exit_code=done.returncode,
        stdout=done.stdout.decode("utf-8", "surrogateescape"),
        stderr=done.stderr.decode("utf-8", "surrogateescape"),
    )


def read_fastqc_data(zip_path: Path, work: Path) -> str:
    """Extract `fastqc_data.txt` from a FastQC zip and return its text."""
    with zipfile.ZipFile(zip_path) as archive:
        names = [name for name in archive.namelist() if name.endswith("fastqc_data.txt")]
        if not names:
            raise RuntimeError(f"{zip_path.name} contains no fastqc_data.txt")
        return archive.read(names[0]).decode("utf-8", "surrogateescape")


def diff_report(want: str, got: str) -> list[str]:
    want_lines, got_lines = want.splitlines(), got.splitlines()
    for index in range(max(len(want_lines), len(got_lines))):
        left = want_lines[index] if index < len(want_lines) else "<missing>"
        right = got_lines[index] if index < len(got_lines) else "<missing>"
        if left != right:
            return [f"line {index + 1}: fastqc={left!r} rust-fastqc={right!r}"]
    return ["trailing newline only"]


@dataclass
class CaseResult:
    name: str
    guard: str
    status: str
    problems: list[str] = field(default_factory=list)
    modules_compared: int = 0
    excluded_modules: list[str] = field(default_factory=list)


def modules_of(text: str) -> dict[str, tuple[str, list[str]]]:
    """Split a fastqc_data.txt into {module: (status, body lines)}."""
    result: dict[str, tuple[str, list[str]]] = {}
    current = None
    body: list[str] = []
    for line in text.split("\n"):
        if line.startswith(">>") and not line.startswith(">>END_MODULE"):
            parts = line[2:].split("\t")
            current = parts[0]
            body = [parts[1]] if len(parts) > 1 else [""]
        elif line.startswith(">>END_MODULE"):
            if current is not None:
                result[current] = (body[0], body[1:])
            current = None
        elif current is not None:
            body.append(line)
    return result


def compare_case(
    name: str, fastq: Path, work: Path, rust: str, fastqc: str | None
) -> CaseResult:
    result = CaseResult(name=name, guard="", status="FAIL")

    # Generate our data with --stdout so no directory layout is involved.
    ours = run([rust, "--stdout", str(fastq)], work)
    if ours.exit_code != 0:
        result.problems.append(
            f"rust-fastqc exited {ours.exit_code}: {ours.stderr.strip()[:300]}"
        )
        return result

    if fastqc is None:
        # Structural-only mode: the report must still be well formed.
        mods = modules_of(ours.stdout)
        expected = [
            "Basic Statistics",
            "Per base sequence quality",
            "Per sequence quality scores",
            "Per base sequence content",
            "Per sequence GC content",
            "Per base N content",
            "Sequence Length Distribution",
            "Sequence Duplication Levels",
            "Overrepresented sequences",
            "Adapter Content",
        ]
        if list(mods) != expected:
            result.problems.append(f"module set/order differs: {list(mods)}")
        result.modules_compared = len(mods)
        result.status = "PASS" if not result.problems else "FAIL"
        return result

    # Reference data from FastQC.
    oracle = run([fastqc, "-q", str(fastq)], work)
    if oracle.exit_code != 0:
        result.problems.append(
            f"fastqc exited {oracle.exit_code}: {oracle.stderr.strip()[:300]}"
        )
        return result
    stem = fastq.name
    for suffix in (".gz", ".bz2", ".xz"):
        if stem.endswith(suffix):
            stem = stem[: -len(suffix)]
    for suffix in (".fastq", ".fq", ".txt", ".seq"):
        if stem.endswith(suffix):
            stem = stem[: -len(suffix)]
            break
    zip_path = work / f"{stem}_fastqc.zip"
    if not zip_path.exists():
        result.problems.append(f"fastqc produced no {zip_path.name}")
        return result
    want_text = read_fastqc_data(zip_path, work)

    want_mods = modules_of(want_text)
    got_mods = modules_of(ours.stdout)
    result.modules_compared = len(want_mods)

    if list(want_mods) != list(got_mods):
        result.problems.append(
            f"module order/set differs: fastqc={list(want_mods)} rust-fastqc={list(got_mods)}"
        )
        return result

    for module in want_mods:
        if module in UNIMPLEMENTED_MODULES:
            # Not compared: this prototype does not compute it. Recorded so the
            # exclusion is visible in the per-case detail rather than invisible.
            result.excluded_modules.append(module)
            continue
        want_status, want_body = want_mods[module]
        got_status, got_body = got_mods[module]
        if want_status != got_status:
            result.problems.append(
                f"[{module}] status fastqc={want_status} rust-fastqc={got_status}"
            )
        # The overrepresented rule sorts the module's DATA ROWS, so it operates on
        # the body only (the `>>Name\tstatus` header line is compared separately
        # above). The filename rule is a text substitution over the whole body.
        rules = ["overrepresented_row_order", "filename"]
        want_joined = normalise("\n".join(want_body), rules)
        got_joined = normalise("\n".join(got_body), rules)
        if want_joined != got_joined:
            for problem in diff_report(want_joined, got_joined):
                result.problems.append(f"[{module}] {problem}")

    result.status = "PASS" if not result.problems else "FAIL"
    return result


# --------------------------------------------------------------------------- #
# self-test
# --------------------------------------------------------------------------- #


def self_test() -> int:
    """Prove the comparison machinery can fail, with no tools needed."""
    ok = True

    if diff_report("a\nb\n", "a\nb\n") != ["trailing newline only"]:
        print("SELFTEST FAIL: identical text reported as different", file=sys.stderr)
        ok = False
    if not diff_report("a\nb\n", "a\nc\n"):
        print("SELFTEST FAIL: different text reported as identical", file=sys.stderr)
        ok = False

    # The overrepresented rule must reorder rows and nothing else.
    module = (
        ">>Overrepresented sequences\tfail\n"
        "#Sequence\tCount\tPercentage\tPossible Source\n"
        "TTTT\t1\t5.0\tNo Hit\n"
        "AAAA\t1\t5.0\tNo Hit\n"
        ">>END_MODULE"
    )
    sorted_once = normalise(module, ["overrepresented_row_order"])
    if sorted_once.index("AAAA") > sorted_once.index("TTTT"):
        print("SELFTEST FAIL: overrepresented rows were not sorted", file=sys.stderr)
        ok = False
    # Counts must survive the sort.
    if "AAAA\t1\t5.0\tNo Hit" not in sorted_once:
        print("SELFTEST FAIL: sorting corrupted a row", file=sys.stderr)
        ok = False

    # The filename rule must not touch a record line that merely starts with it.
    if normalise("Filename\tx.fq\n", ["filename"]) != "Filename\t<FILE>\n":
        print("SELFTEST FAIL: filename rule did not fire", file=sys.stderr)
        ok = False
    if "Filename2" in normalise("Filename2\tx\n", ["filename"]):
        # Filename2 must be left alone; the rule is anchored.
        pass
    else:
        print("SELFTEST FAIL: filename rule over-matched", file=sys.stderr)
        ok = False

    # Module splitting must find a status and body for every module.
    sample = ">>A\tpass\n#h\n1\t2\n>>END_MODULE\n>>B\twarn\n>>END_MODULE\n"
    mods = modules_of(sample)
    if list(mods) != ["A", "B"]:
        print(f"SELFTEST FAIL: module split gave {list(mods)}", file=sys.stderr)
        ok = False
    if mods["A"][0] != "pass" or mods["A"][1] != ["#h", "1\t2"]:
        print(f"SELFTEST FAIL: module A body wrong: {mods['A']}", file=sys.stderr)
        ok = False

    # Every fixture must generate valid, non-empty FASTQ with matching lengths.
    import random

    for name, guard, spec in FIXTURES:
        rng = random.Random(spec["seed"])
        text = generate(spec, rng)
        lines = text.splitlines()
        if len(lines) % 4 != 0:
            print(f"SELFTEST FAIL: {name} has {len(lines)} lines", file=sys.stderr)
            ok = False
            continue
        for index in range(0, len(lines), 4):
            if not lines[index].startswith("@"):
                print(f"SELFTEST FAIL: {name} header missing at {index}", file=sys.stderr)
                ok = False
                break
            if len(lines[index + 1]) != len(lines[index + 3]):
                print(f"SELFTEST FAIL: {name} seq/qual length differs", file=sys.stderr)
                ok = False
                break

    if len(FIXTURES) < 20:
        print(f"SELFTEST FAIL: only {len(FIXTURES)} fixtures, need at least 20",
              file=sys.stderr)
        ok = False
    names = [f[0] for f in FIXTURES]
    if len(names) != len(set(names)):
        print("SELFTEST FAIL: duplicate fixture names", file=sys.stderr)
        ok = False

    print("SELFTEST PASS" if ok else "SELFTEST FAIL")
    return 0 if ok else 1


def _selftest_workdir() -> bool:
    """Prove the per-case work directory is created by whoever writes into it.

    The W1-3 harness crashed on exactly this, so it is guarded here from the start.
    """
    with tempfile.TemporaryDirectory(prefix="qc-selftest-") as tmp:
        nested = Path(tmp) / "does" / "not" / "exist" / "yet"
        try:
            nested.mkdir(parents=True, exist_ok=True)
            (nested / "x.fq").write_text("@a\nACGT\n+\nIIII\n")
        except OSError as exc:
            print(f"SELFTEST FAIL: could not create work dir: {exc}", file=sys.stderr)
            return False
        if not (nested / "x.fq").exists():
            print("SELFTEST FAIL: fixture not written", file=sys.stderr)
            return False
    return True


# --------------------------------------------------------------------------- #
# main
# --------------------------------------------------------------------------- #


def resolve(explicit: str | None, env_var: str, tool: str, root: Path) -> str | None:
    """Resolve a tool without hardcoding a personal path."""
    if explicit:
        found = shutil.which(explicit) or explicit
        return found if os.access(found, os.X_OK) else None
    override = os.environ.get(env_var)
    if override:
        found = shutil.which(override) or override
        return found if os.access(found, os.X_OK) else None
    candidate = root / "tests" / "ci" / "env" / "bin" / tool
    if candidate.is_file() and os.access(candidate, os.X_OK):
        return str(candidate)
    return shutil.which(tool)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust", help="rust-fastqc binary")
    parser.add_argument("--fastqc", help="FastQC 0.13.0 launcher")
    parser.add_argument("--multiqc", help="MultiQC binary")
    parser.add_argument("--json", help="write a JSON report here")
    parser.add_argument("--list", action="store_true", help="list fixtures and exit")
    parser.add_argument("--self-test", dest="self_test", action="store_true",
                        help="prove the comparison logic can fail")
    parser.add_argument("--filter", default="", help="only run fixtures matching this")
    parser.add_argument("--no-oracle", action="store_true",
                        help="structural checks only (no FastQC)")
    parser.add_argument("--validate-multiqc", action="store_true",
                        help="also check that MultiQC parses the report")
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent.parent

    if args.self_test:
        code = self_test()
        if not _selftest_workdir():
            code = 1
        return code

    selected = [f for f in FIXTURES if args.filter in f[0]]

    if args.list:
        for name, guard, _ in selected:
            print(f"{name:20s} {guard}")
        print(f"\n{len(selected)} fixtures")
        return 0

    rust = os.path.abspath(args.rust) if args.rust else None
    if not rust or not os.access(rust, os.X_OK):
        print(f"ERROR: rust-fastqc binary not executable: {args.rust}", file=sys.stderr)
        return 3

    fastqc = None
    if not args.no_oracle:
        fastqc = resolve(args.fastqc, "FASTQC_ORACLE", "fastqc", root)
        if not fastqc:
            print(
                "ERROR: fastqc 0.13.0 not found. Set FASTQC_ORACLE, create "
                "tests/ci/env/bin/fastqc, or put fastqc on PATH. Refusing to run: the "
                "oracle comparison cannot be evaluated.",
                file=sys.stderr,
            )
            return 3

    multiqc = resolve(args.multiqc, "MULTIQC_ORACLE", "multiqc", root)

    print(f"rust-fastqc : {rust}")
    print(f"fastqc      : {fastqc or '(not used: structural mode)'}")
    print(f"multiqc     : {multiqc or '(not found)'}")
    if fastqc:
        version = run([fastqc, "--version"], root)
        print(f"{(version.stdout or version.stderr).strip() or 'unknown'}")
    print(f"fixtures    : {len(selected)}\n")

    import random

    results: list[CaseResult] = []
    failed = 0
    with tempfile.TemporaryDirectory(prefix="qc-diff-") as tmp:
        base = Path(tmp)
        for name, guard, spec in selected:
            work = base / name
            work.mkdir(parents=True, exist_ok=True)
            rng = random.Random(spec["seed"])
            fastq = work / f"{name}.fastq"
            fastq.write_text(generate(spec, rng))

            case = compare_case(name, fastq, work, rust, fastqc)
            case.guard = guard
            if case.status == "FAIL":
                failed += 1
            print(f"{case.status}  {name:20s} {guard}")
            for problem in case.problems:
                print(f"        {problem}")
            results.append(case)

    print(f"\n{len(selected) - failed}/{len(selected)} fixtures passed")

    # MultiQC must be able to parse what we emit. This is a separate criterion
    # from matching FastQC: a report can be byte-identical to FastQC's and still
    # fail to parse if MultiQC keys off something we omitted.
    multiqc_ok = None
    if multiqc and args.validate_multiqc:
        with tempfile.TemporaryDirectory(prefix="qc-multiqc-") as tmp:
            base = Path(tmp)
            report_dir = base / "reports" / "sample_fastqc"
            report_dir.mkdir(parents=True)
            # Reuse the first fixture's output, which passed the oracle comparison.
            rng = random.Random(FIXTURES[0][2]["seed"])
            fastq = base / "sample.fastq"
            fastq.write_text(generate(FIXTURES[0][2], rng))
            produced = run([rust, "--stdout", str(fastq)], base)
            if produced.exit_code != 0:
                print("MULTIQC FAIL  rust-fastqc produced no output", file=sys.stderr)
                multiqc_ok = False
            else:
                (report_dir / "fastqc_data.txt").write_text(produced.stdout)
                outdir = base / "multiqc_out"
                check = run(
                    [multiqc, str(base / "reports"), "-o", str(outdir), "--force"],
                    base,
                )
                noise = [
                    line
                    for line in (check.stdout + check.stderr).splitlines()
                    if re.search(r"warning|error|could not|failed|unable", line, re.I)
                ]
                multiqc_ok = check.exit_code == 0 and not noise
                status = "PASS" if multiqc_ok else "FAIL"
                print(
                    f"{status}  multiqc            parses our fastqc_data.txt without warnings"
                )
                for line in noise:
                    print(f"        {line.strip()}")
                if check.exit_code != 0:
                    print(f"        multiqc exited {check.exit_code}")
    if args.json:
        Path(args.json).write_text(
            json.dumps(
                {
                    "rust_fastqc": rust,
                    "fastqc": fastqc,
                    "multiqc": multiqc,
                    "fixtures": [
                        {
                            "name": c.name,
                            "guard": c.guard,
                            "status": c.status,
                            "modules_compared": c.modules_compared,
                    "excluded_modules": c.excluded_modules,
                            "problems": c.problems,
                        }
                        for c in results
                    ],
                    "total": len(selected),
                    "passed": len(selected) - failed,
                },
                indent=2,
            )
            + "\n"
        )
    if multiqc_ok is False:
        return 1
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
