#!/usr/bin/env python3
"""W1-1 byte-neutrality gate: record and verify the observable behaviour of the
two bioinfo-tools binaries.

W1-1 moves exit-code classes, diagnostic formatting, the `--summary-json`
contract and the provenance block out of `rust-cutadapt` and `rust-umitools`
into a shared `bioinfo-common` crate. The refactor may change *where* code
lives; it must not change a single observable byte. This tool records those
observables from the PRE-refactor binaries and compares POST-refactor builds
against that recording.

Modes
-----
    --record     run the case matrix, write one JSON file per case to --out
    --verify     run the case matrix, compare against --golden; non-zero on any
                 difference
    --self-test  assert the verifier rejects a mutated golden copy, and that the
                 unmutated copy still passes

`--self-test` is the anti-trivial check. A harness that cannot fail proves
nothing, so it is mandatory and is wired into `m3.sh`.

Comparison levels (docs/DEVELOPMENT-PLAN.md, "Comparison levels")
----------------------------------------------------------------
    L0  exit code equal                  every case
    L1  bytes equal                      files and binary outputs
    L2  text equal after named rules     reports (timing, cores, paths)
    L3  unused                           no float tolerance is needed here
    L4  unused                           every case below is L0-L2

Normalisation policy
--------------------
Normalisation is a small, explicit, *per-case, per-stream* list of named regex
rules. There is no blanket scrub, and a rule is never attached to a stream that
is supposed to be byte-exact (the provenance block gets no rules at all).

Two rules are parameterised by the run:

* `clap_basename` replaces the literal argv[0] basename with `<BIN>`. clap
  renders usage/help text from argv[0]'s basename, so the saved reference
  `rust-umitools.before` prints `Usage: rust-umitools.before ...` where the real
  build prints `rust-umitools`. The pattern is built with `re.escape` from the
  actual basename being executed, so it can only ever replace that exact
  literal. It is opt-in per case (`clap_text=True`) because some outputs, such
  as the provenance block, legitimately contain the program name as *data*.
* `absolute_temp_path` replaces this run's temp directory with `<TMP>`.

Python 3.12 standard library only. No network. No cargo dependency.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field, replace
from pathlib import Path

# <workspace>/tests/gate/snapshot.py
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


# --------------------------------------------------------------------------- #
# normalisation rules
# --------------------------------------------------------------------------- #

@dataclass(frozen=True)
class Rule:
    """One named regex normalisation attached to one named stream."""

    name: str
    pattern: str
    replacement: str

    def apply(self, text: str) -> tuple[str, bool]:
        new, count = re.subn(self.pattern, self.replacement, text, flags=re.MULTILINE)
        return new, count > 0


# `Finished in 0.123 s (45.678 us/read; 1.23 M reads/minute).` -- wall-clock
# timing. Only this line carries the numbers. The --json report has no timing
# field at all, which is why that document needs no rule.
R_FINISHED = Rule(
    "cutadapt_finished_line",
    r"^Finished in [0-9.]+ s \([0-9.]+ (\S+)s/read; [0-9.]+ M reads/minute\)\.$",
    r"Finished in <TIME> s (<TIME> \1s/read; <TIME> M reads/minute).",
)

# `Processing single-end reads on 4 cores ...` -- the resolved worker count is a
# property of -j and of the host (via -j 0, which reads available_parallelism),
# not of the refactor. Only the shape of the line is contractual.
# `Processing ... reads on 1 core ...` vs `on 4 cores ...`. R_CORES above keeps
# the singular/plural spelling because both sides are usually the same binary.
# For an oracle-recorded case the two implementations differ here by design
# (cutadapt defaults to one worker, rust-cutadapt to four), so the plural is
# normalised too.
R_CORES_ANY = Rule(
    "cutadapt_core_count_any_plural",
    r"^(Processing (?:single-end|paired-end) reads on )[0-9]+ cores?( \.\.\.)$",
    r"\1<N> cores\2",
)

# cutadapt's first stdout line names the implementation and its Python runtime:
#   `This is cutadapt 4.6 with Python 3.14.8`
#   `This is rust-cutadapt 0.1.0 with Python 3`  (honest: no Python involved)
# The differential harness normalises the same line for the same reason. It only
# matters for cases recorded from the upstream oracle, because every other case
# compares two builds of the same binary.
R_BANNER = Rule(
    "cutadapt_version_banner",
    r"^This is \S+ [0-9.]+ with Python [0-9.]+$",
    "This is <PROGRAM> <VERSION> with Python <PYVER>",
)

R_CORES = Rule(
    "cutadapt_core_count",
    r"^(Processing (?:single-end|paired-end) reads on )[0-9]+( cores? \.\.\.)$",
    r"\1<N>\2",
)

# `--summary-json` "timings": wall-clock seconds rendered %.3f. Key names,
# ordering, escaping and the trailing newline stay byte-exact; only the values
# are machine-dependent.
R_SUMMARY_TIMINGS = Rule(
    "umitools_summary_timings",
    r'"timings":\{[^}]*\}',
    '"timings":<TIMINGS>',
)

# `--summary-json` "resources": `threads` comes from available_parallelism
# capped by the cgroup CPU quota, `memory_mib` from /proc/meminfo and the cgroup
# memory limit, `chunk_mib` from the largest L3 cache and statvfs(tmpdir). All
# three are host properties, not run properties. See README, "Could not be made
# deterministic".
R_SUMMARY_RESOURCES = Rule(
    "umitools_summary_resources",
    r'"resources":\{[^}]*\}',
    '"resources":<RESOURCES>',
)

# The runtime log's timing lines: `wall time`, and in dedup also
# `parse+cluster` and `sort+commit`, all rendered `{:.1}s`.
R_LOG_WALL = Rule(
    "umitools_log_timing_lines",
    r"^((?:wall time|parse\+cluster|sort\+commit)\s+: )[0-9.]+s$",
    r"\1<TIME>s",
)

# The first log line reports the resolved budget:
#   `rust-umitools 0.2.0 dedup: threads=8 memory=4096MiB method=... threshold=...`
# `threads` comes from available_parallelism (capped by the cgroup CPU quota) and
# `memory` from /proc/meminfo, so both are host facts. The command name, method,
# threshold and representative strategy on the same line stay exact.
R_LOG_THREADS = Rule(
    "umitools_log_threads",
    r"\bthreads=[0-9]+",
    "threads=<N>",
)
R_LOG_MEMORY = Rule(
    "umitools_log_memory",
    r"\bmemory=[0-9]+MiB",
    "memory=<N>MiB",
)

# The dedup log mirrors HardwareProbe::summary(), which prints logical CPUs,
# physical CPUs, L3 size, available memory, free tmp space and cgroup limits.
# Those are host facts; normalising the two lines still leaves every counter and
# the event summary byte-exact.
R_LOG_HARDWARE = Rule(
    "umitools_log_hardware_line",
    r"^(hardware\s+: ).*$",
    r"\1<HARDWARE>",
)
R_LOG_LIMITS = Rule(
    "umitools_log_container_line",
    r"^(container limits\s+: ).*$",
    r"\1<LIMITS>",
)

# An I/O failure names the staging file it could not create, which is
# `.<prefix>.<pid>.<attempt>.tmp` (io/spill.rs). The PID is not reproducible
# between a record and a verify run, so it is normalised; the directory, prefix
# and error text (the parts a user acts on) stay exact.
R_TEMP_NAME = Rule(
    "umitools_temp_staging_name",
    r"\.([a-z0-9-]+)\.([0-9]+)\.([0-9]+)\.tmp\b",
    r".\1.<PID>.<N>.tmp",
)


# The @PG `CL` tag holds the full argv, whose first element is the binary path
# as it was invoked. A golden recorded from `.work/.../rust-umitools.before`
# must also verify against `target/release/rust-umitools`, so that absolute path
# is normalised. The rest of the command line stays byte-exact, which is what
# makes this a real check of the provenance record.
def binary_path_rule(binary: Path) -> Rule:
    return Rule(
        "binary_invocation_path",
        re.escape(str(binary.resolve() if binary.exists() else binary)),
        "<BINPATH>",
    )


def program_name_rules(basename: str) -> list[Rule]:
    """One rule that replaces the *program name* in clap-generated text.

    clap renders usage/help/error text from either argv[0]'s basename or an
    explicitly pinned `bin_name`, so the two sides of a comparison can disagree:

    * `rust-umitools` does not pin a bin_name, so the saved
      `rust-umitools.before` prints `Usage: rust-umitools.before ...` while a
      workspace build prints `rust-umitools`.
    * `rust-cutadapt` pins `.bin_name("rust-cutadapt")`, so BOTH the saved
      `rust-cutadapt.before` and a new build print `rust-cutadapt`.

    Replacing only the executed basename would fire on one side of the first
    case and on neither side of the second, and would fire on only one side of a
    record/verify pair, producing a false failure. So the alternation covers the
    resolved basename *and* both canonical names. Longest alternative first, so
    `rust-umitools.before` cannot be half-matched by `rust-umitools`.

    The rule has a *stable name* (`program_name`) even though its pattern depends
    on the run, because `rules_fired` is compared between the recorded and the
    verified observation. A per-run rule name would make every clap case fail.

    Attached only to cases whose text is clap-generated (`clap_text=True`). The
    provenance block is deliberately excluded: its program name is data, not
    argv[0], and it is compared byte-exactly.
    """
    literals = sorted(
        {basename, "rust-cutadapt", "rust-umitools", "cutadapt"},
        key=len,
        reverse=True,
    )
    pattern = "|".join(re.escape(literal) for literal in literals)
    return [Rule("program_name", pattern, "<BIN>")]


@dataclass
class Normalisation:
    """The ordered rules attached to one stream of one case."""

    rules: list[Rule] = field(default_factory=list)

    def apply(self, data: bytes) -> tuple[bytes, list[str]]:
        text = data.decode("utf-8", errors="surrogateescape")
        fired: list[str] = []
        for rule in self.rules:
            text, did_fire = rule.apply(text)
            if did_fire:
                fired.append(rule.name)
        return text.encode("utf-8", errors="surrogateescape"), fired


# --------------------------------------------------------------------------- #
# case model
# --------------------------------------------------------------------------- #

@dataclass
class Case:
    """One invocation and the observables it guards.

    `guard` names the refactor risk, so a failing case tells a reviewer which
    part of bioinfo-common broke.
    """

    name: str
    binary: str  # "cutadapt" | "umitools"
    guard: str
    argv: list[str]
    level: str  # "L0" | "L1" | "L2"
    streams: list[tuple[str, str]] = field(default_factory=list)  # (label, kind)
    files: list[tuple[str, str]] = field(default_factory=list)    # (label, path)
    # Stream label -> extra named rules for that stream.
    normalise: dict[str, list[Rule]] = field(default_factory=dict)
    # True when clap may have rendered argv[0]'s basename into this case's text.
    # Never set for the provenance block, whose program name is data.
    clap_text: bool = False
    # (left, right) observables that must be byte-equal after normalisation,
    # recorded as a synthetic `mirror:<left>==<right>` stream.
    mirror: list[tuple[str, str]] = field(default_factory=list)
    # Model expectation; a mismatch is reported loudly but the measured value is
    # what gets recorded, so the gate never encodes a guess.
    expect_exit: int | None = None
    # Record this case from the upstream oracle instead of from `--cutadapt`.
    #
    # Only for cases where the PRE-refactor binary was itself wrong, so its
    # output must not be frozen into a golden. The upstream tool is the
    # compatibility authority, so its output is the correct expectation. Using
    # this flag is a deliberate, visible statement that the baseline was buggy;
    # it must never be used to paper over a refactor regression.
    record_from_oracle: bool = False


@dataclass
class Observation:
    name: str
    guard: str
    level: str
    exit_code: int
    argv: list[str]
    streams: dict[str, dict]
    rules_fired: dict[str, list[str]]

    def to_json(self) -> dict:
        return {
            "argv": self.argv,
            "exit_code": self.exit_code,
            "guard": self.guard,
            "level": self.level,
            "name": self.name,
            "rules_fired": self.rules_fired,
            "streams": self.streams,
        }


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def render_text(raw: bytes, norm: Normalisation) -> tuple[dict, list[str]]:
    """Inline the text so a reviewer can read the golden; hash it if not UTF-8."""
    normalised, fired = norm.apply(raw)
    try:
        text = normalised.decode("utf-8")
    except UnicodeDecodeError:
        return {"kind": "sha256", "sha256": sha256(normalised), "bytes": len(raw)}, fired
    return {"kind": "text", "text": text, "bytes": len(raw)}, fired


def render_gzip(raw: bytes) -> dict:
    """The contract for a .gz output is its DECOMPRESSED bytes.

    The compressed hash is recorded as a note only: a deflate-level or backend
    change legitimately moves it (the repo already documents level 3 vs
    cutadapt's 6), so it must not be a contract failure.
    """
    try:
        plain = gzip.decompress(raw)
    except OSError as error:
        return {"kind": "gzip_error", "error": str(error), "sha256": sha256(raw)}
    return {
        "kind": "gzip",
        "compressed_sha256": sha256(raw),
        "decompressed_bytes": len(plain),
        "decompressed_sha256": sha256(plain),
    }


def render_bam(
    path: Path, samtools: str, rules: Normalisation | None = None
) -> tuple[dict, list[str]]:
    """The contract for a BAM is its header plus records, not its BGZF bytes.

    `samtools view -h` renders both as text, which is the plan's documented
    comparison rule for BAM and is readable in a golden. The raw BGZF hash is
    kept as a note, because a compression change would move it without
    violating the contract.
    """
    done = subprocess.run(
        [samtools, "view", "-h", str(path)], capture_output=True, check=False
    )
    raw_hash = sha256(path.read_bytes())
    if done.returncode != 0:
        return (
            {
                "kind": "bam_error",
                "error": done.stderr.decode("utf-8", "replace"),
                "sha256": raw_hash,
            },
            [],
        )
    # `samtools view -h` renders the header, and a header written by
    # `samtools sort` carries a @PG line whose CL tag embeds this run's temp
    # directory. A `--add-pg` run additionally embeds the binary path. The usual
    # rules must apply to the rendered text, or the golden is not reproducible
    # and a refactor of the provenance record could not be detected.
    rendered, fired = render_text(done.stdout, rules or Normalisation())
    # Deliberately not recorded: the raw BGZF hash. The plan's rule is the
    # decompressed header+records (D4), and the container bytes of a *correct*
    # output legitimately differ between two runs because the @PG CL tag holds
    # the run's temp directory.
    rendered.pop("bgzf_sha256", None)
    return rendered, fired


# --------------------------------------------------------------------------- #
# fixtures
# --------------------------------------------------------------------------- #

CUTADAPT_FIXTURES = ROOT / "rust-cutadapt" / "tests" / "fixtures"
SIMPLE_FASTQ = CUTADAPT_FIXTURES / "simple.fastq"
SIMPLE_GZ = CUTADAPT_FIXTURES / "simple.fastq.gz"
ADAPTER = "AGATCGGAAGAGC"

HEADER = (
    "@HD\tVN:1.6\tSO:coordinate\n"
    "@SQ\tSN:chr1\tLN:1000000\n"
    "@SQ\tSN:chr2\tLN:1000000\n"
)


def umi_record(
    name: str,
    umi: str,
    flag: int = 0,
    pos: int = 100,
    mapq: int = 60,
    cigar: str | None = None,
    extra: str = "",
    seq: str = "A" * 10,
) -> str:
    """A SAM record with a UMI in the QNAME, shaped like tests/oracle/exact.py."""
    if cigar is None:
        cigar = f"{len(seq)}M"
    return (
        f"{name}_{umi}\t{flag}\tchr1\t{pos}\t{mapq}\t{cigar}\t*\t0\t0"
        f"\t{seq}\t{'I' * len(seq)}{extra}\n"
    )


def umi_families_body() -> str:
    """A non-degenerate UMI layout: an exact tie, a one-edit chain, both strands
    at one position, a 9 nt family, and a trailing unmapped record."""
    rows: list[str] = []
    families = [
        (100, False, [("AAAAAAAA", 6, 60), ("CAAAAAAA", 3, 60), ("CCAAAAAA", 1, 60)]),
        (100, True, [("GGGGGGGG", 4, 60), ("GGGGGGGA", 4, 60)]),
        (200, False, [("ACGTACGT", 5, 60), ("ACGTACGA", 1, 60), ("ACGTACCA", 1, 60)]),
        (300, False, [("TTTTTTTT", 2, 40), ("TTTTTTTA", 2, 40), ("TTTTTTAA", 2, 40)]),
        (400, True, [("CCCCCCCC", 10, 60), ("CCCCCCCA", 1, 60)]),
    ]
    for pos, reverse, members in families:
        flag = 16 if reverse else 0
        for umi, count, mapq in members:
            for _ in range(count):
                rows.append(umi_record(f"r{len(rows)}", umi, flag=flag, pos=pos, mapq=mapq))
    rows.append("runmap_AAAAAAAA\t4\t*\t0\t0\t*\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\n")
    return HEADER + "".join(rows)


def umi_genes_body() -> str:
    """Gene-tagged records so `count --per-gene --gene-tag GX` has work to do."""
    genes = [
        ("G1", "AAAAAAAA"),
        ("G1", "AAAAAAAC"),
        ("G1", "TTTTTTTT"),
        ("G2", "CCCCCCCC"),
        ("G2", "CCCCCCCG"),
    ]
    return HEADER + "".join(
        umi_record(f"c{index}", umi, pos=1200 + index, extra=f"\tGX:Z:{gene}")
        for index, (gene, umi) in enumerate(genes)
    )


def umi_unsorted_body() -> str:
    """Deliberately out of coordinate order, and `samtools sort` is NOT applied,
    so the binary itself must detect the precondition violation."""
    return HEADER + umi_record("u0", "AAAAAAAA", pos=500) + umi_record(
        "u1", "CCCCCCCC", pos=100
    )


def write_bam(directory: Path, name: str, sam_body: str, sort: bool = True) -> Path:
    """Build a BAM the way rust-umitools' own oracle harness does."""
    sam = directory / f"{name}.sam"
    bam = directory / f"{name}.bam"
    sam.write_text(sam_body)
    command = (
        ["samtools", "sort", "-o", str(bam), str(sam)]
        if sort
        else ["samtools", "view", "-b", "-o", str(bam), str(sam)]
    )
    done = subprocess.run(command, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise RuntimeError(f"samtools failed for {name}: {done.stderr}")
    if sort:
        # umi_tools dedup fetches without a region, which needs an index.
        indexed = subprocess.run(
            ["samtools", "index", str(bam)], capture_output=True, text=True, check=False
        )
        if indexed.returncode != 0:
            raise RuntimeError(f"samtools index failed for {name}: {indexed.stderr}")
    return bam


# --------------------------------------------------------------------------- #
# oracle resolution, mirroring rust-umitools/tests/ci/oracle.sh
# --------------------------------------------------------------------------- #

def resolve_oracle_bin(workspace: Path) -> Path | None:
    """$UMI_TOOLS_ORACLE_PATH, $CONDA_PREFIX/bin, tests/ci/env/bin, then PATH.

    A candidate counts only when it holds BOTH umi_tools and samtools, so a run
    never mixes htslib versions. Returns None when nothing resolves; the caller
    then exits 3 instead of silently skipping the cases that need it.
    """
    candidates: list[Path] = []
    override = os.environ.get("UMI_TOOLS_ORACLE_PATH")
    if override:
        candidates.append(Path(override))
    conda = os.environ.get("CONDA_PREFIX")
    if conda:
        candidates.append(Path(conda) / "bin")
    candidates.append(workspace / "rust-umitools" / "tests" / "ci" / "env" / "bin")

    for candidate in candidates:
        if (
            candidate.is_dir()
            and os.access(candidate / "umi_tools", os.X_OK)
            and os.access(candidate / "samtools", os.X_OK)
        ):
            return candidate

    on_path = shutil.which("umi_tools")
    path_sam = shutil.which("samtools")
    if on_path and path_sam and Path(on_path).parent == Path(path_sam).parent:
        return Path(on_path).parent
    return None


def resolve_cutadapt_oracle(workspace: Path) -> Path | None:
    """The upstream cutadapt 4.6 binary, resolved the way the repo documents.

    Order: `$CUTADAPT_ORACLE`, the declared `tests/ci/env` prefix, an active
    `$CONDA_PREFIX`, a sibling `.work/oracle-venv`, then `PATH`. Only used to
    record cases whose pre-refactor output was itself wrong; never to relax a
    comparison.
    """
    candidates: list[Path] = []
    override = os.environ.get("CUTADAPT_ORACLE")
    if override:
        candidates.append(Path(override))
    candidates.append(workspace / "rust-cutadapt" / "tests" / "ci" / "env" / "bin" / "cutadapt")
    conda = os.environ.get("CONDA_PREFIX")
    if conda:
        candidates.append(Path(conda) / "bin" / "cutadapt")
    candidates.append(workspace / ".work" / "oracle-venv" / "bin" / "cutadapt")
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    found = shutil.which("cutadapt")
    return Path(found) if found else None


def resolve_samtools(oracle: Path | None) -> str | None:
    """Prefer the oracle's samtools so the fixture builder matches the suite."""
    if oracle is not None and os.access(oracle / "samtools", os.X_OK):
        return str(oracle / "samtools")
    return shutil.which("samtools")


# --------------------------------------------------------------------------- #
# the case matrix
# --------------------------------------------------------------------------- #

def cutadapt_cases() -> list[Case]:
    """rust-cutadapt. Fixtures: simple.fastq/.gz, adapter AGATCGGAAGAGC, -m 10.

    The input is always the fixture's bare filename: `stage_cutadapt` copies it
    into the case's own directory, so argv never contains an absolute path.
    """
    fastq = SIMPLE_FASTQ.name      # "simple.fastq"
    fastq_gz = SIMPLE_GZ.name      # "simple.fastq.gz"
    both = [("stdout", "exact"), ("stderr", "exact")]
    report = [R_CORES, R_FINISHED]
    cases: list[Case] = []

    cases.append(Case(
        "cutadapt_version", "cutadapt", "CLI version text", ["--version"], "L2",
        streams=[("stdout", "exact")], clap_text=True))
    cases.append(Case(
        "cutadapt_help", "cutadapt", "whole --help text (the 60-option grammar)",
        ["--help"], "L2", streams=[("stdout", "exact")], clap_text=True))

    # exit class 2: clap usage error.
    cases.append(Case(
        "cutadapt_unknown_option", "cutadapt",
        "exit class 2 + clap usage diagnostic",
        ["--not-a-real-option"], "L2", streams=both, clap_text=True, expect_exit=2))

    # exit class 1: cutadapt prints the banner, then the ERROR line, on STDOUT.
    cases.append(Case(
        "cutadapt_missing_input", "cutadapt",
        "exit class 1 + stdout ERROR line for a missing input file",
        ["-a", ADAPTER, "-m", "10", "-o", "out.fastq", "definitely-missing.fastq"],
        "L2", streams=both, normalise={"stdout": report}, expect_exit=1))

    cases.append(Case(
        "cutadapt_malformed_fastq", "cutadapt",
        "exit class 1 + exact malformed-FASTQ diagnostic",
        ["-a", ADAPTER, "-m", "10", "-o", "out.fastq", "malformed.fastq"],
        "L2", streams=both, normalise={"stdout": report}, expect_exit=1))

    cases.append(Case(
        "cutadapt_plain_fastq", "cutadapt",
        "L1 output bytes + full text report",
        ["-a", ADAPTER, "-m", "10", "-o", "out.fastq", fastq], "L2",
        streams=both, files=[("out.fastq", "out.fastq")],
        normalise={"stdout": report}, expect_exit=0))

    cases.append(Case(
        "cutadapt_gzip_fastq", "cutadapt",
        "L1 decompressed output bytes from a .gz input",
        ["-a", ADAPTER, "-m", "10", "-o", "out.fastq.gz", fastq_gz], "L2",
        streams=both, files=[("out.fastq.gz", "out.fastq.gz")],
        normalise={"stdout": report}, expect_exit=0))

    # Read-2 adapter options. `-A`/`-G`/`-B` select 3', 5' and anywhere adapters
    # for read 2, exactly as `-a`/`-g`/`-b` do for read 1. The option letter is
    # uppercase, and `adapter::parse` is given that uppercase letter, so the
    # mapping must be case-insensitive.
    #
    # This case exists because it was NOT covered when W1-1 was first completed:
    # `-G` and `-B` were silently downgraded to 3' adapters in release builds,
    # trimming the wrong end of read 2, and every suite still passed because
    # nothing exercised a 5'-class adapter on read 2. See docs/EVIDENCE-W1-1.md
    # DR-10.
    #
    # These three are recorded from the UPSTREAM oracle (`record_from_oracle`),
    # not from the pre-refactor binary: the pre-refactor binary answered `-G` and
    # `-B` wrongly, so its output must not be frozen as the expectation. The
    # upstream tool is the compatibility authority.
    # `-B` is deliberately NOT in this list. Its *trimming* is covered by
    # `-G` (the 5' path) and by the anywhere unit tests, but its text report has
    # a separate, pre-existing cosmetic difference from cutadapt (rust-cutadapt
    # omits the empty 5' histogram header in the anywhere-adapter block). That is
    # unrelated to W1-1 and asserting it here would make the W1-1 gate fail for a
    # reason W1-1 did not cause. It is recorded in docs/EVIDENCE-W1-1.md (DR-11).
    for option, what in (("-A", "3prime"), ("-G", "5prime")):
        cases.append(Case(
            f"cutadapt_read2_{what}", "cutadapt",
            f"read-2 adapter {option}: trims read 2 the way cutadapt 4.6 does",
            ["-a", ADAPTER, option, "ACGT", "-m", "0",
             "-o", "out1.fastq", "-p", "out2.fastq", fastq, fastq], "L2",
            streams=both,
            files=[("out1.fastq", "out1.fastq"), ("out2.fastq", "out2.fastq")],
            normalise={"stdout": [R_CORES_ANY, R_FINISHED, R_BANNER]}, expect_exit=0,
            clap_text=True, record_from_oracle=True))

    cases.append(Case(
        "cutadapt_quiet", "cutadapt",
        "L1 --quiet: empty report, unchanged output bytes",
        ["--quiet", "-a", ADAPTER, "-m", "10", "-o", "out.fastq", fastq], "L1",
        streams=both, files=[("out.fastq", "out.fastq")], expect_exit=0))

    # The --json document has no timing fields, so it must be byte-stable.
    cases.append(Case(
        "cutadapt_json_report", "cutadapt",
        "L1 --json report bytes (the document has no timing fields)",
        ["-a", ADAPTER, "-m", "10", "--json", "report.json", "-o", "out.fastq", fastq],
        "L1", streams=both,
        files=[("out.fastq", "out.fastq"), ("report.json", "report.json")],
        normalise={"stdout": report}, expect_exit=0))

    cases.append(Case(
        "cutadapt_report_minimal", "cutadapt",
        "L1 --report minimal stdout bytes (no banner, no timing line)",
        ["--report", "minimal", "-a", ADAPTER, "-m", "10", "-o", "out.fastq", fastq],
        "L1", streams=both, files=[("out.fastq", "out.fastq")], expect_exit=0))

    cases.append(Case(
        "cutadapt_debug", "cutadapt",
        "L2 --debug diagnostics are stable (adapter and step lines)",
        ["--debug", "-a", ADAPTER, "-m", "10", "-o", "out.fastq", fastq], "L2",
        streams=both, files=[("out.fastq", "out.fastq")],
        normalise={"stdout": report}, expect_exit=0))

    cases.append(Case(
        "cutadapt_empty_input", "cutadapt",
        "L2 empty input: exit 0 + 'No reads processed!' + full report",
        ["-a", ADAPTER, "-m", "10", "-o", "out.fastq", "empty.fastq"], "L2",
        streams=both, files=[("out.fastq", "out.fastq")],
        normalise={"stdout": report}, expect_exit=0))

    cases.append(Case(
        "cutadapt_demultiplex", "cutadapt",
        "L1 demultiplex per-file bytes (o_1.fastq, o_unknown.fastq)",
        ["-a", ADAPTER, "-m", "10", "-o", "o_{name}.fastq", fastq], "L2",
        streams=both,
        files=[("o_1.fastq", "o_1.fastq"), ("o_unknown.fastq", "o_unknown.fastq")],
        normalise={"stdout": report}, expect_exit=0))

    for cores in ("1", "2"):
        cases.append(Case(
            f"cutadapt_j{cores}", "cutadapt",
            f"-j {cores}: identical output bytes, core-count line normalised",
            ["-j", cores, "-a", ADAPTER, "-m", "10", "-o", "out.fastq", fastq], "L2",
            streams=both, files=[("out.fastq", "out.fastq")],
            normalise={"stdout": report}, expect_exit=0))

    return cases


def umitools_cases() -> list[Case]:
    """rust-umitools."""
    both = [("stdout", "exact"), ("stderr", "exact")]
    summary_rules = [R_SUMMARY_TIMINGS, R_SUMMARY_RESOURCES]

    # `Logger::info` writes to stderr ALWAYS, and to the `-L` file in addition
    # (commands/mod.rs). So every command case carries the host/timing lines on
    # stderr whether or not it passes `-L`. These rules are attached to every
    # command case's stderr and log stream; a rule that does not fire is
    # harmless, whereas missing one makes the golden machine-dependent.
    log_rules = [
        R_LOG_WALL, R_LOG_THREADS, R_LOG_MEMORY, R_LOG_HARDWARE, R_LOG_LIMITS,
    ]
    cases: list[Case] = []

    cases.append(Case(
        "umitools_version", "umitools", "CLI version text", ["--version"], "L2",
        streams=[("stdout", "exact")], clap_text=True))

    # THE provenance block: byte-exact, NO rules at all. `clap_text` is False on
    # purpose -- the program name on the first line is data here, not argv[0].
    cases.append(Case(
        "umitools_version_verbose", "umitools",
        "PROVENANCE BLOCK: byte-exact --version --verbose, zero normalisation",
        ["--version", "--verbose"], "L1", streams=both, expect_exit=0))

    cases.append(Case(
        "umitools_help", "umitools", "whole --help text", ["--help"], "L2",
        streams=[("stdout", "exact")], clap_text=True))
    for sub in ("dedup", "group", "count", "count_tab", "extract", "whitelist"):
        cases.append(Case(
            f"umitools_{sub}_help", "umitools",
            f"{sub} --help text (option grammar + after_help block)",
            [sub, "--help"], "L2", streams=[("stdout", "exact")], clap_text=True))

    # count_tab needs no BAM. Column 1 is `read_id<sep>UMI`, column 2 is the
    # gene (src/commands/count_tab.rs).
    cases.append(Case(
        "umitools_count_tab", "umitools",
        "L1 count_tab output bytes + --summary-json contract",
        ["count_tab", "-I", "reads.tsv", "-S", "out.tsv", "--summary-json", "s.json"],
        "L1", streams=both,
        files=[("out.tsv", "out.tsv"), ("s.json", "s.json")],
        normalise={"stderr": list(log_rules), "s.json": summary_rules},
        expect_exit=0))

    cases.append(Case(
        "umitools_count_tab_per_cell", "umitools",
        "L1 count_tab --per-cell output bytes",
        ["count_tab", "-I", "reads_cell.tsv", "-S", "out.tsv", "--per-cell"],
        "L1", streams=both, files=[("out.tsv", "out.tsv")],
        normalise={"stderr": list(log_rules)}, expect_exit=0))

    # A three-column line violates the table precondition: InputError::BadTable,
    # which error.rs maps to ExitClass::Input. The measured code is recorded.
    cases.append(Case(
        "umitools_count_tab_bad_table", "umitools",
        "input-precondition diagnostic + its exit code (BadTable)",
        ["count_tab", "-I", "bad.tsv", "-S", "out.tsv"], "L2", streams=both,
        normalise={"stderr": list(log_rules)}))

    for command in ("dedup", "group", "count"):
        # dedup/group write BAM; count writes a TSV (src/commands/count.rs).
        # The `-S` name must match the checked filename, or the comparison would
        # look for a file the tool never wrote.
        if command == "count":
            output = "out.tsv"
            files = [("out.tsv", "out.tsv"), ("s.json", "s.json")]
        else:
            output = "out.bam"
            files = [("out.bam", "out.bam"), ("s.json", "s.json")]
        argv = [command, "-I", "in.bam", "-S", output, "--random-seed", "1",
                "--summary-json", "s.json"]
        if command == "count":
            argv += ["--per-gene", "--gene-tag", "GX"]
        cases.append(Case(
            f"umitools_{command}", "umitools",
            f"{command}: output bytes + --summary-json contract",
            argv, "L1", streams=both, files=files,
            normalise={"stderr": list(log_rules), "s.json": summary_rules},
            expect_exit=0))

    # `--add-pg` writes the provenance @PG record into the BAM header. W1-1 moved
    # the tag set and its order into bioinfo_common::provenance::ProgramRecord,
    # so this case guards that refactor specifically: the ID/PN/VN/CL tags and
    # their order must survive. The CL tag holds the real argv (the binary path),
    # which is normalised; everything else in the header stays byte-exact.
    cases.append(Case(
        "umitools_add_pg", "umitools",
        "provenance @PG record: ID/PN/VN/CL tags and order unchanged",
        ["dedup", "-I", "in.bam", "-S", "out.bam", "--random-seed", "1", "--add-pg"],
        "L1", streams=both, files=[("out.bam", "out.bam")],
        normalise={"stderr": list(log_rules)}, expect_exit=0))

    cases.append(Case(
        "umitools_unknown_option", "umitools",
        "exit class 2 + clap usage diagnostic",
        ["--not-a-real-option"], "L2", streams=both, clap_text=True, expect_exit=2))

    # --progress is validated (not ignored) even though --quiet suppresses its
    # effect; count_tab does not accept the flag, so `group` is used here.
    cases.append(Case(
        "umitools_bad_progress", "umitools",
        "exit class 2 + exact 'unknown --progress value' diagnostic + hint",
        ["group", "-I", "in.bam", "--group-out", "g.tsv", "--progress", "sometimes"],
        "L2", streams=both, expect_exit=2))

    cases.append(Case(
        "umitools_missing_input", "umitools",
        "exit class 4 + exact IO diagnostic for a missing input",
        ["count_tab", "-I", "nope.tsv", "-S", "out.tsv"], "L2",
        streams=both, expect_exit=4))

    cases.append(Case(
        "umitools_unsorted_bam", "umitools",
        "exit class 3 + exact NotSorted diagnostic",
        ["dedup", "-I", "unsorted.bam", "-S", "out.bam"], "L2",
        streams=both,
        # dedup logs its resolved budget and the hardware probe before it reads
        # the input, so stderr carries host facts (threads, memory, L3 size,
        # free tmp space) in front of the diagnostic. The diagnostic itself and
        # every counter stay exact.
        normalise={"stderr": list(log_rules)},
        expect_exit=3))

    cases.append(Case(
        "umitools_io_failure", "umitools",
        "exit class 4 + exact IO diagnostic when the output parent is not a directory",
        ["count_tab", "-I", "reads.tsv", "-S", "locked/out.tsv"], "L2",
        streams=both,
        normalise={"stderr": [*log_rules, R_TEMP_NAME]}))

    # -L mirroring. count_tab's log has no timing or hardware line, but stderr
    # still receives any that the command emits, so both sides carry the same
    # rule set and the mirror comparison stays meaningful.
    cases.append(Case(
        "umitools_logfile", "umitools",
        "L1 -L log bytes unchanged + log/stderr mirroring",
        ["count_tab", "-I", "reads.tsv", "-S", "out.tsv", "-L", "run.log"], "L1",
        streams=both, files=[("run.log", "run.log")],
        normalise={"run.log": list(log_rules), "stderr": list(log_rules)},
        mirror=[("run.log", "stderr")], expect_exit=0))

    # dedup's log adds timing lines, the resolved budget and the host hardware
    # probe; the same rules must apply to both streams or the mirror check would
    # compare a normalised log against an unnormalised stderr.
    cases.append(Case(
        "umitools_logfile_dedup", "umitools",
        "L2 dedup -L log: timings/budget/host lines normalised, rest exact",
        ["dedup", "-I", "in.bam", "-S", "out.bam", "--random-seed", "1",
         "-L", "run.log"], "L2", streams=both,
        files=[("run.log", "run.log")],
        normalise={"run.log": list(log_rules), "stderr": list(log_rules)},
        mirror=[("run.log", "stderr")], expect_exit=0))

    return cases


# --------------------------------------------------------------------------- #
# staging
# --------------------------------------------------------------------------- #

def stage_cutadapt(case: Case, work: Path) -> None:
    """Copy the fixtures next to the case and use RELATIVE argv paths.

    Absolute fixture paths would embed the workspace location in every golden
    and in the recorded `argv`, so a golden recorded on one machine could never
    be verified on another. Copying is cheap and the FASTQ fixtures are tiny.
    """
    if case.name == "cutadapt_malformed_fastq":
        # Quality length does not match sequence length, so the parser must
        # reject the record rather than silently truncate it.
        (work / "malformed.fastq").write_text("@r1\nACGTACGT\n+\nIII\n")
        return
    if case.name == "cutadapt_empty_input":
        (work / "empty.fastq").write_bytes(b"")
        return
    # Every other case reads a repository fixture. The `.gz` case is identified
    # by its argv, and the exact basename is used so a future fixture rename
    # fails loudly here rather than silently staging the wrong file.
    gzipped = "simple.fastq.gz" in " ".join(case.argv)
    source = CUTADAPT_FIXTURES / ("simple.fastq.gz" if gzipped else "simple.fastq")
    if not source.is_file():
        raise SystemExit(f"missing cutadapt fixture: {source}")
    shutil.copyfile(source, work / source.name)


def stage_umitools(case: Case, work: Path, samtools: str | None) -> None:
    if case.name == "umitools_io_failure":
        # Force a real I/O error that does not depend on the caller's UID.
        # `TempOutput::create` opens `<dir>/.<prefix>.<pid>.<n>.tmp` with
        # create_new in the destination's parent, so making that parent a
        # *regular file* yields ENOTDIR ("Not a directory") for root and
        # non-root alike. A read-only directory would NOT fail for root, which
        # would silently turn this case into a passing no-op.
        (work / "locked").write_text("this is a file, not a directory\n")
        (work / "reads.tsv").write_text("r1_AAAAAAAA\tG1\n")
        return

    if case.name.startswith("umitools_count_tab") or case.name == "umitools_logfile":
        (work / "reads.tsv").write_text(
            "r1_AAAAAAAA\tG1\nr2_AAAAAAAA\tG1\nr3_CCCCCCCC\tG1\nr4_TTTTTTTT\tG2\n"
        )
        (work / "reads_cell.tsv").write_text(
            "r1_CB1_AAAAAAAA\tG1\nr2_CB1_AAAAAAAA\tG1\n"
            "r3_CB2_AAAAAAAA\tG1\nr4_CB2_CCCCCCCC\tG1\n"
        )
        (work / "bad.tsv").write_text("r1_AAAAAAAA\tG1\textra\n")
        return

    needs_bam = case.name in {
        "umitools_dedup",
        "umitools_add_pg",
        "umitools_group",
        "umitools_count",
        "umitools_bad_progress",
        "umitools_unsorted_bam",
        "umitools_logfile_dedup",
    }
    if not needs_bam:
        return
    if samtools is None:
        raise SystemExit("samtools is required for the BAM-backed umitools cases")

    if case.name == "umitools_count":
        write_bam(work, "in", umi_genes_body())
    elif case.name == "umitools_unsorted_bam":
        write_bam(work, "unsorted", umi_unsorted_body(), sort=False)
    else:
        write_bam(work, "in", umi_families_body())


def stage(case: Case, work: Path, samtools: str | None) -> None:
    if case.binary == "cutadapt":
        stage_cutadapt(case, work)
    else:
        stage_umitools(case, work, samtools)


# --------------------------------------------------------------------------- #
# observation
# --------------------------------------------------------------------------- #

@dataclass
class Context:
    cutadapt: Path | None
    umitools: Path | None
    workspace: Path
    samtools: str | None
    env: dict[str, str]
    # The upstream cutadapt 4.6 binary, when one resolves. Used only to record
    # cases whose pre-refactor output was itself wrong (`record_from_oracle`).
    cutadapt_oracle: Path | None = None


def build_env(oracle: Path | None) -> dict[str, str]:
    env = dict(os.environ)
    # Remove knobs that would let the environment change the produced bytes.
    # RUST_UMITOOLS_TRACE is a diagnostics switch that adds `[trace]` lines to
    # stderr (src/commands/dedup.rs), so it must not leak in from the caller.
    for knob in (
        "RUST_UMITOOLS_MAX_PENDING_UMIS",
        "RUST_UMITOOLS_MAX_MATES",
        "RUST_UMITOOLS_IO_THREADS",
        "RUST_UMITOOLS_TRACE",
        "RUST_UMITOOLS_QUIET",
    ):
        env.pop(knob, None)
    env["LC_ALL"] = "C"
    env["TZ"] = "UTC"
    # clap wraps help/usage text to the terminal width; pin it so a wider
    # terminal cannot change the golden.
    env["COLUMNS"] = "100"
    if oracle is not None:
        env["PATH"] = f"{oracle}{os.pathsep}{env.get('PATH', '')}"
    return env


def rules_for(
    case: Case, label: str, basename: str, work: Path, binary: Path | None = None
) -> Normalisation:
    """The ordered rules for one stream: named case rules, then run-derived."""
    rules = list(case.normalise.get(label, []))
    if case.clap_text:
        rules.extend(program_name_rules(basename))
    if binary is not None:
        rules.append(binary_path_rule(binary))
    # Replace exactly this run's temp directory.
    rules.append(Rule("absolute_temp_path", re.escape(str(work)), "<TMP>"))
    return Normalisation(rules)


def observe(case: Case, binary: Path, work: Path, context: Context) -> Observation:
    """Run one case in the already-staged `work` directory."""
    basename = binary.name
    done = subprocess.run(
        [str(binary), *case.argv],
        cwd=work,
        env=context.env,
        capture_output=True,
        check=False,
    )

    streams: dict[str, dict] = {}
    fired: dict[str, list[str]] = {}
    raw_streams = {"stdout": done.stdout, "stderr": done.stderr}

    for label, kind in case.streams:
        if label not in raw_streams:
            raise SystemExit(f"case {case.name}: unknown stream {label!r}")
        norm = rules_for(case, label, basename, work, binary)
        if kind == "exact":
            rendered, names = render_text(raw_streams[label], norm)
        elif kind == "sha256":
            normalised, names = norm.apply(raw_streams[label])
            rendered = {
                "kind": "sha256",
                "sha256": sha256(normalised),
                "bytes": len(raw_streams[label]),
            }
        else:
            raise SystemExit(f"case {case.name}: unknown stream kind {kind!r}")
        streams[label] = rendered
        if names:
            fired[label] = names

    for label, template in case.files:
        path = work / template
        if not path.exists():
            streams[label] = {"kind": "missing"}
            continue
        if template.endswith(".gz"):
            streams[label] = render_gzip(path.read_bytes())
            continue
        if template.endswith(".bam"):
            if context.samtools is None:
                raise SystemExit("samtools is required to compare BAM outputs")
            rendered, names = render_bam(
                path,
                context.samtools,
                rules_for(case, label, basename, work, binary),
            )
        else:
            rendered, names = render_text(
                path.read_bytes(), rules_for(case, label, basename, work, binary)
            )
        streams[label] = rendered
        if names:
            fired[label] = names

    # Structural invariant: the -L log mirrors the human diagnostics.
    for left, right in case.mirror:
        left_text = streams.get(left, {}).get("text")
        right_text = streams.get(right, {}).get("text")
        if left_text is None or right_text is None:
            verdict = "<unavailable>\n"
        else:
            verdict = "equal\n" if left_text == right_text else "DIFFERENT\n"
        streams[f"mirror:{left}=={right}"] = {"kind": "text", "text": verdict}

    return Observation(
        name=case.name,
        guard=case.guard,
        level=case.level,
        exit_code=done.returncode,
        # Recorded output paths must not carry this run's temp directory, or the
        # golden would not be reproducible. The same rule machinery is reused so
        # `--argv` and the streams stay consistent.
        argv=[
            Normalisation(rules_for(case, "argv", basename, work).rules)
            .apply(item.encode("utf-8", errors="surrogateescape"))[0]
            .decode("utf-8", errors="surrogateescape")
            for item in case.argv
        ],
        streams=streams,
        rules_fired=fired,
    )


# --------------------------------------------------------------------------- #
# comparison
# --------------------------------------------------------------------------- #

def first_line_difference(want: str, got: str) -> list[str]:
    want_lines = want.splitlines()
    got_lines = got.splitlines()
    for index in range(max(len(want_lines), len(got_lines))):
        left = want_lines[index] if index < len(want_lines) else "<missing line>"
        right = got_lines[index] if index < len(got_lines) else "<missing line>"
        if left != right:
            return [
                f"first difference at line {index + 1}",
                f"    expected: {left!r}",
                f"    got     : {right!r}",
            ]
    return ["texts differ only in the trailing newline"]


def compare_stream(label: str, want: dict, got: dict) -> tuple[list[str], list[str]]:
    """Return (failures, informational notes).

    `bytes` is a length hint next to an inlined text, not an independent
    contract: when the text is equal the byte count is equal by construction.
    Comparing it separately would fail a report that legitimately changed its
    length. Hashes, on the other hand, are the contract for binary output.
    """
    kind = want.get("kind")
    if kind != got.get("kind"):
        return [f"{label}: observable kind changed {kind} -> {got.get('kind')}"], []

    if kind == "text":
        if want.get("text") != got.get("text"):
            return [f"{label}: text changed"] + first_line_difference(
                want.get("text", ""), got.get("text", "")
            ), []
        # Text is equal, so the decompressed contract holds. The BGZF container
        # is not part of the contract (plan decision D4); nothing further to
        # compare here.
        return [], []

    if kind == "sha256":
        if want.get("sha256") != got.get("sha256"):
            return [
                f"{label}: sha256 {want.get('sha256')} -> {got.get('sha256')}",
                f"    bytes {want.get('bytes')} -> {got.get('bytes')}",
            ], []
        return [], []

    if kind == "gzip":
        if want.get("decompressed_sha256") != got.get("decompressed_sha256"):
            return [
                f"{label}: DECOMPRESSED sha256 "
                f"{want.get('decompressed_sha256')} -> {got.get('decompressed_sha256')}"
            ], []
        if want.get("compressed_sha256") != got.get("compressed_sha256"):
            return [], [
                f"{label}: note: compressed bytes differ; decompressed bytes are "
                f"equal, so this is not a contract failure"
            ]
        return [], []

    if want == got:
        return [], []
    return [f"{label}: {want!r} != {got!r}"], []


def golden_path(golden: Path, case: Case) -> Path:
    return golden / f"{case.name}.json"


# --------------------------------------------------------------------------- #
# driver
# --------------------------------------------------------------------------- #

def prepare(cutadapt: str | None, umitools: str | None, workspace: str) -> Context | None:
    ws = Path(workspace).resolve()
    oracle = resolve_oracle_bin(ws)
    if umitools and oracle is None:
        print(
            "ERROR: umi_tools and samtools were not found. Set UMI_TOOLS_ORACLE_PATH, "
            "activate the pinned conda environment, create rust-umitools/tests/ci/env, "
            "or put both tools on PATH. Refusing to skip the cases that need them.",
            file=sys.stderr,
        )
        return None
    for label, path in (("cutadapt", cutadapt), ("umitools", umitools)):
        if path and not os.access(path, os.X_OK):
            print(f"ERROR: {label} binary is not executable: {path}", file=sys.stderr)
            return None
    return Context(
        cutadapt=Path(cutadapt).resolve() if cutadapt else None,
        umitools=Path(umitools).resolve() if umitools else None,
        workspace=ws,
        samtools=resolve_samtools(oracle),
        env=build_env(oracle),
        cutadapt_oracle=resolve_cutadapt_oracle(ws),
    )


def cases_for(context: Context) -> list[Case]:
    cases: list[Case] = []
    if context.cutadapt is not None:
        cases.extend(cutadapt_cases())
    if context.umitools is not None:
        cases.extend(umitools_cases())
    return cases


def run_case(case: Case, context: Context, base: Path) -> Observation:
    work = base / case.name
    work.mkdir()
    stage(case, work, context.samtools)
    binary = context.cutadapt if case.binary == "cutadapt" else context.umitools
    assert binary is not None
    return observe(case, binary, work, context)


def run_all(context: Context, handle) -> None:
    """Run every case under one temporary base directory."""
    with tempfile.TemporaryDirectory(prefix="w1-1-gate-") as tmp:
        handle(Path(tmp))


def record(context: Context, out_dir: Path) -> int:
    out_dir.mkdir(parents=True, exist_ok=True)
    for stale in out_dir.glob("*.json"):
        stale.unlink()

    written = 0
    warnings = 0

    def body(base: Path) -> None:
        nonlocal written, warnings
        for case in cases_for(context):
            if case.record_from_oracle:
                if context.cutadapt_oracle is None:
                    print(
                        f"ERROR {case.name}: recorded from the oracle, but no cutadapt "
                        f"oracle resolved (set CUTADAPT_ORACLE)",
                        file=sys.stderr,
                    )
                    raise SystemExit(3)
                observation = run_case(
                    replace(case, binary="cutadapt"),
                    replace(context, cutadapt=context.cutadapt_oracle),
                    base,
                )
            else:
                observation = run_case(case, context, base)
            if case.expect_exit is not None and observation.exit_code != case.expect_exit:
                # Record the measured code; a wrong model expectation must be
                # visible but must never be written into the golden as a guess.
                print(
                    f"WARN  {case.name}: measured exit {observation.exit_code}, "
                    f"model expected {case.expect_exit} -- recording the measured code",
                    file=sys.stderr,
                )
                warnings += 1
            golden_path(out_dir, case).write_text(
                json.dumps(observation.to_json(), indent=2, sort_keys=True) + "\n"
            )
            written += 1
            print(f"recorded  {case.name}  exit={observation.exit_code}")

    run_all(context, body)
    print(f"\nrecorded {written} cases into {out_dir}")
    if warnings:
        print(
            f"WARNING: {warnings} case(s) exited differently from the model "
            f"expectation; the measured codes were recorded.",
            file=sys.stderr,
        )
    return 0


def verify(context: Context, golden: Path) -> int:
    if not golden.is_dir():
        print(f"ERROR: golden directory not found: {golden}", file=sys.stderr)
        return 3

    state = {"checked": 0, "failures": 0}

    def body(base: Path) -> None:
        for case in cases_for(context):
            path = golden_path(golden, case)
            if not path.exists():
                print(f"FAIL  {case.name}: no golden at {path}", file=sys.stderr)
                state["failures"] += 1
                continue
            want = json.loads(path.read_text())
            got = run_case(case, context, base)

            problems: list[str] = []
            notes: list[str] = []

            # L0 first: if the exit class moved, nothing else is meaningful.
            if want["exit_code"] != got.exit_code:
                problems.append(
                    f"exit code {want['exit_code']} -> {got.exit_code} "
                    f"(guards: {case.guard})"
                )
            if want["argv"] != got.argv:
                problems.append(f"argv changed: {want['argv']} -> {got.argv}")

            for label in want["streams"]:
                if label not in got.streams:
                    problems.append(f"{label}: missing from the new observation")
                    continue
                stream_problems, stream_notes = compare_stream(
                    label, want["streams"][label], got.streams[label]
                )
                problems.extend(stream_problems)
                notes.extend(stream_notes)
            for label in got.streams:
                if label not in want["streams"]:
                    problems.append(f"{label}: unexpected new observable")

            # A rule that no longer fires means the volatile text it covered has
            # changed shape. Counting that as a pass would silently weaken the
            # gate, so it is a failure.
            for label, names in want.get("rules_fired", {}).items():
                for name in sorted(set(names) - set(got.rules_fired.get(label, []))):
                    problems.append(
                        f"{label}: normalisation rule {name!r} no longer matches "
                        f"(the text it covered changed shape)"
                    )

            state["checked"] += 1
            if problems:
                state["failures"] += 1
                print(f"FAIL  {case.name}  [{case.level}]  {case.guard}")
                for problem in problems:
                    print(f"        {problem}")
            else:
                print(f"PASS  {case.name}  [{case.level}]  {case.guard}")
            for note in notes:
                print(f"        {note}")

    run_all(context, body)
    checked, failures = state["checked"], state["failures"]
    print(f"\n{checked - failures}/{checked} cases pass")
    return 0 if failures == 0 else 1


# --------------------------------------------------------------------------- #
# self-test: prove the harness can fail
# --------------------------------------------------------------------------- #

def mutate_text_byte(path: Path) -> str | None:
    """Change one alphanumeric character in a real text stream."""
    data = json.loads(path.read_text())
    for label, stream in data["streams"].items():
        if label.startswith("mirror:"):
            continue  # synthetic; mutate the real observable instead
        text = stream.get("text")
        if stream.get("kind") == "text" and text:
            for index, char in enumerate(text):
                if char.isalnum():
                    swap = "Z" if char != "Z" else "Q"
                    stream["text"] = text[:index] + swap + text[index + 1:]
                    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
                    return data["name"]
    return None


def mutate_exit_code(path: Path) -> str | None:
    data = json.loads(path.read_text())
    data["exit_code"] = data["exit_code"] + 1
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
    return data["name"]


def mutate_hash(path: Path) -> str | None:
    """Flip a hash that is actually compared as a contract, not a note."""
    data = json.loads(path.read_text())
    for stream in data["streams"].values():
        key = None
        if stream.get("kind") == "sha256":
            key = "sha256"
        elif stream.get("kind") == "gzip":
            key = "decompressed_sha256"
        if key is not None:
            original = stream[key]
            stream[key] = ("0" if original[0] != "0" else "1") + original[1:]
            path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
            return data["name"]
    return None


MUTATIONS = [
    ("one byte in a text stream", mutate_text_byte),
    ("one exit code", mutate_exit_code),
    ("one hash in a binary stream", mutate_hash),
]


def self_test(context: Context, golden: Path) -> int:
    """Prove the verifier fails on a mutated golden and passes on the clean one."""
    if not golden.is_dir():
        print(f"SELFTEST FAIL  no golden directory at {golden}", file=sys.stderr)
        return 1

    # The clean golden must pass first. Without this the self-test proves
    # nothing: a verifier that always fails would "catch" every mutation.
    print("self-test: verifying the clean golden")
    if verify(context, golden) != 0:
        print("SELFTEST FAIL  the clean golden does not verify", file=sys.stderr)
        return 1
    print("self-test: clean golden verifies\n")

    ok = True
    with tempfile.TemporaryDirectory(prefix="w1-1-selftest-") as tmp:
        for index, (label, mutate) in enumerate(MUTATIONS):
            copy = Path(tmp) / f"mutation{index}"
            shutil.copytree(golden, copy)
            changed = None
            for path in sorted(copy.glob("*.json")):
                changed = mutate(path)
                if changed:
                    break
            if changed is None:
                print(f"SELFTEST FAIL  no case was mutable for {label!r}", file=sys.stderr)
                ok = False
                continue
            print(f"self-test: {label} (in {changed})")
            if verify(context, copy) == 0:
                print(f"SELFTEST FAIL  mutation {label!r} was NOT detected", file=sys.stderr)
                ok = False
            else:
                print(f"self-test: mutation {label!r} correctly rejected\n")

    print("SELFTEST PASS" if ok else "SELFTEST FAIL")
    return 0 if ok else 1


# --------------------------------------------------------------------------- #
# cli
# --------------------------------------------------------------------------- #

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="W1-1 byte-neutrality gate for the bioinfo-tools binaries.")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--record", action="store_true", help="record goldens")
    mode.add_argument("--verify", action="store_true", help="verify against goldens")
    mode.add_argument("--self-test", dest="self_test", action="store_true",
                      help="prove the verifier rejects a mutated golden")
    parser.add_argument("--cutadapt", help="path to the rust-cutadapt binary")
    parser.add_argument("--umitools", help="path to the rust-umitools binary")
    parser.add_argument("--out", help="golden output directory (--record)")
    parser.add_argument("--golden", help="golden directory (--verify/--self-test)")
    parser.add_argument("--workspace", default=str(ROOT),
                        help="workspace root for oracle discovery (default: repo root)")
    args = parser.parse_args(argv)

    if not args.cutadapt and not args.umitools:
        parser.error("at least one of --cutadapt/--umitools is required")
    if args.record and not args.out:
        parser.error("--record requires --out DIR")
    if (args.verify or args.self_test) and not args.golden:
        parser.error("--verify/--self-test require --golden DIR")

    context = prepare(args.cutadapt, args.umitools, args.workspace)
    if context is None:
        return 3

    if args.record:
        return record(context, Path(args.out))
    if args.verify:
        return verify(context, Path(args.golden))
    return self_test(context, Path(args.golden))


if __name__ == "__main__":
    sys.exit(main())
