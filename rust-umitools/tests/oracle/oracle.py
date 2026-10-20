#!/usr/bin/env python3
"""Oracle comparison for rust-umitools against UMI-tools 1.1.6.

Every check runs both implementations on the same synthetic input and compares
the observable result: group membership, representative UMI, cluster counts and
the output records. Nothing here inspects rust-umitools internals, so a passing
run is evidence about behaviour, not about implementation shape.

Usage:  bash tests/oracle/run_oracle.sh
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
# tests/ci/env.sh points the build at a writable target directory because the
# check sandbox mounts the source tree read-only.
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "rust-umitools"

PASSED: list[str] = []
FAILED: list[tuple[str, str]] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    if ok:
        PASSED.append(name)
        print(f"PASS  {name}")
    else:
        FAILED.append((name, detail))
        print(f"FAIL  {name}\n        {detail}")


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, **kwargs)


def umi(args: list[str]) -> subprocess.CompletedProcess:
    return run(["umi_tools", *args])


def rust(args: list[str]) -> subprocess.CompletedProcess:
    return run([str(BINARY), *args])


# --------------------------------------------------------------------------- #
# fixture construction
# --------------------------------------------------------------------------- #

HEADER = "@HD\tVN:1.6\tSO:coordinate\n@SQ\tSN:chr1\tLN:1000000\n@SQ\tSN:chr2\tLN:1000000\n"


def record(
    name: str,
    flag: int,
    pos: int,
    umi: str,
    mapq: int = 60,
    cigar: str = "10M",
    rname: str = "chr1",
    extra: str = "",
    seq: str = "A" * 10,
) -> str:
    qual = "I" * len(seq)
    return f"{name}_{umi}\t{flag}\t{rname}\t{pos}\t{mapq}\t{cigar}\t*\t0\t0\t{seq}\t{qual}{extra}\n"


def write_bam(directory: Path, name: str, sam_body: str) -> Path:
    sam = directory / f"{name}.sam"
    bam = directory / f"{name}.bam"
    sam.write_text(sam_body)
    result = run(["samtools", "sort", "-o", str(bam), str(sam)])
    if result.returncode != 0:
        raise RuntimeError(f"samtools sort failed for {name}: {result.stderr}")
    # umi_tools dedup calls fetch() without a region, which needs an index.
    indexed = run(["samtools", "index", str(bam)])
    if indexed.returncode != 0:
        raise RuntimeError(f"samtools index failed for {name}: {indexed.stderr}")
    return bam


def families_sam(families) -> str:
    """families: [(pos, reverse, [(umi, count, mapq), ...]), ...]"""
    rows = []
    for pos, reverse, members in families:
        flag = 16 if reverse else 0
        for umi, count, mapq in members:
            for _ in range(count):
                rows.append(record(f"r{len(rows)}", flag, pos, umi, mapq))
    return HEADER + "".join(rows)


# --------------------------------------------------------------------------- #
# comparison helpers
# --------------------------------------------------------------------------- #

def records(path: Path) -> set[tuple[str, ...]]:
    out = run(["samtools", "view", str(path)])
    if out.returncode != 0:
        raise RuntimeError(out.stderr)
    result = set()
    for line in out.stdout.splitlines():
        f = line.split("\t")
        result.add((f[0], f[1], f[2], f[3], f[4], f[5], f[9], f[10]))
    return result


def record_list(path: Path) -> list[tuple[str, ...]]:
    out = run(["samtools", "view", str(path)])
    return [tuple(line.split("\t")[:6]) for line in out.stdout.splitlines()]


def safe_records(path: Path):
    """Like `records`, but returns None when a step refused to produce output."""
    if not Path(path).exists():
        return None
    try:
        return records(path)
    except RuntimeError:
        return None


def group_table(tsv: Path) -> set[tuple[str, int, frozenset[str]]]:
    """Independent comparison of the grouping layer: (top UMI, group count, members)."""
    text = tsv.read_text().splitlines()
    if not text:
        return set()
    header = text[0].split("\t")
    index = {name: position for position, name in enumerate(header)}
    groups: dict[str, dict] = {}
    for line in text[1:]:
        fields = line.split("\t")
        key = fields[index["unique_id"]]
        entry = groups.setdefault(
            key,
            {
                "top": fields[index["final_umi"]],
                "count": int(fields[index["final_umi_count"]]),
                "members": set(),
            },
        )
        entry["members"].add(fields[index["umi"]])
    return {(g["top"], g["count"], frozenset(g["members"])) for g in groups.values()}


def read_pairs(path: Path) -> tuple[list, list]:
    """Sorted-by-key record tuples and a per-record mate-position check."""
    out = run(["samtools", "view", str(path)]).stdout.splitlines()
    rows = [line.split("\t") for line in out]
    keys = [(row[2], int(row[3]), row[1]) for row in rows]
    return rows, keys


def is_coordinate_sorted(path: Path) -> bool:
    out = run(["samtools", "view", str(path)]).stdout.splitlines()
    seen = []
    for line in out:
        f = line.split("\t")
        tid = 0 if f[2] == "chr1" else 1
        seen.append((tid, int(f[3])))
    return all(seen[i] <= seen[i + 1] for i in range(len(seen) - 1))


def m5(path: Path) -> str:
    return run(["samtools", "view", str(path)]).stdout


# --------------------------------------------------------------------------- #
# checks
# --------------------------------------------------------------------------- #

def check_b01(work: Path) -> None:
    """9 nt UMIs differing at the first base must merge (regression B01)."""
    body = families_sam([(100, False, [("AAAAAAAAA", 1, 60), ("CAAAAAAAA", 1, 60)])])
    bam = write_bam(work, "b01", body)
    reference = work / "b01_umi.bam"
    umi(["dedup", "-I", str(bam), "-S", str(reference), "--method=directional"])
    subject = work / "b01_rust.bam"
    result = rust(["dedup", "-I", str(bam), "-S", str(subject), "--method=directional"])
    check("B01 9nt UMI merges", result.returncode == 0 and len(records(subject)) == 1,
          f"exit={result.returncode} stderr={result.stderr.strip()[:200]}")
    check("B01 matches umi_tools", safe_records(subject) is not None and safe_records(subject) == safe_records(reference),
          f"rust={safe_records(subject) and len(safe_records(subject))} umi={safe_records(reference) and len(safe_records(reference))}")


def check_b02(work: Path) -> None:
    """Mixed UMI lengths inside one bundle: upstream aborts, we must fail cleanly."""
    body = families_sam([(200, False, [("A", 1, 60), ("AA", 1, 60)])])
    bam = write_bam(work, "b02", body)
    reference = umi(["dedup", "-I", str(bam), "-S", str(work / "b02_umi.bam")])
    subject = rust(["dedup", "-I", str(bam), "-S", str(work / "b02_rust.bam")])
    check("B02 umi_tools also rejects mixed lengths", reference.returncode != 0,
          f"umi_tools exit={reference.returncode}")
    check("B02 rust exits 3 with a clear message",
          subject.returncode == 3 and "lengths" in subject.stderr,
          f"exit={subject.returncode} stderr={subject.stderr.strip()[:200]}")
    check("B02 leaves no output file", not (work / "b02_rust.bam").exists(),
          "partial output was committed")


def check_b03(work: Path) -> None:
    """A trailing unmapped read must not be reported as an ordering error."""
    body = HEADER + record("r0", 0, 300, "AAAAAAAA") \
        + "r1_CCCCCCCC\t4\t*\t0\t0\t*\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\n"
    bam = write_bam(work, "b03", body)
    reference = work / "b03_umi.bam"
    umi(["dedup", "-I", str(bam), "-S", str(reference)])
    subject = work / "b03_rust.bam"
    result = rust(["dedup", "-I", str(bam), "-S", str(subject)])
    check("B03 unmapped tail is accepted", result.returncode == 0,
          f"exit={result.returncode} stderr={result.stderr.strip()[:200]}")
    check("B03 matches umi_tools",
          safe_records(subject) is not None and safe_records(subject) == safe_records(reference),
          f"rust={safe_records(subject) and len(safe_records(subject))} "
          f"umi={safe_records(reference) and len(safe_records(reference))}")


def check_b04(work: Path) -> None:
    """Soft clipped reads can reorder positions; the default output must be sorted."""
    body = HEADER + record("r0", 0, 401, "AAAAAAAA") + record("r1", 0, 402, "CCCCCCCC", cigar="5S5M")
    bam = write_bam(work, "b04", body)
    unsorted = work / "b04_unsorted.bam"
    rust(["dedup", "-I", str(bam), "-S", str(unsorted), "--no-sort-output"])
    subject = work / "b04_rust.bam"
    result = rust(["dedup", "-I", str(bam), "-S", str(subject)])
    check("B04 default output is coordinate sorted",
          result.returncode == 0 and is_coordinate_sorted(subject),
          f"exit={result.returncode}")
    check("B04 --no-sort-output keeps processing order",
          not is_coordinate_sorted(unsorted),
          "processing order already happened to be sorted")


METHODS = ["unique", "percentile", "cluster", "adjacency", "directional"]


def check_grouping(work: Path) -> None:
    """Grouping layer comparison across every method and two thresholds (C2)."""
    rng_state = 12345

    def next_umi() -> str:
        nonlocal rng_state
        alphabet = "ACGT"
        rng_state = (rng_state * 1103515245 + 12345) % (2**31)
        return "".join(alphabet[(rng_state >> (3 * i)) & 3] for i in range(8))

    families = []
    for block in range(12):
        parent = next_umi()
        members = [(parent, 6, 60)]
        for offset in range(1, 4):
            child = list(parent)
            child[offset] = "ACGT"[(offset + block) % 4]
            members.append(("".join(child), max(1, 3 - offset), 60))
        families.append((1000 + block * 50, block % 2 == 1, members))
    # A distance two chain and an equal count pair to stress the directional rule.
    families.append((2000, False, [("AAAAACGT", 5, 60), ("AAAAACGA", 1, 60), ("AAAAACCA", 1, 60)]))
    families.append((2100, False, [("GGGGTTTT", 4, 60), ("GGGGTTTA", 4, 60)]))
    bam = write_bam(work, "grouping", families_sam(families))

    for method in METHODS:
        for threshold in (1, 2):
            ref = work / f"grp_{method}_{threshold}_umi.tsv"
            sub = work / f"grp_{method}_{threshold}_rust.tsv"
            reference = umi([
                "group", "-I", str(bam), "--group-out", str(ref),
                "--method", method, "--edit-distance-threshold", str(threshold),
                "--no-sort-output",
            ])
            subject = rust([
                "group", "-I", str(bam), "--group-out", str(sub),
                "--method", method, "--edit-distance-threshold", str(threshold),
            ])
            if reference.returncode != 0 or subject.returncode != 0:
                check(f"C2 group {method} k={threshold}", False,
                      f"umi exit={reference.returncode} rust exit={subject.returncode} "
                      f"rust_stderr={subject.stderr.strip()[:160]}")
                continue
            expected = group_table(ref)
            got = group_table(sub)
            check(f"C2 group {method} k={threshold}", expected == got,
                  f"groups differ: expected {len(expected)} got {len(got)}; "
                  f"missing={sorted(expected - got)[:2]} extra={sorted(got - expected)[:2]}")


def check_dedup_records(work: Path) -> None:
    """Record layer comparison with the upstream RNG strategy (C3)."""
    rng_state = 999

    def next_umi() -> str:
        nonlocal rng_state
        rng_state = (rng_state * 1103515245 + 12345) % (2**31)
        return "".join("ACGT"[(rng_state >> (2 * i)) & 3] for i in range(8))

    families = []
    for block in range(20):
        parent = next_umi()
        members = [(parent, 4, 60)]
        # Equal MAPQ within a UMI forces the reservoir draw we replicate.
        for offset in range(1, 3):
            child = list(parent)
            child[offset] = "ACGT"[(offset + block) % 4]
            members.append(("".join(child), 2, 60))
        families.append((500 + block * 25, block % 2 == 1, members))
    bam = write_bam(work, "dedup", families_sam(families))

    reference = work / "dedup_umi.bam"
    subject = work / "dedup_rust.bam"
    umi(["dedup", "-I", str(bam), "-S", str(reference), "--random-seed", "1"])
    result = rust(["dedup", "-I", str(bam), "-S", str(subject), "--random-seed", "1"])
    got = safe_records(subject)
    expected = safe_records(reference)
    check(
        "C3 fixed seed record match",
        result.returncode == 0 and got is not None and got == expected,
        f"exit={result.returncode} rust={got and len(got)} umi={expected and len(expected)} "
        f"rust_stderr={result.stderr.strip()[:200]}",
    )

    stable = work / "dedup_stable.bam"
    rust(["dedup", "-I", str(bam), "-S", str(stable), "--representative", "stable", "--random-seed", "1"])
    stable_got = safe_records(stable)
    umis_seen = (
        {row[0].split("_")[-1] for row in stable_got} if stable_got else set()
    )
    reference_umis = {row[0].split("_")[-1] for row in expected} if expected else set()
    check("C3 stable strategy keeps the molecule set",
          bool(stable_got) and umis_seen == reference_umis,
          f"stable={sorted(umis_seen)[:4]} reference={sorted(reference_umis)[:4]}")

    tsv = work / "group_tab.tsv"
    rust(["group", "-I", str(bam), "--group-out", str(tsv)])
    reference_tsv = work / "group_tab_umi.tsv"
    umi(["group", "-I", str(bam), "--group-out", str(reference_tsv), "--no-sort-output"])
    check("C3 group membership matches",
          group_table(tsv) == group_table(reference_tsv),
          f"expected {len(group_table(reference_tsv))} groups, got {len(group_table(tsv))}")


def check_determinism(work: Path) -> None:
    """Same input, different thread counts, identical grouping and records (C10)."""
    out = []
    for threads in (1, 2, 8):
        path = work / f"det_{threads}.bam"
        result = rust(["dedup", "-I", str(work / "dedup.bam"), "-S", str(path), "--threads", str(threads)])
        if result.returncode != 0:
            check(f"C10 threads={threads}", False, result.stderr.strip()[:200])
            return
        out.append(m5(path))
    check("C10 identical across thread counts", len(set(out)) == 1,
          "outputs differ between thread counts")


def check_paired(work: Path) -> None:
    """Paired-end mate recovery (C8)."""
    rows = []
    for index, (pos, umi_seq) in enumerate([(700, "AAAAAAAA"), (702, "CCCCCCCC"), (704, "GGGGGGGG")]):
        rows.append(
            f"p{index}_{umi_seq}\t99\tchr1\t{pos}\t60\t10M\t=\t{pos + 50}\t60\tAAAAAAAAAA\tIIIIIIIIII\n"
        )
        rows.append(
            f"p{index}_{umi_seq}\t147\tchr1\t{pos + 50}\t60\t10M\t=\t{pos}\t-60\tAAAAAAAAAA\tIIIIIIIIII\n"
        )
    bam = write_bam(work, "paired", HEADER + "".join(rows))

    reference = work / "paired_umi.bam"
    subject = work / "paired_rust.bam"
    reference_result = umi(["dedup", "-I", str(bam), "-S", str(reference), "--paired", "--random-seed", "1"])
    subject_result = rust(["dedup", "-I", str(bam), "-S", str(subject), "--paired", "--random-seed", "1"])
    if reference_result.returncode != 0 or subject_result.returncode != 0:
        check("C8 paired exit status", False,
              f"umi exit={reference_result.returncode} rust exit={subject_result.returncode} "
              f"rust_stderr={subject_result.stderr.strip()[:200]}")
        return
    check("C8 paired records match",
          safe_records(subject) is not None and safe_records(subject) == safe_records(reference),
          f"rust={safe_records(subject) and len(safe_records(subject))} "
          f"umi={safe_records(reference) and len(safe_records(reference))} "
          f"rust_stderr={subject_result.stderr.strip()[:160]}")
    _, keys = read_pairs(subject)
    names = [line.split("\t")[0] for line in run(["samtools", "view", str(subject)]).stdout.splitlines()]
    # Each selected read1 must be joined by exactly one recovered mate, so the
    # QNAME appears twice and there are three distinct pairs.
    check("C8 mates recovered",
          len(names) == 6 and len(set(names)) == 3,
          f"expected 3 pairs (6 records, 3 names), got {len(names)} records and "
          f"{len(set(names))} names: {names}")
    check("C8 paired output sorted", is_coordinate_sorted(subject), "not coordinate sorted")
    _ = keys


def check_tags(work: Path) -> None:
    """UMI taken from a BAM tag rather than the read name."""
    rows = []
    for index, umi_tag_value in enumerate(["AAAAAAAA", "AAAAAAAC", "TTTTTTTT"]):
        rows.append(
            f"t{index}\t0\tchr1\t{900 + index}\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tRX:Z:{umi_tag_value}\n"
        )
    bam = write_bam(work, "tagged", HEADER + "".join(rows))
    reference = work / "tagged_umi.bam"
    subject = work / "tagged_rust.bam"
    umi(["dedup", "-I", str(bam), "-S", str(reference), "--extract-umi-method=tag", "--umi-tag=RX"])
    result = rust(["dedup", "-I", str(bam), "-S", str(subject), "--extract-umi-method=tag", "--umi-tag=RX"])
    check("C4 tag UMI source matches",
          result.returncode == 0 and safe_records(subject) is not None
          and safe_records(subject) == safe_records(reference),
          f"exit={result.returncode} rust={safe_records(subject) and len(safe_records(subject))} "
          f"umi={safe_records(reference) and len(safe_records(reference))} "
          f"stderr={result.stderr.strip()[:160]}")


def check_count_and_tab(work: Path) -> None:
    """count --per-gene and count_tab (part of C9)."""
    rows = []
    genes = [("G1", "AAAAAAAA"), ("G1", "AAAAAAAC"), ("G1", "TTTTTTTT"), ("G2", "CCCCCCCC"), ("G2", "CCCCCCCG")]
    for index, (gene, umi_value) in enumerate(genes):
        rows.append(
            f"c{index}_{umi_value}\t0\tchr1\t{1200 + index}\t60\t10M\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\tGX:Z:{gene}\n"
        )
    bam = write_bam(work, "genes", HEADER + "".join(rows))

    reference = work / "count_umi.tsv"
    subject = work / "count_rust.tsv"
    reference_result = umi(["count", "-I", str(bam), "-S", str(reference), "--per-gene", "--gene-tag", "GX"])
    subject_result = rust(["count", "-I", str(bam), "-S", str(subject), "--per-gene", "--gene-tag", "GX"])
    if reference_result.returncode != 0 or subject_result.returncode != 0:
        check("C9 count --per-gene", False,
              f"umi exit={reference_result.returncode} rust exit={subject_result.returncode} "
              f"stderr={subject_result.stderr.strip()[:200]}")
    else:
        check("C9 count --per-gene", reference.read_text() == subject.read_text(),
              f"expected {reference.read_text()!r} got {subject.read_text()!r}")

    tab = work / "reads.tsv"
    tab.write_text("r1_AAAAAAAA\tG1\nr2_AAAAAAAA\tG1\nr3_CCCCCCCC\tG1\nr4_TTTTTTTT\tG2\n")
    reference_tab = work / "reads_umi.tsv"
    subject_tab = work / "reads_rust.tsv"
    reference_result = umi(["count_tab", "-I", str(tab), "-S", str(reference_tab)])
    subject_result = rust(["count_tab", "-I", str(tab), "-S", str(subject_tab)])
    if reference_result.returncode != 0 or subject_result.returncode != 0:
        check("C9 count_tab", False,
              f"umi exit={reference_result.returncode} rust exit={subject_result.returncode} "
              f"stderr={subject_result.stderr.strip()[:200]}")
    else:
        check("C9 count_tab", reference_tab.read_text() == subject_tab.read_text(),
              f"expected {reference_tab.read_text()!r} got {subject_tab.read_text()!r}")

    per_cell_tab = work / "reads_cell.tsv"
    per_cell_tab.write_text(
        "r1_CB1_AAAAAAAA\tG1\nr2_CB1_AAAAAAAA\tG1\nr3_CB2_AAAAAAAA\tG1\nr4_CB2_CCCCCCCC\tG1\n"
    )
    reference_cell = work / "reads_cell_umi.tsv"
    subject_cell = work / "reads_cell_rust.tsv"
    reference_result = umi(["count_tab", "-I", str(per_cell_tab), "-S", str(reference_cell), "--per-cell"])
    subject_result = rust(["count_tab", "-I", str(per_cell_tab), "-S", str(subject_cell), "--per-cell"])
    if reference_result.returncode != 0 or subject_result.returncode != 0:
        check("C9 count_tab --per-cell", False,
              f"umi exit={reference_result.returncode} rust exit={subject_result.returncode} "
              f"stderr={subject_result.stderr.strip()[:200]}")
    else:
        check("C9 count_tab --per-cell", reference_cell.read_text() == subject_cell.read_text(),
              f"expected {reference_cell.read_text()!r} got {subject_cell.read_text()!r}")

    stats_prefix = work / "stats"
    stats_result = rust(["dedup", "-I", str(work / "genes.bam"), "-S", str(work / "stats_out.bam"),
                         "--output-stats", str(stats_prefix)])
    produced = sorted(p.name for p in work.glob("stats_*"))
    # The null model is not implemented; emitting columns that only repeat the
    # observed values would look like a valid statistic. Refuse instead, and do
    # it before any output file is created.
    check("C4 --output-stats is refused until the null model exists",
          stats_result.returncode == 2 and not produced and not (work / "stats_out.bam").exists(),
          f"exit={stats_result.returncode} files={produced} stderr={stats_result.stderr.strip()[:200]}")

    bad = work / "bad.tsv"
    bad.write_text("only_one_column\n")
    result = rust(["count_tab", "-I", str(bad), "-S", str(work / "bad_out.tsv")])
    check("C9 count_tab rejects malformed tables", result.returncode == 3,
          f"exit={result.returncode} stderr={result.stderr.strip()[:160]}")


def check_capability_gating(work: Path) -> None:
    """Unsupported options must fail before any output exists (C4)."""
    bam = work / "dedup.bam"
    for option, args in [
        ("--per-gene", ["--per-gene"]),
        ("--per-cell", ["--per-cell"]),
        ("--filter-umi", ["--filter-umi"]),
        ("--gene-transcript-map", ["--gene-transcript-map", "map.tsv"]),
    ]:
        target = work / f"gate_{option.strip('-')}.bam"
        result = rust(["dedup", "-I", str(bam), "-S", str(target), *args])
        check(f"C4 {option} is rejected before writing",
              result.returncode == 2 and not target.exists(),
              f"exit={result.returncode} exists={target.exists()} stderr={result.stderr.strip()[:160]}")
    # --chrom is supported now: it must succeed and keep only the target contig.
    chrom_target = work / "gate_chrom.bam"
    result = rust(["dedup", "-I", str(bam), "-S", str(chrom_target), "--chrom", "chr1"])
    check("C4 --chrom processes the requested contig",
          result.returncode == 0 and chrom_target.exists(),
          f"exit={result.returncode} stderr={result.stderr.strip()[:160]}")
    result = rust(["dedup", "-I", str(bam), "-S", str(work / "same.bam")])
    check("C4 baseline invocation works", result.returncode == 0, result.stderr.strip()[:200])
    result = rust(["dedup", "-I", str(bam), "-S", str(bam)])
    check("C4 input/output collision is rejected", result.returncode == 2,
          f"exit={result.returncode} stderr={result.stderr.strip()[:160]}")


def main() -> int:
    if shutil.which("umi_tools") is None or shutil.which("samtools") is None:
        print("ERROR: umi_tools and samtools must be on PATH", file=sys.stderr)
        return 2
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built; run cargo build --release", file=sys.stderr)
        return 2
    version = run(["umi_tools", "--version"]).stdout.strip()
    print(f"oracle: {version}")
    if "1.1.6" not in version:
        print(f"WARNING: expected UMI-tools 1.1.6, found {version}", file=sys.stderr)

    with tempfile.TemporaryDirectory(prefix="umitools-oracle-") as directory:
        work = Path(directory)
        check_b01(work)
        check_b02(work)
        check_b03(work)
        check_b04(work)
        check_grouping(work)
        check_dedup_records(work)
        check_determinism(work)
        check_paired(work)
        check_tags(work)
        check_count_and_tab(work)
        check_capability_gating(work)

    print(f"\n{len(PASSED)} passed, {len(FAILED)} failed")
    for name, detail in FAILED:
        print(f"  FAILED {name}: {detail}")
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main())
