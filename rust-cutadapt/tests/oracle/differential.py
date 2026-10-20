#!/usr/bin/env python3
"""Differential test harness: rust-cutadapt vs a real cutadapt 4.6 oracle.

Runs the reference implementation and the Rust binary on identical inputs and
arguments, then compares everything a user can observe:

  * exit status,
  * decompressed stdout and every output FASTQ/FASTA file,
  * --info-file / --rest-file / --wildcard-file contents,
  * --json report (field by field, ignoring inherently non-deterministic fields),
  * the text report (normalised: timing, versions and paths are stripped).

Usage
-----
    python3 tests/oracle/differential.py                    # run the default suite
    python3 tests/oracle/differential.py --matrix           # compact pass/fail matrix
    python3 tests/oracle/differential.py --filter 5prime    # subset by case name
    python3 tests/oracle/differential.py --list             # list case names
    python3 tests/oracle/differential.py --self-check       # harness sanity only
    python3 tests/oracle/differential.py --json out.json    # machine-readable result

Environment
-----------
    CUTADAPT_ORACLE   path to the reference binary (default: search below)
    RUST_CUTADAPT     path to the Rust binary (default: target/release/rust-cutadapt)

The harness needs no network. It exits 0 only when every selected case passes.
"""
from __future__ import annotations

import argparse
import dataclasses
import gzip
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

def _find_root() -> Path:
    here = Path(__file__).resolve()
    for parent in here.parents:
        if (parent / "Cargo.toml").exists() or (parent / "tests").is_dir():
            return parent
    return here.parent


ROOT = _find_root()

# --------------------------------------------------------------------------
# Binary discovery
# --------------------------------------------------------------------------

def _oracle_candidates():
    candidates = [ROOT / "tests/ci/env/bin/cutadapt"]
    conda_prefix = os.environ.get("CONDA_PREFIX")
    if conda_prefix:
        candidates.append(Path(conda_prefix) / "bin/cutadapt")
    # a venv created next to the repo (.work/oracle-venv) as well as inside it
    candidates.extend(p / ".work/oracle-venv/bin/cutadapt" for p in ROOT.parents)
    return candidates


DEFAULT_ORACLE_CANDIDATES = _oracle_candidates()


def find_oracle(explicit: str | None) -> Path | None:
    if explicit:
        p = Path(explicit)
        return p if p.exists() else None
    env = os.environ.get("CUTADAPT_ORACLE")
    if env:
        p = Path(env)
        return p if p.exists() else None
    for candidate in DEFAULT_ORACLE_CANDIDATES:
        if candidate.exists():
            return candidate
    found = shutil.which("cutadapt")
    return Path(found) if found else None


def find_rust(explicit: str | None) -> Path | None:
    if explicit:
        p = Path(explicit)
        return p if p.exists() else None
    env = os.environ.get("RUST_CUTADAPT")
    if env:
        p = Path(env)
        return p if p.exists() else None
    for profile in ("release", "debug"):
        p = ROOT / f"target/{profile}/rust-cutadapt"
        if p.exists():
            return p
    found = shutil.which("rust-cutadapt")
    return Path(found) if found else None


# --------------------------------------------------------------------------
# Fixtures
# --------------------------------------------------------------------------

ADAPTER3 = "AGATCGGAAGAGC"
ADAPTER5 = "ACGTACGTACGTACGT"

# Small deterministic single-end FASTQ covering the interesting shapes:
# full 3' adapter, internal adapter, partial 3' adapter, mismatch, no adapter,
# 5' adapter, low quality at both ends, N bases, poly-A, CASAVA header,
# empty-trim result, and a short read.
SINGLE_END_RECORDS = [
    ("@full",          "GCTAAAGACAATTACATAACATACACGTCAGCACGAAACT" + ADAPTER3),
    ("@internal",      "TGTTGGCCCAGTGTGAATCGCTTAAGGGTT" + ADAPTER3 + "AAGTAAGTGT"),
    ("@partial",       "GATGCATACGCCTTTACTTGCTGTGTCCACCCCATCGGACTGGCAAGATCG"),
    ("@none",          "TTTTTATTACACTCAGAAACAGAACTCGGGTAATTTTGACAGGTCACGCA"),
    ("@mismatch",      "GAGGCGCGCCCTCCTGAAGTGCGTGGACACTCGCTATGAAAGATCGTAAGAGC"),
    ("@adapter_start", ADAPTER3 + "GCTCTATTGACTACGACGCGCTCATTCCCTTGTCGGAGAG"),
    ("@short",         "ACGTA"),
    ("@fiveprime",     ADAPTER5 + "TTGACCGTAAACGTTGACCGTAAACGT"),
    ("@lowqual",       "ACGTTGCAACGTAGCTAGCTAGGCTA" + ADAPTER3),
    ("@withN",         "ACGTNNNNACGTACGTTGCAACGTAGCTAGCTAGGCTA" + ADAPTER3),
    ("@polya",         "ACGTACGTTGCAACGTAGCTAGCTAGGCTAAAAAAAAAAAA"),
    ("@casava",        "ACGTTGCAACGTAGCTAGCTAGGCTA" + ADAPTER3),
    # Two adapters in one read (for -n / --times)
    ("@twoadapters",   "ACGTACGTTGCAACGT" + ADAPTER3 + "GGGGGGGG" + ADAPTER3),
]


def qualities_for(seq: str, name: str = "") -> str:
    """Deterministic quality string matching `seq` in length.

    The @lowqual record gets a low-quality 5' stretch and a low-quality 3'
    stretch so that -q/--nextseq-trim have something to trim; every other record
    gets uniform high quality.
    """
    if name == "@lowqual":
        n = len(seq)
        return "#" * 10 + "I" * max(0, n - 20) + "#" * min(10, max(0, n - 10))
    return "I" * len(seq)

CASAVA_HEADER = "@casava 1:1:FC:1:1:1:1 1:N:0:ACGT"


def normalise_records(records):
    """Accept (name, seq) or (name, seq, qual) and always yield valid qualities."""
    out = []
    for item in records:
        name, seq = item[0], item[1]
        qual = item[2] if len(item) > 2 else qualities_for(seq, name)
        if len(seq) != len(qual):
            raise ValueError(
                f"fixture {name}: sequence length {len(seq)} != quality length {len(qual)}"
            )
        out.append((name, seq, qual))
    return out


def write_fastq(path: Path, records, casava: bool = False) -> None:
    lines = []
    for name, seq, qual in normalise_records(records):
        header = CASAVA_HEADER if (casava and name == "@casava") else name
        lines.append(f"{header}\n{seq}\n+\n{qual}\n")
    path.write_text("".join(lines))


def write_fasta(path: Path, records) -> None:
    path.write_text("".join(f">{n[1:]}\n{s}\n" for n, s, _ in normalise_records(records)))


def make_paired(records):
    """Derive a deterministic R2 for every R1 record."""
    r2 = []
    for name, seq, qual in normalise_records(records):
        mate = seq[::-1].translate(str.maketrans("ACGTNacgtn", "TGCANtgcan"))
        r2.append((name, mate[: max(5, len(mate) - 5)], "I" * max(5, len(mate) - 5)))
    return r2


def random_records(rng: random.Random, count: int, adapter: str):
    records = []
    for i in range(count):
        length = rng.randint(20, 90)
        seq = "".join(rng.choice("ACGT") for _ in range(length))
        kind = i % 5
        if kind == 0:      # full adapter at the end
            seq = seq[: max(0, length - len(adapter))] + adapter
        elif kind == 1:    # partial adapter at the end
            keep = rng.randint(3, len(adapter) - 1)
            seq = seq[: max(0, length - keep)] + adapter[:keep]
        elif kind == 2:    # internal adapter
            pos = rng.randint(0, max(0, len(seq) - len(adapter)))
            seq = seq[:pos] + adapter + seq[pos:]
        elif kind == 3:    # one mismatch in a full adapter
            if len(seq) >= len(adapter):
                pos = len(seq) - len(adapter)
                mutated = list(adapter)
                idx = rng.randrange(len(mutated))
                mutated[idx] = rng.choice([b for b in "ACGT" if b != mutated[idx]])
                seq = seq[:pos] + "".join(mutated)
        qual = "".join(
            chr(rng.randint(33, 74)) if rng.random() < 0.85 else chr(rng.randint(40, 74))
            for _ in range(len(seq))
        )
        records.append((f"@r{i}", seq, qual))
    return records


# --------------------------------------------------------------------------
# Normalisation helpers
# --------------------------------------------------------------------------

def read_maybe_gzip(path: Path) -> bytes:
    """Return the DECOMPRESSED bytes of a file in any supported format.

    Compressed bytes legitimately differ between implementations (different
    deflate/bzip2/xz encoders), so the harness always compares decompressed
    content, as cutadapt's own documentation does.
    """
    if not path.exists():
        return b""
    data = path.read_bytes()
    if data[:2] == b"\x1f\x8b":
        try:
            return gzip.decompress(data)
        except OSError:
            return data
    if data[:3] == b"BZh":
        import bz2 as _bz2

        try:
            return _bz2.decompress(data)
        except OSError:
            return data
    if data[:6] == b"\xfd7zXZ\x00":
        import lzma as _lzma

        try:
            return _lzma.decompress(data)
        except OSError:
            return data
    return data


TIMING = re.compile(r"in [0-9.]+ ?(?:s|ms|µs|us|min).*$", re.M)
VERSION_LINE = re.compile(r"This is cutadapt [0-9.]+ with Python [0-9.]+")
CMD_LINE = re.compile(r"^Command line parameters:.*$", re.M)
PATH_TOKEN = re.compile(r"(?<![\w/])(?:\.{0,2}/)?(?:[\w.\-]+/)*[\w.\-]+\.(?:fastq|fq|fasta|fa|txt|json|info|rest|wildcards)(?:\.gz)?")
RATE_LINE = re.compile(r"^\s*[0-9.]+%?/read.*$", re.M)
PROCESSING_CORES = re.compile(r"Processing .* on \d+ core.*$", re.M)


def normalise_report(text: str) -> str:
    """Strip inherently machine-dependent parts of cutadapt's text report."""
    text = VERSION_LINE.sub("This is cutadapt <VERSION>", text)
    text = CMD_LINE.sub("Command line parameters: <CMD>", text)
    text = PROCESSING_CORES.sub("Processing <N> reads on <C> cores ...", text)
    text = TIMING.sub("in <TIME>", text)
    text = RATE_LINE.sub("<RATE>", text)
    text = PATH_TOKEN.sub("<PATH>", text)
    return text


JSON_IGNORED_KEYS = {
    "python_version",
    "command_line_arguments",
    "cores",
    "tag",
}


def normalise_json(obj):
    """Drop fields that legitimately differ between implementations/runs."""
    if isinstance(obj, dict):
        return {
            k: normalise_json(v)
            for k, v in obj.items()
            if k not in JSON_IGNORED_KEYS
        }
    if isinstance(obj, list):
        return [normalise_json(v) for v in obj]
    return obj


def json_diff(expected, actual, path="") -> list[str]:
    """Return a list of human-readable differences between two JSON values."""
    diffs: list[str] = []
    if type(expected) is not type(actual):
        # ints and floats are comparable
        if isinstance(expected, (int, float)) and isinstance(actual, (int, float)):
            if expected != actual:
                diffs.append(f"{path}: {expected!r} != {actual!r}")
            return diffs
        return [f"{path}: type {type(expected).__name__} != {type(actual).__name__}"]
    if isinstance(expected, dict):
        for key in sorted(set(expected) | set(actual)):
            if key not in expected:
                diffs.append(f"{path}.{key}: missing in oracle")
            elif key not in actual:
                diffs.append(f"{path}.{key}: missing in rust")
            else:
                diffs.extend(json_diff(expected[key], actual[key], f"{path}.{key}"))
    elif isinstance(expected, list):
        if len(expected) != len(actual):
            diffs.append(f"{path}: length {len(expected)} != {len(actual)}")
        for i, (a, b) in enumerate(zip(expected, actual)):
            diffs.extend(json_diff(a, b, f"{path}[{i}]"))
    elif isinstance(expected, float) and isinstance(actual, (int, float)):
        if abs(expected - actual) > 1e-9:
            diffs.append(f"{path}: {expected!r} != {actual!r}")
    elif expected != actual:
        diffs.append(f"{path}: {expected!r} != {actual!r}")
    return diffs


# --------------------------------------------------------------------------
# Case definitions
# --------------------------------------------------------------------------

# --------------------------------------------------------------------------
# Placeholder expansion
# --------------------------------------------------------------------------
# Case arguments use @@name@@ placeholders. str.format would clash with
# cutadapt's own braced templates (--rename '{id}', -x '{name}'), so expansion
# is explicit and leaves every other brace untouched.

PLACEHOLDERS = (
    "in", "r2", "missing", "out", "out_gz", "out2", "info", "rest",
    "wildcards", "too_short", "too_long", "untrimmed", "json",
    "adapters_fa", "adapters_messy",
    "demux_o", "demux_u", "demux_d", "demux_p1", "demux_p2",
    "in_bz2", "in_xz", "in_gz_unlabelled", "out_bz2", "out_xz", "out_gz",
)
PLACEHOLDER_RE = re.compile(r"@@([a-z_0-9]+)@@")


def expand(token: str, paths: dict[str, str]) -> str:
    return PLACEHOLDER_RE.sub(lambda m: paths.get(m.group(1), m.group(0)), token)


@dataclasses.dataclass
class Case:
    name: str
    args: list[str]              # arguments, with @@in@@, @@out@@, @@r2@@ placeholders
    category: str = "single-end"
    compare_report: bool = True  # compare the text report
    compare_json: bool = False   # compare --json output
    needs_r2: bool = False
    exit_code: int = 0           # expected oracle exit code


def cases() -> list[Case]:
    A = ADAPTER3
    F = ADAPTER5
    out = "@@out@@"
    # Most cases pass an explicit -m 0: cutadapt defaults it to 0 while the
    # v0.1.0 binary requires it. That gap is measured by the dedicated case
    # `default_minimum_length` instead of masking every other case.
    base = ["-m", "0", "-o", out]
    return [
        # ---- default values (compatibility of defaults) ----------------------
        Case("default_minimum_length", ["-a", A, "-o", out, "@@in@@"], compare_report=False),
        # ---- adapter types (single-end) -------------------------------------
        Case("a3_regular",        ["-a", A, *base, "@@in@@"]),
        Case("a3_anchored",       ["-a", A + "$", *base, "@@in@@"]),
        Case("g5_regular",        ["-g", F, *base, "@@in@@"]),
        Case("g5_anchored",       ["-g", "^" + F, *base, "@@in@@"]),
        Case("b_anywhere",        ["-b", A, *base, "@@in@@"]),
        Case("a3_no_indels",      ["-a", A, "--no-indels", *base, "@@in@@"]),
        Case("a3_error_rate_0",   ["-a", A, "-e", "0", *base, "@@in@@"]),
        Case("a3_error_rate_int", ["-a", A, "-e", "1", *base, "@@in@@"]),
        Case("a3_error_rate_02",  ["-a", A, "-e", "0.2", *base, "@@in@@"]),
        Case("a3_overlap_1",      ["-a", A, "-O", "1", *base, "@@in@@"]),
        Case("a3_overlap_10",     ["-a", A, "-O", "10", *base, "@@in@@"]),
        Case("a3_times_2",        ["-a", A, "-n", "2", *base, "@@in@@"]),
        Case("a3_times_3",        ["-a", A, "-n", "3", *base, "@@in@@"]),
        Case("multiple_adapters", ["-a", A, "-a", F, *base, "@@in@@"]),
        Case("anchored_and_regular", ["-a", A + "$", "-g", F, *base, "@@in@@"]),
        Case("linked_adapter",    ["-a", "^" + F + "..." + A, *base, "@@in@@"]),
        Case("wildcard_adapter",  ["-a", "AGATCGGAAGAGCN", *base, "@@in@@"]),
        Case("no_match_wildcards", ["-a", "AGATCGGAAGAGCN", "-N", *base, "@@in@@"]),
        Case("match_read_wildcards", ["-a", A, "--match-read-wildcards", *base, "@@in@@"]),
        Case("revcomp",           ["-a", A, "--rc", *base, "@@in@@"]),
        # ---- file:FILE adapter lists ----------------------------------------
        Case("file_adapter_list",      ["-a", "file:@@adapters_fa@@", *base, "@@in@@"]),
        Case("file_adapter_list_names", ["-a", "file:@@adapters_fa@@", "-x", "{name}_", *base, "@@in@@"]),
        Case("file_adapter_list_messy", ["-a", "file:@@adapters_messy@@", *base, "@@in@@"]),
        Case("file_adapter_list_front", ["-g", "file:@@adapters_fa@@", *base, "@@in@@"]),
        Case("file_adapter_list_paired",
             ["-a", A, "-A", "file:@@adapters_fa@@", "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True),
        Case("file_adapter_missing",   ["-a", "file:@@missing@@.fa", "-o", "@@out@@", "@@in@@"],
             exit_code=1, compare_report=False),
        # ---- documented validation rules ------------------------------------
        Case("b_linked_rejected", ["-b", f"^{F}...{A}", "-o", "@@out@@", "@@in@@"],
             exit_code=2, compare_report=False),
        Case("overlap_zero_rejected", ["-a", A, "-O", "0", "-o", "@@out@@", "@@in@@"],
             exit_code=2, compare_report=False),
        Case("times_zero", ["-a", A, "-n", "0", *base, "@@in@@"]),
        # ---- per-mate lengths and interleaved output -------------------------
        Case("paired_min_length_r2",
             ["-a", A, "-A", F, "-m", "0:35", "-o", "@@out@@", "-p", "@@out2@@",
              "@@in@@", "@@r2@@"], category="paired-end", needs_r2=True, compare_report=False),
        Case("paired_interleaved_output",
             ["-a", A, "-A", F, "--interleaved", "-m", "0", "-o", "@@out@@",
              "@@in@@", "@@r2@@"], category="paired-end", needs_r2=True, compare_report=False),
        Case("paired_max_length_r2",
             ["-a", A, "-A", F, "-M", "60:45", "-m", "0", "-o", "@@out@@", "-p", "@@out2@@",
              "@@in@@", "@@r2@@"], category="paired-end", needs_r2=True, compare_report=False),
        # ---- paired auxiliary outputs and pair filters ----------------------
        Case("paired_untrimmed_aux",
             ["-a", A, "-A", F, "--untrimmed-output", "@@untrimmed@@",
              "--untrimmed-paired-output", "@@too_short@@", "-m", "0",
              "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        Case("paired_too_short_aux",
             ["-a", A, "-A", F, "--too-short-output", "@@too_short@@",
              "--too-short-paired-output", "@@too_long@@", "-m", "30",
              "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        Case("paired_aux_mismatched",
             ["-a", A, "-A", F, "--untrimmed-paired-output", "@@too_short@@", "-m", "0",
              "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, exit_code=2, compare_report=False),
        Case("pair_filter_any",
             ["-a", A, "-A", F, "--pair-filter", "any", "-m", "30",
              "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        Case("pair_filter_both",
             ["-a", A, "-A", F, "--pair-filter", "both", "-m", "30",
              "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        Case("pair_filter_first",
             ["-a", A, "-A", F, "--pair-filter", "first", "-m", "30",
              "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        # ---- pair-adapters and debug ----------------------------------------
        Case("pair_adapters",
             ["-a", A, "-A", F, "--pair-adapters", "-m", "0", "-o", "@@out@@", "-p", "@@out2@@",
              "@@in@@", "@@r2@@"], category="paired-end", needs_r2=True),
        Case("pair_adapters_min",
             ["-a", A, "-A", F, "--pair-adapters", "-m", "30", "-o", "@@out@@", "-p", "@@out2@@",
              "@@in@@", "@@r2@@"], category="paired-end", needs_r2=True, compare_report=False),
        # ---- bzip2 / xz ------------------------------------------------------
        Case("bzip2_input",           ["-a", A, "-m", "0", "-o", "@@out@@", "@@in_bz2@@"]),
        Case("xz_input",              ["-a", A, "-m", "0", "-o", "@@out@@", "@@in_xz@@"]),
        Case("bzip2_output",          ["-a", A, "-m", "0", "-o", "@@out_bz2@@", "@@in@@"]),
        Case("xz_output",             ["-a", A, "-m", "0", "-o", "@@out_xz@@", "@@in@@"]),
        Case("gzip_output",           ["-a", A, "-m", "0", "-o", "@@out_gz@@", "@@in@@"]),
        Case("bzip2_output_fast",     ["-a", A, "-m", "0", "-Z", "-o", "@@out_bz2@@", "@@in@@"]),
        Case("xz_output_fast",        ["-a", A, "-m", "0", "-Z", "-o", "@@out_xz@@", "@@in@@"]),
        Case("magic_bytes_unlabelled", ["-a", A, "-m", "0", "-o", "@@out@@", "@@in_gz_unlabelled@@"]),
        Case("bzip2_to_xz",           ["-a", A, "-m", "0", "-o", "@@out_xz@@", "@@in_bz2@@"]),
        Case("xz_paired",
             ["-a", A, "-A", F, "-m", "0", "-o", "@@out_xz@@", "-p", "@@out_bz2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        # ---- inline named adapters (name=SEQ) -------------------------------
        Case("named_adapter",          ["-a", f"ad1={A}", *base, "@@in@@"]),
        Case("named_adapter_anchored", ["-a", f"an={A}$", *base, "@@in@@"]),
        Case("named_adapter_linked",   ["-a", f"ln=^{F}...{A}", *base, "@@in@@"]),
        Case("named_adapter_front",    ["-g", f"fr={F}", *base, "@@in@@"]),
        Case("named_adapter_anywhere", ["-b", f"an={A}", *base, "@@in@@"]),
        Case("named_adapters_multiple", ["-a", f"n1={A}", "-a", f"n2={F}", *base, "@@in@@"]),
        Case("named_adapter_empty_name", ["-a", f"={A}", *base, "@@in@@"]),
        Case("named_adapter_in_prefix", ["-a", f"ad1={A}", "-x", "{name}_", *base, "@@in@@"]),
        Case("named_adapter_in_rename", ["-a", f"ad1={A}", "--rename", "{id}_{adapter_name}", *base, "@@in@@"]),
        Case("demux_named",           ["-a", f"n1={A}", "-a", f"n2={F}", "-m", "0",
                                       "-o", "@@demux_o@@/o_{name}.fastq", "@@in@@"],
             compare_report=True),
        Case("demux_unnamed",         ["-a", A, "-m", "0", "-o", "@@demux_u@@/o_{name}.fastq", "@@in@@"],
             compare_report=False),
        Case("demux_discard_untrimmed", ["-a", f"n1={A}", "-m", "0", "--discard-untrimmed",
                                        "-o", "@@demux_d@@/o_{name}.fastq", "@@in@@"],
             compare_report=False),
        Case("demux_paired",          ["-a", f"n1={A}", "-a", f"n2={F}", "-m", "0",
                                       "-o", "@@demux_p1@@/r1_{name}.fastq",
                                       "-p", "@@demux_p2@@/r2_{name}.fastq", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True, compare_report=False),
        Case("named_adapter_paired",
             ["-a", f"A1={A}", "-A", f"B1={F}", "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True),
        # ---- actions --------------------------------------------------------
        Case("action_trim",       ["-a", A, "--action", "trim", *base, "@@in@@"]),
        Case("action_retain",     ["-a", A, "--action", "retain", *base, "@@in@@"]),
        Case("action_mask",       ["-a", A, "--action", "mask", *base, "@@in@@"]),
        Case("action_lowercase",  ["-a", A, "--action", "lowercase", *base, "@@in@@"]),
        Case("action_none",       ["-a", A, "--action", "none", *base, "@@in@@"]),
        # ---- read modifications --------------------------------------------
        Case("cut_5prime",        ["-a", A, "-u", "5", *base, "@@in@@"]),
        Case("cut_3prime",        ["-a", A, "-u", "-5", *base, "@@in@@"]),
        Case("cut_both",          ["-a", A, "-u", "3", "-u", "-3", *base, "@@in@@"]),
        Case("quality_3prime",    ["-a", A, "-q", "20", *base, "@@in@@"]),
        Case("quality_both",      ["-a", A, "-q", "20,20", *base, "@@in@@"]),
        Case("quality_base64",    ["-a", A, "-q", "20", "--quality-base", "64", *base, "@@in@@"]),
        Case("nextseq_trim",      ["-a", A, "--nextseq-trim", "20", *base, "@@in@@"]),
        Case("poly_a",            ["-a", A, "--poly-a", *base, "@@in@@"]),
        Case("length_shorten",    ["-a", A, "-l", "20", *base, "@@in@@"]),
        Case("length_negative",   ["-a", A, "-l", "-20", *base, "@@in@@"]),
        Case("trim_n",            ["-a", A, "--trim-n", *base, "@@in@@"]),
        Case("zero_cap",          ["-a", A, "-z", *base, "@@in@@"]),
        Case("strip_suffix",      ["-a", A, "--strip-suffix", ":1", *base, "@@in@@"]),
        Case("prefix",            ["-a", A, "-x", "PRE_", *base, "@@in@@"]),
        Case("suffix",            ["-a", A, "-y", "_SUF", *base, "@@in@@"]),
        Case("rename",            ["-a", A, "--rename", "{id}_{adapter_name}", *base, "@@in@@"]),
        Case("length_tag",        ["-a", A, "--length-tag", "length=", *base, "@@in@@"]),
        # ---- filters --------------------------------------------------------
        Case("filter_min",        ["-a", A, "-m", "30", *base, "@@in@@"]),
        Case("filter_max",        ["-a", A, "-M", "50", *base, "@@in@@"]),
        Case("filter_max_n",      ["-a", A, "--max-n", "2", *base, "@@in@@"]),
        Case("filter_max_ee",     ["-a", A, "--max-expected-errors", "2", *base, "@@in@@"]),
        Case("filter_max_aer",    ["-a", A, "--max-average-error-rate", "0.1", *base, "@@in@@"]),
        Case("discard_trimmed",   ["-a", A, "--discard-trimmed", *base, "@@in@@"]),
        Case("discard_untrimmed", ["-a", A, "--discard-untrimmed", *base, "@@in@@"]),
        Case("discard_casava",    ["-a", A, "--discard-casava", *base, "@@in@@"]),
        # ---- outputs --------------------------------------------------------
        Case("out_fasta",         ["-a", A, "--fasta", *base, "@@in@@"]),
        Case("out_gzip",          ["-a", A, "-o", "@@out_gz@@", "@@in@@"]),
        Case("out_stdout",        ["-a", A, "-m", "0", "@@in@@"], compare_report=False),
        Case("default_output",    ["-a", A, "-m", "0", "@@in@@"], compare_report=False),
        Case("info_file",         ["-a", A, "--info-file", "@@info@@", *base, "@@in@@"]),
        Case("rest_file",         ["-a", A, "-r", "@@rest@@", *base, "@@in@@"]),
        Case("wildcard_file",     ["-a", "AGATCGGAAGAGCN", "--wildcard-file", "@@wildcards@@", *base, "@@in@@"]),
        Case("too_short_output",  ["-a", A, "-m", "40", "--too-short-output", "@@too_short@@", *base, "@@in@@"]),
        Case("too_long_output",   ["-a", A, "-M", "40", "--too-long-output", "@@too_long@@", *base, "@@in@@"]),
        Case("untrimmed_output",  ["-a", A, "--untrimmed-output", "@@untrimmed@@", *base, "@@in@@"]),
        Case("report_minimal",    ["-a", A, "--report", "minimal", *base, "@@in@@"]),
        Case("json_report",       ["-a", A, "--json", "@@json@@", *base, "@@in@@"], compare_json=True),
        Case("json_report_full",  ["-a", A, "-m", "30", "-q", "20", "--json", "@@json@@", *base, "@@in@@"], compare_json=True),
        Case("cores_2",           ["-a", A, "-j", "2", *base, "@@in@@"]),
        Case("compression_z",     ["-a", A, "-Z", "-o", "@@out_gz@@", "@@in@@"]),
        Case("quiet",             ["-a", A, "--quiet", *base, "@@in@@"], compare_report=False),
        # ---- paired-end -----------------------------------------------------
        Case("paired_basic",      ["-a", A, "-A", F, "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True),
        Case("paired_interleaved", ["-a", A, "-A", F, "--interleaved", "-o", "@@out@@", "@@in@@"],
             category="paired-end", needs_r2=True),
        Case("paired_min",        ["-a", A, "-A", F, "-m", "30", "-o", "@@out@@", "-p", "@@out2@@", "@@in@@", "@@r2@@"],
             category="paired-end", needs_r2=True),
        # ---- failures (exit status parity) ----------------------------------
        Case("err_missing_input", ["-a", A, "-o", "@@out@@", "@@missing@@"], exit_code=1, compare_report=False),
        Case("err_unknown_option", ["--frobnicate", "-o", "@@out@@", "@@in@@"], exit_code=2, compare_report=False),
        Case("err_bad_adapter",   ["-a", "AXCT", "-o", "@@out@@", "@@in@@"], exit_code=2, compare_report=False),
        Case("err_both_stdin",    ["-a", A, "-o", "-", "-"], exit_code=2, compare_report=False),
        Case("err_no_adapter",    ["-o", "@@out@@", "@@in@@"], exit_code=2, compare_report=False),
    ]


# --------------------------------------------------------------------------
# Execution
# --------------------------------------------------------------------------

@dataclasses.dataclass
class Observation:
    exit_code: int
    stdout: bytes
    stderr: str
    files: dict[str, bytes]


def run_one(binary: Path, case: Case, workdir: Path, fixture_dir: Path, tag: str) -> Observation:
    paths = {
        "in_bz2": str(fixture_dir / "simple.fastq.bz2"),
        "in_xz": str(fixture_dir / "simple.fastq.xz"),
        "in_gz_unlabelled": str(fixture_dir / "compressed_but_unlabelled"),
        "out_bz2": str(workdir / f"{tag}.out.fastq.bz2"),
        "out_xz": str(workdir / f"{tag}.out.fastq.xz"),
        "out_gz": str(workdir / f"{tag}.out.fastq.gz"),
        "adapters_fa": str(fixture_dir / "adapters.fa"),
        "adapters_messy": str(fixture_dir / "adapters_messy.fa"),
        "in": str(fixture_dir / "simple.fastq"),
        "r2": str(fixture_dir / "simple.r2.fastq"),
        "missing": str(fixture_dir / "does-not-exist.fastq"),
        "out": str(workdir / f"{tag}.out.fastq"),
        "demux_o": str(workdir / f"{tag}_demux_o"),
        "demux_u": str(workdir / f"{tag}_demux_u"),
        "demux_d": str(workdir / f"{tag}_demux_d"),
        "demux_p1": str(workdir / f"{tag}_demux_p1"),
        "demux_p2": str(workdir / f"{tag}_demux_p2"),
        "out_gz": str(workdir / f"{tag}.out.fastq.gz"),
        "out2": str(workdir / f"{tag}.out2.fastq"),
        "info": str(workdir / f"{tag}.info"),
        "rest": str(workdir / f"{tag}.rest"),
        "wildcards": str(workdir / f"{tag}.wildcards"),
        "too_short": str(workdir / f"{tag}.too_short.fastq"),
        "too_long": str(workdir / f"{tag}.too_long.fastq"),
        "untrimmed": str(workdir / f"{tag}.untrimmed.fastq"),
        "json": str(workdir / f"{tag}.json"),
    }
    # Demultiplexing cases write into directories that must exist first.
    for key in ("demux_o", "demux_u", "demux_d", "demux_p1", "demux_p2"):
        Path(paths[key]).mkdir(parents=True, exist_ok=True)
    argv = [str(binary)]
    for token in case.args:
        argv.append(expand(token, paths))
    proc = subprocess.run(argv, capture_output=True)
    observed_files = {}
    for key in ("out", "out_gz", "out_bz2", "out_xz", "out2", "info", "rest",
                "wildcards", "too_short", "too_long", "untrimmed", "json"):
        p = Path(paths[key])
        if p.exists():
            observed_files[key] = read_maybe_gzip(p) if key != "json" else p.read_bytes()
    return Observation(
        exit_code=proc.returncode,
        stdout=proc.stdout,
        stderr=proc.stderr.decode("utf-8", "replace"),
        files=observed_files,
    )


# Cases whose `-o` template creates several files: every file in each side's
# directory is compared by name and content.
DEMUX_CASES = {
    "demux_named": ("demux_o", None),
    "demux_unnamed": ("demux_u", None),
    "demux_discard_untrimmed": ("demux_d", None),
    "demux_paired": ("demux_p1", "demux_p2"),
}


def compare_demux(oracle_dir: Path, rust_dir: Path) -> list[str]:
    """Compare every demultiplexed output file by name and content."""
    problems: list[str] = []
    oracle_files = sorted(p.name for p in oracle_dir.iterdir() if p.is_file())
    rust_files = sorted(p.name for p in rust_dir.iterdir() if p.is_file())
    if oracle_files != rust_files:
        problems.append(f"demux files differ: oracle={oracle_files} rust={rust_files}")
    for name in oracle_files:
        a = (oracle_dir / name).read_bytes()
        b_path = rust_dir / name
        if not b_path.exists():
            problems.append(f"demux {name}: missing in rust")
        elif a != b_path.read_bytes():
            problems.append(f"demux {name}: content differs ({len(a)} vs {len(b_path.read_bytes())} bytes)")
    return problems


def compare(case: Case, oracle: Observation, rust: Observation) -> list[str]:
    problems: list[str] = []
    if oracle.exit_code != rust.exit_code:
        problems.append(f"exit code {oracle.exit_code} != {rust.exit_code}")
        # If both failed, per-file comparison is not meaningful.
        if oracle.exit_code != 0:
            return problems
    # stdout: when reads go to stdout the report may precede them, so compare
    # only the FASTQ/FASTA payload (the report is checked separately).
    if case.name in ("out_stdout", "default_output"):
        o_reads = fastq_payload(oracle.stdout)
        r_reads = fastq_payload(rust.stdout)
        if o_reads != r_reads:
            problems.append(
                f"stdout reads differ ({len(o_reads)} vs {len(r_reads)} bytes)")
    elif case.compare_report:
        o = normalise_report(oracle.stdout.decode("utf-8", "replace"))
        r = normalise_report(rust.stdout.decode("utf-8", "replace"))
        if o != r:
            problems.append("text report differs: " + first_line_diff(o, r))
    # stderr parity: both empty or both non-empty
    if bool(oracle.stderr.strip()) != bool(rust.stderr.strip()):
        problems.append("stderr presence differs")
    # output files
    if case.compare_json:
        if "json" in oracle.files and "json" in rust.files:
            try:
                oj = normalise_json(json.loads(oracle.files["json"]))
                rj = normalise_json(json.loads(rust.files["json"]))
                diffs = json_diff(oj, rj)
                if diffs:
                    problems.append("json differs: " + "; ".join(diffs[:4]))
            except json.JSONDecodeError as exc:
                problems.append(f"json parse error: {exc}")
        elif "json" in oracle.files:
            problems.append("json: rust wrote no report")
    for key in ("out", "out_gz", "out_bz2", "out_xz", "out2", "info", "rest",
                "wildcards", "too_short", "too_long", "untrimmed"):
        o_has = key in oracle.files
        r_has = key in rust.files
        if o_has != r_has:
            problems.append(f"{key}: present in oracle={o_has}, rust={r_has}")
        elif o_has and oracle.files[key] != rust.files[key]:
            problems.append(
                f"{key}: content differs ({len(oracle.files[key])} vs {len(rust.files[key])} bytes)"
            )
    return problems


def fastq_payload(data: bytes) -> bytes:
    """Return only the FASTQ/FASTA portion of stdout.

    cutadapt writes its report before the reads when no -o is given. A read
    record starts with '@' (FASTQ) or '>' (FASTA) at the beginning of a line.
    """
    lines = data.splitlines(keepends=True)
    for i, line in enumerate(lines):
        if line[:1] in (b"@", b">"):
            return b"".join(lines[i:])
    return b""


def first_line_diff(a: str, b: str) -> str:
    al, bl = a.splitlines(), b.splitlines()
    for i, (x, y) in enumerate(zip(al, bl)):
        if x != y:
            return f"line {i + 1}: oracle={x!r} rust={y!r}"
    if len(al) != len(bl):
        return f"line count {len(al)} vs {len(bl)}"
    return "whitespace only"


# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------

def prepare_fixtures(base: Path) -> Path:
    fixture_dir = base / "fixtures"
    fixture_dir.mkdir(parents=True, exist_ok=True)
    write_fastq(fixture_dir / "simple.fastq", SINGLE_END_RECORDS, casava=True)
    write_fastq(fixture_dir / "simple.r2.fastq", make_paired(SINGLE_END_RECORDS))
    write_fasta(fixture_dir / "simple.fasta", SINGLE_END_RECORDS)
    rng = random.Random(20261018)
    write_fastq(fixture_dir / "random.fastq", random_records(rng, 200, ADAPTER3))
    write_fastq(fixture_dir / "random.r2.fastq", make_paired(random_records(rng, 200, ADAPTER3)))
    # Adapter lists for the `file:FILE` notation: one adapter per FASTA record.
    # The record name becomes the adapter name (report heading and {name}).
    (fixture_dir / "adapters.fa").write_text(
        ">ad1\n"
        f"{ADAPTER3}\n"
        ">ad2 some description\n"
        f"{ADAPTER5}\n"
    )
    # A second list with a trailing blank line and a comment, to exercise the
    # parser's line handling.
    (fixture_dir / "adapters_messy.fa").write_text(
        "; a comment line\n"
        f">first\n{ADAPTER3}\n"
        "\n"
        f">second\n{ADAPTER5}\n"
        "\n"
    )
    # Compressed copies of the same reads, for the bzip2/xz cases and for
    # magic-byte detection with a misleading extension.
    import bz2 as _bz2
    import lzma as _lzma

    plain = (fixture_dir / "simple.fastq").read_bytes()
    (fixture_dir / "simple.fastq.bz2").write_bytes(_bz2.compress(plain, 9))
    (fixture_dir / "simple.fastq.xz").write_bytes(_lzma.compress(plain))
    # A gzip stream whose name says nothing about compression.
    import gzip as _gzip

    (fixture_dir / "compressed_but_unlabelled").write_bytes(_gzip.compress(plain))
    return fixture_dir


def self_check() -> int:
    """Validate the harness machinery without needing the binaries."""
    ok = True
    text = "This is cutadapt 4.6 with Python 3.14.8\nFinished in 0.003 s (347.376 µs/read; 0.17 M reads/minute).\n"
    norm = normalise_report(text)
    if "4.6" in norm or "347" in norm:
        print("SELF-CHECK FAIL: report normalisation did not strip version/timing")
        ok = False
    if json_diff({"a": 1}, {"a": 1}):
        print("SELF-CHECK FAIL: identical JSON reported as different")
        ok = False
    if not json_diff({"a": 1}, {"a": 2}):
        print("SELF-CHECK FAIL: different JSON reported as identical")
        ok = False
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        fx = prepare_fixtures(base)
        names = {p.name for p in fx.iterdir()}
        expected = {"simple.fastq", "simple.r2.fastq", "simple.fasta", "random.fastq", "random.r2.fastq"}
        if not expected <= names:
            print(f"SELF-CHECK FAIL: fixtures missing: {expected - names}")
            ok = False
        # every generated FASTQ must have 4-line records with matching lengths
        for fq in sorted(fx.glob("*.fastq")):
            lines = fq.read_text().splitlines()
            if len(lines) % 4 != 0:
                print(f"SELF-CHECK FAIL: {fq.name} has {len(lines)} lines (not a multiple of 4)")
                ok = False
                continue
            for i in range(0, len(lines), 4):
                if len(lines[i + 1]) != len(lines[i + 3]):
                    print(f"SELF-CHECK FAIL: {fq.name} record {i // 4} has "
                          f"seq {len(lines[i + 1])} vs qual {len(lines[i + 3])}")
                    ok = False
                    break
        # every case's placeholders must resolve
        paths = {k: "/x/" + k for k in PLACEHOLDERS}
        for case in cases():
            for token in case.args:
                for name in PLACEHOLDER_RE.findall(token):
                    if name not in paths:
                        print(f"SELF-CHECK FAIL: case {case.name} has unknown placeholder {name}")
                        ok = False
                if "@@" in expand(token, paths):
                    print(f"SELF-CHECK FAIL: case {case.name} left an unexpanded placeholder")
                    ok = False
    print("self-check:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--oracle", help="path to the reference cutadapt binary")
    parser.add_argument("--rust", help="path to the Rust binary")
    parser.add_argument("--filter", default="", help="only run cases whose name contains this")
    parser.add_argument("--category", default="", help="only run cases in this category")
    parser.add_argument("--list", action="store_true", help="list case names and exit")
    parser.add_argument("--self-check", action="store_true", help="run harness self-check only")
    parser.add_argument("--matrix", action="store_true", help="print a compact matrix")
    parser.add_argument("--json", help="write machine-readable results to this file")
    parser.add_argument("--verbose", "-v", action="store_true", help="print per-case details")
    parser.add_argument("--debug-argv", action="store_true",
                        help="print the exact oracle/rust commands and their stderr")
    args = parser.parse_args(argv)

    if args.self_check:
        return self_check()

    selected = [c for c in cases()
                if args.filter in c.name and (not args.category or c.category == args.category)]
    if args.list:
        for case in selected:
            print(f"{case.category:12s} {case.name}")
        return 0

    oracle_bin = find_oracle(args.oracle)
    rust_bin = find_rust(args.rust)
    missing = []
    if oracle_bin is None:
        missing.append("cutadapt 4.6 oracle (set CUTADAPT_ORACLE or --oracle)")
    if rust_bin is None:
        missing.append("rust-cutadapt binary (build with `cargo build --release` or set RUST_CUTADAPT)")
    if missing:
        print("differential: cannot run, missing:", file=sys.stderr)
        for m in missing:
            print("  -", m, file=sys.stderr)
        return 3

    print(f"oracle: {oracle_bin}")
    print(f"rust  : {rust_bin}")
    version = subprocess.run([str(oracle_bin), "--version"], capture_output=True, text=True)
    print(f"oracle version: {version.stdout.strip() or version.stderr.strip()}")
    versions = subprocess.run([str(rust_bin), "--version"], capture_output=True, text=True)
    print(f"rust version  : {versions.stdout.strip() or versions.stderr.strip()}")
    print(f"cases: {len(selected)}\n")

    results = []
    with tempfile.TemporaryDirectory(prefix="cutadapt-diff-") as tmp:
        base = Path(tmp)
        fixture_dir = prepare_fixtures(base)
        for case in selected:
            case_dir = base / case.name
            (case_dir / "oracle").mkdir(parents=True, exist_ok=True)
            (case_dir / "rust").mkdir(parents=True, exist_ok=True)
            o = run_one(oracle_bin, case, case_dir / "oracle", fixture_dir, "out")
            r = run_one(rust_bin, case, case_dir / "rust", fixture_dir, "out")
            if args.debug_argv:
                print(f"  oracle exit={o.exit_code} stderr={o.stderr[:300]!r}")
                print(f"  rust   exit={r.exit_code} stderr={r.stderr[:300]!r}")
            problems = compare(case, o, r)
            if case.name in DEMUX_CASES:
                key1, key2 = DEMUX_CASES[case.name]
                problems.extend(
                    compare_demux(
                        case_dir / "oracle" / f"out_{key1}",
                        case_dir / "rust" / f"out_{key1}",
                    )
                )
                if key2:
                    problems.extend(
                        compare_demux(
                            case_dir / "oracle" / f"out_{key2}",
                            case_dir / "rust" / f"out_{key2}",
                        )
                    )
            results.append({"name": case.name, "category": case.category,
                            "passed": not problems, "problems": problems})
            status = "PASS" if not problems else "FAIL"
            if args.matrix:
                print(f"{status}  {case.category:11s} {case.name}")
            else:
                print(f"{status}  {case.name}")
            if problems and (args.verbose or not args.matrix):
                for p in problems[:6]:
                    print(f"        {p}")

    passed = sum(1 for r in results if r["passed"])
    failed = [r for r in results if not r["passed"]]
    print(f"\n{passed}/{len(results)} cases passed")
    if failed:
        by_cat: dict[str, int] = {}
        for r in failed:
            by_cat[r["category"]] = by_cat.get(r["category"], 0) + 1
        print("failures by category: " + ", ".join(f"{k}={v}" for k, v in sorted(by_cat.items())))
        print("failing cases: " + ", ".join(r["name"] for r in failed))
    if args.json:
        Path(args.json).write_text(json.dumps(
            {"oracle": str(oracle_bin), "rust": str(rust_bin), "results": results}, indent=2))
        print(f"wrote {args.json}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
