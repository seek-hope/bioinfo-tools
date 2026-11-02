#!/usr/bin/env python3
"""W1-4 differential: `rust-bam` against `samtools` 1.24.

Runs a matrix of fixtures through both tools and compares:

  * SAM text from `view`                — byte-identical
  * BAM records re-read via samtools    — byte-identical after `samtools view`
  * `flagstat`                          — byte-identical
  * `idxstats`                          — byte-identical
  * `quickcheck`                        — same exit code
  * `sort`                              — byte-identical record order
  * `index`                             — byte-identical BAI, and samtools can
                                          use our index for region queries

## Why @PG lines are normalised away

Every `samtools` command that writes a header **appends its own `@PG` line**
recording its version and command line:

    @PG  ID:samtools.1  PN:samtools  PP:samtools  VN:1.24  CL:samtools view -h in.bam

That line names the tool that produced the file, so it can never match between two
different implementations, and it is provenance rather than alignment content. A
`pg_lines` rule drops `@PG` lines from **both** sides before comparison. Nothing
else about the header is normalised: `@HD`, `@SQ`, `@RG` and any input `@PG` lines
are still compared byte for byte.

## Exit codes

  0  every case passed
  1  at least one case failed
  3  an oracle (samtools) or the rust-bam binary could not be resolved
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

# --------------------------------------------------------------------------- #
# fixtures
# --------------------------------------------------------------------------- #

# A small but structurally broad BAM source:
#   * two references, so idxstats has two rows plus the `*` row
#   * a proper pair on the same reference
#   * a proper pair spanning two references (feeds "mate mapped to different chr")
#   * a reverse-strand record
#   * a secondary and a supplementary alignment
#   * a duplicate
#   * a singleton (mate unmapped)
#   * a fully unmapped record with no coordinate
BASIC_SAM = """\
@HD\tVN:1.6\tSO:coordinate
@SQ\tSN:chr1\tLN:1000
@SQ\tSN:chr2\tLN:500
@RG\tID:rg1\tSM:sample1\tPL:ILLUMINA
@PG\tID:input\tPN:fixture\tVN:0.1
pair1\t99\tchr1\t100\t60\t10M\t=\t300\t210\tACGTACGTAC\tIIIIIIIIII\tNM:i:0
secondary\t256\tchr1\t150\t5\t5M\t*\t0\t0\tACGTA\tIIIII
supp\t2048\tchr1\t160\t5\t5M\t*\t0\t0\tACGTA\tIIIII
pair1\t147\tchr1\t300\t60\t10M\t=\t100\t-210\tACGTACGTAC\tIIIIIIIIII\tNM:i:1
cross\t1\tchr1\t500\t25\t5M\tchr2\t100\t0\tACGTA\tIIIII
reverse\t16\tchr1\t700\t40\t5M\t*\t0\t0\tACGTA\tIIIII
dup\t1024\tchr1\t800\t30\t5M\t*\t0\t0\tACGTA\tIIIII
singleton\t73\tchr1\t900\t20\t5M\t*\t0\t0\tACGTA\tIIIII
cross\t65\tchr2\t100\t25\t5M\tchr1\t500\t0\tACGTA\tIIIII
unmapped\t4\t*\t0\t0\t*\t*\t0\t0\tACGTA\tIIIII
"""

# The same records with no header, as a bare SAM record stream.
NO_HEADER_SAM = "".join(
    line
    for line in BASIC_SAM.splitlines(keepends=True)
    if not line.startswith("@")
)

# A deliberately unsorted file, for `sort` and the `index` precondition.
UNSORTED_SAM = """\
@HD\tVN:1.6\tSO:unsorted
@SQ\tSN:chr1\tLN:1000
@SQ\tSN:chr2\tLN:500
r5\t0\tchr2\t300\t60\t5M\t*\t0\t0\tACGTA\tIIIII
r1\t0\tchr1\t500\t60\t5M\t*\t0\t0\tACGTA\tIIIII
r9\t4\t*\t0\t0\t*\t*\t0\t0\tACGTA\tIIIII
r2\t0\tchr1\t100\t60\t5M\t*\t0\t0\tACGTA\tIIIII
r3\t16\tchr1\t100\t60\t5M\t*\t0\t0\tACGTA\tIIIII
r4\t0\tchr1\t100\t60\t5M\t*\t0\t0\tACGTA\tIIIII
"""

# Records that share a start position, to exercise the strand tie-break.
TIE_SAM = """\
@HD\tVN:1.6\tSO:coordinate
@SQ\tSN:chr1\tLN:1000
fwd1\t0\tchr1\t50\t60\t5M\t*\t0\t0\tACGTA\tIIIII
rev1\t16\tchr1\t50\t60\t5M\t*\t0\t0\tACGTA\tIIIII
fwd2\t0\tchr1\t50\t60\t5M\t*\t0\t0\tACGTA\tIIIII
rev2\t16\tchr1\t50\t60\t5M\t*\t0\t0\tACGTA\tIIIII
"""

# A single-reference file with no reads, to check empty-input handling.
EMPTY_SAM = """\
@HD\tVN:1.6\tSO:coordinate
@SQ\tSN:chr1\tLN:1000
@SQ\tSN:chr2\tLN:500
"""


@dataclass
class Case:
    """One differential case."""

    name: str
    category: str
    guard: str
    # How to build the fixture: (filename, content) pairs.
    fixtures: dict[str, str] = field(default_factory=dict)
    # Which comparison to run.
    compare: str = "view"
    # Extra argv for rust-bam after the subcommand.
    rust_argv: list[str] = field(default_factory=list)
    # Normalisation rules to apply to both sides.
    normalise: list[str] = field(default_factory=list)
    # Whether the BAM fixture must be built by samtools first.
    needs_bam: bool = True
    expect_exit: int | None = None


def cases() -> list[Case]:
    basic = {"basic.sam": BASIC_SAM}
    unsorted = {"unsorted.sam": UNSORTED_SAM}
    tie = {"tie.sam": TIE_SAM}
    empty = {"empty.sam": EMPTY_SAM}
    no_header = {"noheader.sam": NO_HEADER_SAM}

    out: list[Case] = []

    # --- view -------------------------------------------------------------
    out.append(Case("view_basic", "view", "SAM text exactly matches samtools view",
                    basic, "view", ["view", "basic.bam"]))
    out.append(Case("view_header", "view", "-h includes the header",
                    basic, "view", ["view", "-h", "basic.bam"], normalise=["pg_lines"]))
    out.append(Case("view_header_only", "view", "-H prints only the header",
                    basic, "view", ["view", "-H", "basic.bam"], normalise=["pg_lines"]))
    out.append(Case("view_count", "view", "-c prints the record count",
                    basic, "view", ["view", "-c", "basic.bam"]))
    out.append(Case("view_empty", "view", "a header with no records",
                    empty, "view", ["view", "empty.bam"], normalise=["pg_lines"]))
    out.append(Case("view_sample_text", "view", "SAM input is accepted as well as BAM",
                    basic, "view", ["view", "basic.sam"], needs_bam=False))
    # A headerless SAM cannot be turned into BAM (samtools needs @SQ to resolve
    # reference ids and fails with "no SQ lines present in the header"), so this
    # case exercises SAM input directly and needs no BAM fixture.
    out.append(Case("view_no_header_input", "view", "a headerless SAM record stream",
                    no_header, "view", ["view", "noheader.sam"], needs_bam=False))
    out.append(Case("view_bam_roundtrip", "view", "-b output re-read by samtools matches",
                    basic, "bam_roundtrip", ["view", "-b", "basic.bam"]))

    # --- flagstat ---------------------------------------------------------
    out.append(Case("flagstat_basic", "flagstat", "all 16 counters match",
                    basic, "flagstat", ["flagstat", "basic.bam"]))
    out.append(Case("flagstat_empty", "flagstat", "N/A percentages on an empty file",
                    empty, "flagstat", ["flagstat", "empty.bam"]))

    # --- idxstats ---------------------------------------------------------
    out.append(Case("idxstats_basic", "idxstats", "per-reference counts and the * row",
                    basic, "idxstats", ["idxstats", "basic.bam"]))
    out.append(Case("idxstats_empty", "idxstats", "zeros with no records",
                    empty, "idxstats", ["idxstats", "empty.bam"]))

    # --- quickcheck -------------------------------------------------------
    out.append(Case("quickcheck_valid", "quickcheck", "a valid BAM passes",
                    basic, "quickcheck", ["quickcheck", "basic.bam"]))
    out.append(Case("quickcheck_empty_valid", "quickcheck", "a header-only BAM passes",
                    empty, "quickcheck", ["quickcheck", "empty.bam"]))

    # --- sort -------------------------------------------------------------
    out.append(Case("sort_unsorted", "sort", "coordinates ordered as samtools does",
                    unsorted, "sort", ["sort", "unsorted.bam"]))
    out.append(Case("sort_ties", "sort", "same-start records keep forward before reverse",
                    tie, "sort", ["sort", "tie.bam"]))
    out.append(Case("sort_basic_sorted", "sort", "an already-sorted file is unchanged",
                    basic, "sort", ["sort", "basic.bam"]))
    out.append(Case("sort_empty", "sort", "an empty file sorts to an empty file",
                    empty, "sort", ["sort", "empty.bam"]))

    # --- index ------------------------------------------------------------
    out.append(Case("index_sorted", "index", "BAI is byte-identical to samtools'",
                    basic, "index", ["index", "basic.bam"]))
    out.append(Case("index_rejects_unsorted", "index",
                    "an unsorted file is refused with an input-precondition failure",
                    unsorted, "index_unsorted", ["index", "unsorted.bam"],
                    expect_exit=3))

    return out


# --------------------------------------------------------------------------- #
# normalisation
# --------------------------------------------------------------------------- #

# Consumes the trailing newline too, so dropping a @PG line does not leave a
# blank line behind (which would itself be a spurious difference).
PG_LINE = re.compile(r"^@PG\t[^\n]*\n?", re.M)

RULES = {
    # `samtools` appends its own @PG provenance line to any header it writes.
    # That line names the tool and its command line, so it cannot match between
    # two implementations; it is provenance, not alignment content. Dropping it
    # from BOTH sides is the only normalisation applied, and every other header
    # line is still compared byte for byte.
    "pg_lines": [(PG_LINE, "")],
}


def normalise(text: str, rules: list[str]) -> str:
    for name in rules:
        for pattern, replacement in RULES[name]:
            text = pattern.sub(replacement, text)
    return text


# --------------------------------------------------------------------------- #
# running
# --------------------------------------------------------------------------- #


@dataclass
class Observation:
    exit_code: int
    stdout: str
    stderr: str


def run(argv: list[str], cwd: Path) -> Observation:
    env = dict(os.environ)
    env["LC_ALL"] = "C"
    env["TZ"] = "UTC"
    done = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, check=False)
    return Observation(
        exit_code=done.returncode,
        stdout=done.stdout.decode("utf-8", "surrogateescape"),
        stderr=done.stderr.decode("utf-8", "surrogateescape"),
    )


def build_fixtures(case: Case, work: Path, samtools: str) -> None:
    """Write SAM fixtures and, where needed, convert them to BAM."""
    work.mkdir(parents=True, exist_ok=True)
    for name, content in case.fixtures.items():
        path = work / name
        path.write_text(content)
        if case.needs_bam and name.endswith(".sam"):
            bam = work / (path.stem + ".bam")
            done = subprocess.run(
                [samtools, "view", "-b", "-o", str(bam), str(path)],
                cwd=work,
                capture_output=True,
                check=False,
            )
            if done.returncode != 0:
                raise RuntimeError(
                    f"fixture {name} is not valid SAM: "
                    f"{done.stderr.decode('utf-8', 'replace')}"
                )


def diff_report(want: str, got: str) -> list[str]:
    want_lines, got_lines = want.splitlines(), got.splitlines()
    for index in range(max(len(want_lines), len(got_lines))):
        left = want_lines[index] if index < len(want_lines) else "<missing>"
        right = got_lines[index] if index < len(got_lines) else "<missing>"
        if left != right:
            return [f"line {index + 1}: samtools={left!r} rust-bam={right!r}"]
    return ["trailing newline only"]


def compare_case(
    case: Case, work: Path, samtools: str, rust: str
) -> tuple[list[str], dict]:
    """Run one case and return (problems, detail)."""
    problems: list[str] = []
    detail: dict = {}

    if case.compare == "quickcheck":
        want = run([samtools, "quickcheck", *case.rust_argv[1:]], work)
        got = run([rust, *case.rust_argv], work)
        detail["samtools_exit"] = want.exit_code
        detail["rust_exit"] = got.exit_code
        if want.exit_code != got.exit_code:
            problems.append(
                f"quickcheck exit {want.exit_code} -> {got.exit_code}"
            )
        return problems, detail

    if case.compare == "bam_roundtrip":
        # `view -b`: write BAM with rust-bam, then read it back with samtools and
        # compare the records against samtools' own view of the source. This
        # proves the BGZF/BAM container is readable by htslib, not just by us.
        out_bam = work / "rust.bam"
        got = run([rust, *case.rust_argv[:-1], "-o", str(out_bam), case.rust_argv[-1]], work)
        detail["rust_exit"] = got.exit_code
        if got.exit_code != 0:
            problems.append(f"rust-bam exited {got.exit_code}: {got.stderr.strip()}")
            return problems, detail

        reread = run([samtools, "view", str(out_bam)], work)
        source = run([samtools, "view", case.rust_argv[-1]], work)
        if reread.exit_code != 0:
            problems.append(
                f"samtools could not read our BAM: {reread.stderr.strip()}"
            )
            return problems, detail
        detail["samtools_can_read_our_bam"] = True
        if reread.stdout != source.stdout:
            problems.extend(["BAM records differ"] + diff_report(source.stdout, reread.stdout))

        # And htslib's own structural check must accept it.
        check = run([samtools, "quickcheck", str(out_bam)], work)
        detail["quickcheck_on_our_bam"] = check.exit_code
        if check.exit_code != 0:
            problems.append(
                f"samtools quickcheck rejected our BAM (exit {check.exit_code}): "
                f"{check.stderr.strip()}"
            )
        return problems, detail

    if case.compare == "index_unsorted":
        got = run([rust, *case.rust_argv], work)
        detail["rust_exit"] = got.exit_code
        if case.expect_exit is not None and got.exit_code != case.expect_exit:
            problems.append(
                f"exit {got.exit_code} != modelled {case.expect_exit} "
                f"(stderr: {got.stderr.strip()!r})"
            )
        return problems, detail

    if case.compare == "index":
        # Build both indexes and compare the bytes. samtools writes `<bam>.bai`
        # next to the file; rust-bam does the same, so copy first.
        ours_dir = work / "ours"
        theirs_dir = work / "theirs"
        ours_dir.mkdir(exist_ok=True)
        theirs_dir.mkdir(exist_ok=True)
        bam = work / case.rust_argv[1]
        for target in (ours_dir, theirs_dir):
            shutil.copy(bam, target / bam.name)

        got = run([rust, "index", bam.name], ours_dir)
        want = run([samtools, "index", bam.name], theirs_dir)
        detail["rust_exit"] = got.exit_code
        detail["samtools_exit"] = want.exit_code
        if got.exit_code != 0:
            problems.append(f"rust-bam index exited {got.exit_code}: {got.stderr.strip()}")
            return problems, detail
        if want.exit_code != 0:
            problems.append(f"samtools index exited {want.exit_code}")
            return problems, detail

        ours = (ours_dir / (bam.name + ".bai")).read_bytes()
        theirs = (theirs_dir / (bam.name + ".bai")).read_bytes()
        detail["bai_bytes"] = len(ours)
        if ours != theirs:
            problems.append(
                f"BAI differs: rust-bam {len(ours)} bytes, samtools {len(theirs)} bytes"
            )

        # Semantic check: htslib must be able to use OUR index to answer a
        # region query, which is the only thing an index is for.
        region = run([samtools, "view", "-c", bam.name, "chr1"], ours_dir)
        detail["region_query_exit"] = region.exit_code
        detail["region_query_count"] = region.stdout.strip()
        if region.exit_code != 0:
            problems.append(
                f"samtools could not use our BAI for a region query: {region.stderr.strip()}"
            )
        return problems, detail

    if case.compare == "sort":
        # `sort` writes BAM to stdout by default. Comparing those bytes as text is
        # meaningless: BGZF block boundaries are an encoding choice, so two
        # correct implementations legitimately produce different compressed bytes.
        #
        # What IS contractual is the decoded content: the record order and the
        # header. So both sides write BAM to a file, and both files are then
        # decoded with samtools before comparison. The header is compared with
        # @PG lines normalised away (both tools append their own provenance).
        our_bam = work / "ours.sorted.bam"
        their_bam = work / "theirs.sorted.bam"
        source = case.rust_argv[1]

        got = run([rust, "sort", source, "-o", str(our_bam)], work)
        want = run([samtools, "sort", source, "-O", "BAM", "-o", str(their_bam)], work)
        detail["rust_exit"] = got.exit_code
        detail["samtools_exit"] = want.exit_code
        if got.exit_code != 0:
            problems.append(f"rust-bam sort exited {got.exit_code}: {got.stderr.strip()}")
            return problems, detail
        if want.exit_code != 0:
            problems.append(f"samtools sort exited {want.exit_code}: {want.stderr.strip()}")
            return problems, detail

        our_text = run([samtools, "view", "-h", str(our_bam)], work)
        their_text = run([samtools, "view", "-h", str(their_bam)], work)
        if our_text.exit_code != 0:
            problems.append(
                f"samtools could not decode our sorted BAM: {our_text.stderr.strip()}"
            )
            return problems, detail
        detail["samtools_can_read_our_sorted_bam"] = True

        want_out = normalise(their_text.stdout, ["pg_lines"])
        got_out = normalise(our_text.stdout, ["pg_lines"])
        if want_out != got_out:
            problems.extend(["sorted output differs"] + diff_report(want_out, got_out))

        # And htslib's structural check must accept our sorted file.
        check = run([samtools, "quickcheck", str(our_bam)], work)
        detail["quickcheck_on_our_sorted_bam"] = check.exit_code
        if check.exit_code != 0:
            problems.append(
                f"samtools quickcheck rejected our sorted BAM (exit {check.exit_code}): "
                f"{check.stderr.strip()}"
            )
        return problems, detail

    # Everything else compares a text stream from both tools.
    want = run([samtools, *case.rust_argv], work)
    got = run([rust, *case.rust_argv], work)
    detail["samtools_exit"] = want.exit_code
    detail["rust_exit"] = got.exit_code

    want_out = normalise(want.stdout, case.normalise)
    got_out = normalise(got.stdout, case.normalise)
    if want.exit_code != got.exit_code:
        problems.append(f"exit {want.exit_code} -> {got.exit_code}")
    if want_out != got_out:
        problems.extend(["stdout differs"] + diff_report(want_out, got_out))
    return problems, detail


# --------------------------------------------------------------------------- #
# self-test
# --------------------------------------------------------------------------- #


def self_test() -> int:
    """Prove the comparison machinery can fail, without needing samtools."""
    ok = True

    if diff_report("a\nb\n", "a\nb\n") != ["trailing newline only"]:
        print("SELFTEST FAIL: identical text reported as different", file=sys.stderr)
        ok = False
    if not diff_report("a\nb\n", "a\nc\n"):
        print("SELFTEST FAIL: different text reported as identical", file=sys.stderr)
        ok = False

    # The @PG rule must drop @PG and nothing else.
    header = "@HD\tVN:1.6\n@PG\tID:samtools.1\tPN:samtools\n@SQ\tSN:chr1\tLN:10\n"
    expected = "@HD\tVN:1.6\n@SQ\tSN:chr1\tLN:10\n"
    if normalise(header, ["pg_lines"]) != expected:
        print(
            f"SELFTEST FAIL: pg_lines rule wrong: {normalise(header, ['pg_lines'])!r}",
            file=sys.stderr,
        )
        ok = False
    # A record line that happens to start with @PG-like text is not a header line.
    if normalise("@PGx\tnot a header\n", ["pg_lines"]) != "@PGx\tnot a header\n":
        print("SELFTEST FAIL: pg_lines rule over-matched", file=sys.stderr)
        ok = False

    # Every case must be structurally well formed.
    matrix = cases()
    if not matrix:
        print("SELFTEST FAIL: the case matrix is empty", file=sys.stderr)
        ok = False
    for case in matrix:
        if not case.name or not case.category or not case.guard:
            print(f"SELFTEST FAIL: case {case.name!r} has an empty field", file=sys.stderr)
            ok = False
        if not case.rust_argv:
            print(f"SELFTEST FAIL: case {case.name!r} has no argv", file=sys.stderr)
            ok = False
        if case.compare not in {
            "view", "flagstat", "idxstats", "quickcheck", "sort",
            "index", "index_unsorted", "bam_roundtrip",
        }:
            print(
                f"SELFTEST FAIL: case {case.name!r} has unknown compare {case.compare!r}",
                file=sys.stderr,
            )
            ok = False
    names = [c.name for c in matrix]
    if len(names) != len(set(names)):
        print("SELFTEST FAIL: duplicate case names", file=sys.stderr)
        ok = False

    # The fixture SAM must actually be valid: a mismatch here would make every
    # case fail for an unrelated reason.
    if not BASIC_SAM.startswith("@HD"):
        print("SELFTEST FAIL: BASIC_SAM has no header", file=sys.stderr)
        ok = False
    if "pair1" not in BASIC_SAM:
        print("SELFTEST FAIL: BASIC_SAM lost its records", file=sys.stderr)
        ok = False

    print("SELFTEST PASS" if ok else "SELFTEST FAIL")
    return 0 if ok else 1


def _selftest_build_fixtures_creates_workdir(samtools: str | None) -> bool:
    """Prove build_fixtures creates its own work directory.

    The W1-3 harness crashed on exactly this: fixtures were written into a
    directory no caller had created, so the very first case died with
    FileNotFoundError and zero comparisons ran. Exercised without samtools by
    using a case that needs no BAM conversion.
    """
    with tempfile.TemporaryDirectory(prefix="w14-selftest-") as tmp:
        nested = Path(tmp) / "does" / "not" / "exist" / "yet"
        case = Case(
            name="selftest_fixtures",
            category="selftest",
            guard="build_fixtures creates its own work directory",
            fixtures={"x.sam": BASIC_SAM},
            needs_bam=False,
        )
        try:
            build_fixtures(case, nested, samtools or "samtools")
        except OSError as exc:
            print(f"SELFTEST FAIL: build_fixtures could not create its work dir: {exc}",
                  file=sys.stderr)
            return False
        if not nested.is_dir():
            print("SELFTEST FAIL: build_fixtures did not create the work dir", file=sys.stderr)
            return False
        if not (nested / "x.sam").exists():
            print("SELFTEST FAIL: build_fixtures did not write the fixture", file=sys.stderr)
            return False
    return True


# --------------------------------------------------------------------------- #
# main
# --------------------------------------------------------------------------- #


def resolve_samtools(explicit: str | None) -> str | None:
    """Resolve samtools without hardcoding a personal path.

    Order: --samtools, $SAMTOOLS_ORACLE, the pinned oracle env's bin directory,
    then PATH.
    """
    if explicit:
        found = shutil.which(explicit) or explicit
        return found if os.access(found, os.X_OK) else None
    override = os.environ.get("SAMTOOLS_ORACLE")
    if override:
        found = shutil.which(override) or override
        if os.access(found, os.X_OK):
            return found
        return None
    for candidate in (
        os.environ.get("BIOINFO_ORACLE_BIN"),
        os.environ.get("CONDA_PREFIX", "") + "/bin" if os.environ.get("CONDA_PREFIX") else None,
    ):
        if candidate:
            path = os.path.join(candidate, "samtools")
            if os.access(path, os.X_OK):
                return path
    return shutil.which("samtools")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust", help="rust-bam binary")
    parser.add_argument("--samtools", help="samtools 1.24 binary")
    parser.add_argument("--json", help="write a JSON report here")
    parser.add_argument("--list", action="store_true", help="list cases and exit")
    parser.add_argument("--self-test", dest="self_test", action="store_true",
                        help="prove the comparison logic can fail")
    parser.add_argument("--filter", default="", help="only run cases whose name matches")
    args = parser.parse_args()

    samtools = resolve_samtools(args.samtools)

    if args.self_test:
        code = self_test()
        if not _selftest_build_fixtures_creates_workdir(samtools):
            code = 1
        return code

    matrix = [c for c in cases() if args.filter in c.name]

    if args.list:
        for case in matrix:
            print(f"{case.category:10s} {case.name:26s} {case.guard}")
        print(f"\n{len(matrix)} cases")
        return 0

    # Resolve both binaries to absolute paths: every case runs with cwd=<work>, so
    # a relative path would not be found.
    rust = os.path.abspath(args.rust) if args.rust else None
    if rust and not os.access(rust, os.X_OK):
        print(f"ERROR: rust-bam binary not executable: {rust}", file=sys.stderr)
        return 3
    if not rust:
        print("ERROR: --rust is required to run", file=sys.stderr)
        return 3
    if not samtools or not os.access(samtools, os.X_OK):
        print(
            "ERROR: samtools 1.24 not found. Set SAMTOOLS_ORACLE, or put samtools on "
            "PATH. Refusing to run: the oracle comparison cannot be evaluated.",
            file=sys.stderr,
        )
        return 3

    version = run([samtools, "--version"], Path.cwd())
    print(f"rust-bam : {rust}")
    print(f"samtools : {samtools}")
    print(f"{version.stdout.splitlines()[0] if version.stdout else 'unknown'}")
    print(f"cases    : {len(matrix)}\n")

    results = []
    failed = 0
    with tempfile.TemporaryDirectory(prefix="w14-diff-") as tmp:
        base = Path(tmp)
        for case in matrix:
            work = base / case.name
            problems: list[str] = []
            detail: dict = {}
            try:
                build_fixtures(case, work, samtools)
                problems, detail = compare_case(case, work, samtools, rust)
            except Exception as exc:  # noqa: BLE001 - a crash is a failed case
                problems = [f"harness error: {exc}"]

            status = "PASS" if not problems else "FAIL"
            if problems:
                failed += 1
            print(f"{status}  {case.category:10s} {case.name:26s} {case.guard}")
            for problem in problems:
                print(f"        {problem}")
            results.append({
                "name": case.name,
                "category": case.category,
                "guard": case.guard,
                "compare": case.compare,
                "argv": case.rust_argv,
                "status": status,
                "problems": problems,
                "detail": detail,
            })

    print(f"\n{len(matrix) - failed}/{len(matrix)} cases passed")
    if args.json:
        import json

        Path(args.json).write_text(
            json.dumps(
                {
                    "rust_bam": rust,
                    "samtools": samtools,
                    "samtools_version": version.stdout.splitlines()[0] if version.stdout else None,
                    "cases": results,
                    "total": len(matrix),
                    "passed": len(matrix) - failed,
                },
                indent=2,
            )
            + "\n"
        )
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
