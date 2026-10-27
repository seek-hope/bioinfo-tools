#!/usr/bin/env python3
"""Byte-level exactness harness: rust-umitools vs UMI-tools 1.1.6.

For every case both tools run on the same input with the same arguments, then the
decompressed BAM bytes (header + records) or the text output bytes are compared
directly. Any difference is reported with its first byte offset and, for BAM
headers, a readable header diff.

Run via tests/oracle/run_exact.sh.
"""

from __future__ import annotations

import gzip
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
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


def run(cmd: list[str], binary: bool = False) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd, capture_output=True, text=binary, check=False
    )


def umi(args: list[str]) -> subprocess.CompletedProcess:
    return run(["umi_tools", *args])


def rust(args: list[str]) -> subprocess.CompletedProcess:
    return run([str(BINARY), *args])


# --------------------------------------------------------------------------- #
# fixtures
# --------------------------------------------------------------------------- #

HEADER = (
    "@HD\tVN:1.6\tSO:coordinate\n"
    "@SQ\tSN:chr1\tLN:1000000\n"
    "@SQ\tSN:chr2\tLN:1000000\n"
)


def rec(
    name: str,
    umi: str,
    flag: int = 0,
    pos: int = 100,
    mapq: int = 60,
    cigar: str | None = None,
    rname: str = "chr1",
    rnext: str = "*",
    pnext: int = 0,
    tlen: int = 0,
    extra: str = "",
    seq: str = "A" * 10,
) -> str:
    # The default CIGAR must consume exactly the query length, otherwise
    # samtools rejects the fixture.
    if cigar is None:
        cigar = f"{len(seq)}M"
    qual = "I" * len(seq)
    return (
        f"{name}_{umi}\t{flag}\t{rname}\t{pos}\t{mapq}\t{cigar}\t{rnext}\t{pnext}\t{tlen}"
        f"\t{seq}\t{qual}{extra}\n"
    )


def write_bam(work: Path, name: str, body: str) -> Path:
    sam = work / f"{name}.sam"
    bam = work / f"{name}.bam"
    sam.write_text(HEADER + body)
    result = run(["samtools", "sort", "-o", str(bam), str(sam)])
    if result.returncode != 0:
        raise RuntimeError(f"samtools sort failed: {result.stderr!r}")
    indexed = run(["samtools", "index", str(bam)])
    if indexed.returncode != 0:
        raise RuntimeError(f"samtools index failed: {indexed.stderr!r}")
    return bam


def families_body() -> str:
    """UMI families covering ties, chains, both strands, clipping and splicing."""
    rows = []
    families = [
        # (pos, reverse, [(umi, count, mapq)])
        (100, False, [("AAAAAAAA", 6, 60), ("CAAAAAAA", 3, 60), ("CCAAAAAA", 1, 60)]),
        (100, True, [("GGGGGGGG", 4, 60), ("GGGGGGGA", 4, 60)]),
        (200, False, [("ACGTACGT", 5, 60), ("ACGTACGA", 1, 60), ("ACGTACCA", 1, 60)]),
        (300, False, [("TTTTTTTT", 2, 40), ("TTTTTTTA", 2, 40), ("TTTTTTAA", 2, 40)]),
        (400, True, [("CCCCCCCC", 10, 60), ("CCCCCCCA", 1, 60), ("CCCCCCAA", 1, 60)]),
        (500, False, [("AAAAAAAAA", 1, 60), ("CAAAAAAAA", 1, 60)]),  # 9 nt
        (600, False, [("NANANANA", 3, 60), ("NANANANC", 1, 60)]),
    ]
    for pos, reverse, members in families:
        flag = 16 if reverse else 0
        for umi, count, mapq in members:
            for _ in range(count):
                rows.append(rec(f"r{len(rows)}", umi, flag=flag, pos=pos, mapq=mapq))
    # soft clip and spliced reads (different bundle positions)
    rows.append(rec("rsoft", "AAAAAAAA", pos=700, cigar="5S5M"))
    rows.append(rec("rsplice", "CCCCCCCC", pos=800, cigar="5M5N5M"))
    # identical coordinate, different strands
    rows.append(rec("rfwd", "GGGGGGGG", flag=0, pos=900))
    rows.append(rec("rrev", "GGGGGGGG", flag=16, pos=900))
    # trailing unmapped record
    rows.append("runmap_AAAAAAAA\t4\t*\t0\t0\t*\t*\t0\t0\tAAAAAAAAAA\tIIIIIIIIII\n")
    return "".join(rows)


def paired_body() -> str:
    rows = []
    for index, (pos, umi, reverse) in enumerate(
        [(100, "AAAAAAAA", False), (300, "CCCCCCCC", True), (500, "GGGGGGGG", False)]
    ):
        rows.append(
            rec(f"p{index}", umi, flag=99 if not reverse else 83, pos=pos, rnext="=", pnext=pos + 50, tlen=60)
        )
        rows.append(
            rec(f"p{index}", umi, flag=147 if not reverse else 163, pos=pos + 50, rnext="=", pnext=pos, tlen=-60)
        )
    # unpaired read (default --unpaired-reads use)
    rows.append(rec("solo", "TTTTTTTT", flag=0, pos=700))
    # mate unmapped (default --unmapped-reads discard)
    rows.append(rec("m1", "ACGTACGT", flag=73, pos=800, rnext="*", pnext=0))
    rows.append("m2_ACGTACGT\t133\t*\t0\t0\t*\tchr1\t800\t0\tAAAAAAAAAA\tIIIIIIIIII\n")
    # chimeric pair (default --chimeric-pairs use)
    rows.append(rec("x1", "AGAGAGAG", flag=65, pos=900, rname="chr1", rnext="chr2", pnext=100))
    rows.append(rec("x2", "AGAGAGAG", flag=129, pos=100, rname="chr2", rnext="chr1", pnext=900))
    return "".join(rows)


def paired_repeat_body() -> str:
    """Pairs that expose mate recovery around deduplication.

    Two identical pairs share a position and UMI (the representative draw keeps
    one), one pair is mergeable at distance one, one UMI has a MAPQ tie, one
    read1 has no mate in the file, and one pair is chimeric across contigs.
    """
    rows = []
    pairs = [
        (100, "AAAAAAAA", 60, False),
        (100, "AAAAAAAA", 60, False),
        (300, "CCCCCCCC", 60, True),
        (300, "CCCCCCCA", 60, True),
        (500, "GGGGGGGG", 40, False),
        (500, "GGGGGGGG", 60, False),
    ]
    for index, (pos, umi, mapq, reverse) in enumerate(pairs):
        read1 = 99 if not reverse else 83
        read2 = 147 if not reverse else 163
        rows.append(
            rec(f"pd{index}", umi, flag=read1, pos=pos, mapq=mapq, rnext="=", pnext=pos + 50, tlen=60)
        )
        rows.append(
            rec(f"pd{index}", umi, flag=read2, pos=pos + 50, mapq=mapq, rnext="=", pnext=pos, tlen=-60)
        )
    # read1 whose mate is absent from the file
    rows.append(rec("solo", "ACGTACGT", flag=99, pos=800, rnext="=", pnext=850, tlen=60))
    # chimeric pair across contigs (default --chimeric-pairs use)
    rows.append(rec("pc1", "AGAGAGAG", flag=65, pos=900, rname="chr1", rnext="chr2", pnext=100))
    rows.append(rec("pc2", "AGAGAGAG", flag=129, pos=100, rname="chr2", rnext="chr1", pnext=900))
    return "".join(rows)


def tagged_body() -> str:
    genes = [("G1", "AAAAAAAA"), ("G1", "AAAAAAAC"), ("G1", "TTTTTTTT"), ("G2", "CCCCCCCC")]
    rows = []
    for index, (gene, umi) in enumerate(genes):
        rows.append(
            rec(
                f"t{index}",
                umi,
                pos=100 + index * 10,
                extra=f"\tRX:Z:{umi}\tGX:Z:{gene}",
            )
        )
    return "".join(rows)


# --------------------------------------------------------------------------- #
# comparison
# --------------------------------------------------------------------------- #

CHUNK = 1 << 20


def first_difference_streaming(reference: Path, subject: Path) -> int | None:
    """Return the first differing decompressed offset, reading 1 MiB at a time.

    Large BAMs must not be materialised twice; the comparison only keeps one
    chunk of each side alive.
    """
    offset = 0
    with gzip.open(reference, "rb") as left, gzip.open(subject, "rb") as right:
        while True:
            a = left.read(CHUNK)
            b = right.read(CHUNK)
            if a != b:
                for index in range(min(len(a), len(b))):
                    if a[index] != b[index]:
                        return offset + index
                return offset + min(len(a), len(b))
            offset += len(a)
            if not a:
                return None


def header_lines(path: Path) -> list[str]:
    out = run(["samtools", "view", "--no-PG", "-H", str(path)])
    return out.stdout.decode(errors="replace").splitlines()


def compare(name: str, reference: Path, subject: Path, binary: bool) -> None:
    if binary:
        offset = first_difference_streaming(reference, subject)
        if offset is None:
            check(name, True)
            return
        detail = f"decompressed bytes differ at offset {offset}"
    else:
        a = reference.read_bytes()
        b = subject.read_bytes()
        if a == b:
            check(name, True)
            return
        offset = next((i for i in range(min(len(a), len(b))) if a[i] != b[i]), min(len(a), len(b)))
        detail = f"bytes differ at offset {offset} (sizes {len(a)} vs {len(b)})"
    if binary:
        ha, hb = header_lines(reference), header_lines(subject)
        if ha != hb:
            detail += f"; header diff: umi={ha[-3:]} rust={hb[-3:]}"
        else:
            detail += "; headers identical, difference is in records"
    else:
        detail += f"; umi={a[max(0, offset - 40) : offset + 40]!r}"
        detail += f" rust={b[max(0, offset - 40) : offset + 40]!r}"
    check(name, False, detail)


def run_bam_case(
    name: str,
    work: Path,
    input_bam: Path,
    build_args,
    suffix: str = ".bam",
) -> None:
    reference = work / f"{safe(name)}_umi{suffix}"
    subject = work / f"{safe(name)}_rust{suffix}"
    for stale in (reference, subject):
        if stale.exists():
            stale.unlink()
    ref_result = umi(build_args(str(reference)))
    sub_result = rust(build_args(str(subject)))
    if ref_result.returncode != 0 or sub_result.returncode != 0:
        check(
            name,
            False,
            f"exit umi={ref_result.returncode} rust={sub_result.returncode}; "
            f"umi_stderr={ref_result.stderr.decode(errors='replace')[-200:]!r} "
            f"rust_stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}",
        )
        return
    compare(name, reference, subject, binary=suffix == ".bam")


def safe(name: str) -> str:
    return "".join(ch if ch.isalnum() else "_" for ch in name)


# --------------------------------------------------------------------------- #
# cases
# --------------------------------------------------------------------------- #

METHODS = ["unique", "percentile", "cluster", "adjacency", "directional"]


def dedup_cases(work: Path, families: Path, paired: Path, tagged: Path) -> None:
    for method in METHODS:
        for label, input_bam, extra in [
            ("single-sorted", families, []),
            ("single-nosort", families, ["--no-sort-output"]),
            ("paired-sorted", paired, ["--paired"]),
            ("paired-nosort", paired, ["--paired", "--no-sort-output"]),
        ]:
            def args(out: str, method=method, input_bam=input_bam, extra=extra) -> list[str]:
                return [
                    "dedup", "-I", str(input_bam), "-S", out,
                    "--method", method, "--random-seed", "1", *extra,
                ]

            run_bam_case(f"dedup {method} {label}", work, input_bam, args)

        def tag_args(out: str, method=method) -> list[str]:
            return [
                "dedup", "-I", str(tagged), "-S", out,
                "--method", method, "--random-seed", "1",
                "--extract-umi-method=tag", "--umi-tag=RX",
            ]

        run_bam_case(f"dedup {method} tag-source", work, tagged, tag_args)


def paired_repeat_cases(work: Path, bam: Path) -> None:
    for method in METHODS:
        def args(out: str, method=method) -> list[str]:
            return [
                "dedup", "-I", str(bam), "-S", out,
                "--method", method, "--random-seed", "1", "--paired",
            ]

        run_bam_case(f"dedup {method} paired-repeat", work, bam, args)

    def nosort(out: str) -> list[str]:
        return [
            "dedup", "-I", str(bam), "-S", out,
            "--method", "directional", "--random-seed", "1", "--paired",
            "--no-sort-output",
        ]

    run_bam_case("dedup directional paired-repeat-nosort", work, bam, nosort)


def edge_cases(work: Path) -> None:
    """Edge cases and non-default options, all compared byte for byte."""

    def check_dedup(name: str, fixture: str, body: str, extra: list[str]) -> None:
        bam = write_bam(work, fixture, body)

        def args(out: str, bam=bam, extra=extra) -> list[str]:
            return ["dedup", "-I", str(bam), "-S", out, "--random-seed", "1", *extra]

        run_bam_case(name, work, bam, args)

    check_dedup(
        "dedup mapq filter", "edge_mapq",
        "".join(rec(f"ma{i}", "AAAAAAAA", mapq=20) for i in range(3))
        + "".join(rec(f"mb{i}", "CCCCCCCC", mapq=60) for i in range(3)),
        ["--mapping-quality", "40"],
    )
    check_dedup(
        "dedup detection XT", "edge_xt",
        "".join(rec(f"xa{i}", "AAAAAAAA", extra="\tXT:Z:R") for i in range(3))
        + "".join(rec(f"xb{i}", "AAAAAAAA", extra="\tXT:Z:U") for i in range(3)),
        ["--multimapping-detection-method", "XT"],
    )
    check_dedup(
        "dedup detection NH", "edge_nh",
        "".join(rec(f"na{i}", "AAAAAAAA", extra="\tNH:i:2") for i in range(3))
        + "".join(rec(f"nb{i}", "AAAAAAAA", extra="\tNH:i:1") for i in range(3)),
        ["--multimapping-detection-method", "NH"],
    )
    check_dedup(
        "dedup detection X0", "edge_x0",
        "".join(rec(f"za{i}", "AAAAAAAA", extra="\tX0:i:2") for i in range(3))
        + "".join(rec(f"zb{i}", "AAAAAAAA", extra="\tX0:i:1") for i in range(3)),
        ["--multimapping-detection-method", "X0"],
    )
    check_dedup(
        "dedup spliced is unique", "edge_spliced",
        "".join(rec(f"sa{i}", "AAAAAAAA", cigar="5S5M") for i in range(3))
        + "".join(rec(f"sb{i}", "CCCCCCCC", cigar="5M5N5M") for i in range(3)),
        ["--spliced-is-unique"],
    )
    check_dedup(
        "dedup long soft clip and intron", "edge_long",
        rec("lc1", "AAAAAAAA", pos=100, cigar="50S10M", seq="A" * 60)
        + rec("lc2", "CCCCCCCC", pos=100, cigar="10M100N10M", seq="A" * 20)
        + rec("lc3", "GGGGGGGG", pos=100, cigar="10M"),
        [],
    )
    check_dedup(
        "dedup read length", "edge_readlen",
        "".join(rec(f"ra{i}", "AAAAAAAA", seq="A" * 10, cigar="10M") for i in range(3))
        + "".join(rec(f"rb{i}", "AAAAAAAA", seq="A" * 20, cigar="20M") for i in range(3)),
        ["--read-length"],
    )
    check_dedup(
        "dedup paired ignore-tlen", "edge_tlen",
        "".join(rec(f"pa{i}", "AAAAAAAA", flag=99, rnext="=", pnext=150, tlen=60) for i in range(3))
        + "".join(rec(f"pa{i}", "AAAAAAAA", flag=147, pos=150, rnext="=", pnext=100, tlen=-60)
                for i in range(3)),
        ["--paired", "--ignore-tlen"],
    )
    for length in (21, 22, 32, 40):
        umi_a = "A" * length
        umi_b = "C" + "A" * (length - 1)
        check_dedup(
            f"dedup umi length {length}", f"edge_len{length}",
            "".join(rec(f"la{i}", umi_a, seq="A" * 20) for i in range(3))
            + "".join(rec(f"lb{i}", umi_b, seq="A" * 20) for i in range(3)),
            [],
        )
    check_dedup(
        "dedup threshold 2", "edge_threshold2",
        "".join(rec(f"ta{i}", "AAAAAAAA") for i in range(3))
        + rec("tb0", "AAAAAAAC") + rec("tc0", "AAAAAACC"),
        ["--edit-distance-threshold", "2"],
    )
    check_dedup(
        "dedup threshold 0", "edge_threshold0",
        "".join(rec(f"ta{i}", "AAAAAAAA") for i in range(3))
        + rec("tb0", "AAAAAAAC"),
        ["--edit-distance-threshold", "0"],
    )
    check_dedup(
        "dedup threshold at umi length", "edge_threshold_len",
        rec("ta0", "AAAAAAAA") + rec("tb0", "CCCCCCCC"),
        ["--edit-distance-threshold", "8"],
    )
    check_dedup(
        "dedup missing umi tag", "edge_missing_tag",
        rec("tg1", "AAAAAAAA", extra="\tRX:Z:AAAAAAAA\tGX:Z:G1")
        + rec("tg2", "CCCCCCCC", extra="\tGX:Z:G1"),
        ["--extract-umi-method=tag", "--umi-tag=RX"],
    )

    check_dedup(
        "dedup --chrom", "edge_chrom",
        "".join(rec(f"ca{i}", "AAAAAAAA", rname="chr1") for i in range(3))
        + "".join(rec(f"cb{i}", "CCCCCCCC", rname="chr2") for i in range(3)),
        ["--chrom", "chr1"],
    )

    # subset draws from the same RNG as representative ties.
    subset_body = "".join(
        rec(f"su{i}", "AAAAAAAA" if i % 2 == 0 else "CCCCCCCC", pos=100 + (i % 3) * 100)
        for i in range(30)
    )
    check_dedup("dedup subset", "edge_subset", subset_body, ["--subset", "0.5"])

    # gene keyed dedup: the same UMI at two positions of one gene must merge.
    gene_body = (
        "".join(rec(f"ga{i}", "AAAAAAAA", pos=100, extra="\tGX:Z:G1") for i in range(3))
        + "".join(rec(f"gb{i}", "AAAAAAAA", pos=500, extra="\tGX:Z:G1") for i in range(3))
        + "".join(rec(f"gc{i}", "CCCCCCCC", pos=900, extra="\tGX:Z:G2") for i in range(3))
    )
    check_dedup("dedup per-gene", "edge_per_gene", gene_body,
                ["--per-gene", "--gene-tag", "GX"])

    # cell aware dedup: the same UMI in two cells stays two molecules.
    cell_body = "".join(
        rec(f"ca{cell}{i}", "AAAAAAAA", pos=100,
            extra=f"\tRX:Z:AAAAAAAA\tGX:Z:G1\tCB:Z:{cell}")
        for cell in ("CB1", "CB2") for i in range(2)
    )
    check_dedup("dedup per-cell", "edge_per_cell", cell_body,
                ["--per-gene", "--gene-tag", "GX", "--per-cell", "--cell-tag", "CB",
                 "--extract-umi-method=tag", "--umi-tag=RX"])

    # group: large UG ids exercise the C/S/I integer encoding.
    rows = "".join(rec(f"ug{i}", "AAAAAAAA", pos=100 + i * 1000) for i in range(300))
    big = write_bam(work, "edge_large_ug", rows)

    def ug_args(out: str, big=big) -> list[str]:
        return ["group", "-I", str(big), "-S", out, "--output-bam", "--method", "unique"]

    run_bam_case("group large UG ids", work, big, ug_args)

    # group: pre-existing UG/BX must be replaced, not duplicated.
    tagged = write_bam(
        work, "edge_existing_tags",
        rec("ex1", "AAAAAAAA", extra="\tUG:i:7\tBX:Z:AAAAAAAA")
        + rec("ex2", "CCCCCCCC", extra="\tUG:i:9\tBX:Z:CCCCCCCC"),
    )

    def tag_args(out: str, tagged=tagged) -> list[str]:
        return ["group", "-I", str(tagged), "-S", out, "--output-bam", "--method", "unique"]

    run_bam_case("group replaces existing tags", work, tagged, tag_args)


def group_cases(work: Path, families: Path) -> None:
    for method in METHODS:
        reference = work / f"group_{method}_umi.bam"
        subject = work / f"group_{method}_rust.bam"
        ref_tsv = work / f"group_{method}_umi.tsv"
        sub_tsv = work / f"group_{method}_rust.tsv"

        def args(out: str, tsv: Path, method=method) -> list[str]:
            return [
                "group", "-I", str(families), "-S", out, "--output-bam",
                "--group-out", str(tsv), "--method", method,
            ]

        ref_result = umi(args(str(reference), ref_tsv))
        sub_result = rust(args(str(subject), sub_tsv))
        if ref_result.returncode != 0 or sub_result.returncode != 0:
            check(
                f"group {method}",
                False,
                f"exit umi={ref_result.returncode} rust={sub_result.returncode}; "
                f"umi_stderr={ref_result.stderr.decode(errors='replace')[-200:]!r} "
                f"rust_stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}",
            )
            continue
        compare(f"group {method} bam", reference, subject, binary=True)
        compare(f"group {method} tsv", ref_tsv, sub_tsv, binary=False)


def count_cases(work: Path, tagged: Path) -> None:
    reference = work / "count_umi.tsv"
    subject = work / "count_rust.tsv"

    def args(out: str) -> list[str]:
        return ["count", "-I", str(tagged), "-S", out, "--per-gene", "--gene-tag", "GX"]

    ref_result = umi(args(str(reference)))
    sub_result = rust(args(str(subject)))
    if ref_result.returncode != 0 or sub_result.returncode != 0:
        check(
            "count --per-gene",
            False,
            f"exit umi={ref_result.returncode} rust={sub_result.returncode}; "
            f"rust_stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}",
        )
        return
    compare("count --per-gene", reference, subject, binary=False)


def count_tab_cases(work: Path) -> None:
    table = work / "reads.tsv"
    table.write_text(
        "r1_AAAAAAAA\tG1\nr2_AAAAAAAA\tG1\nr3_CCCCCCCC\tG1\n"
        "r4_TTTTTTTT\tG2\nr5_TTTTTTTT\tG2\nr6_GGGGGGGG\tG2\n"
    )
    per_cell = work / "reads_cell.tsv"
    per_cell.write_text(
        "r1_CB1_AAAAAAAA\tG1\nr2_CB1_AAAAAAAA\tG1\nr3_CB2_CCCCCCCC\tG1\n"
        "r4_CB2_CCCCCCCC\tG1\nr5_CB1_TTTTTTTT\tG2\n"
    )
    for label, source, extra in [
        ("plain", table, []),
        ("per-cell", per_cell, ["--per-cell"]),
    ]:
        reference = work / f"count_tab_{safe(label)}_umi.tsv"
        subject = work / f"count_tab_{safe(label)}_rust.tsv"

        def args(out: str, source=source, extra=extra) -> list[str]:
            return ["count_tab", "-I", str(source), "-S", out, *extra]

        ref_result = umi(args(str(reference)))
        sub_result = rust(args(str(subject)))
        if ref_result.returncode != 0 or sub_result.returncode != 0:
            check(
                f"count_tab {label}",
                False,
                f"exit umi={ref_result.returncode} rust={sub_result.returncode}; "
                f"rust_stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}",
            )
            continue
        compare(f"count_tab {label}", reference, subject, binary=False)



def gene_group_cases(work: Path) -> None:
    body = (
        "".join(rec(f"ga{i}", "AAAAAAAA", pos=100, extra="\tGX:Z:G1") for i in range(3))
        + "".join(rec(f"gb{i}", "AAAAAAAA", pos=500, extra="\tGX:Z:G1") for i in range(3))
        + "".join(rec(f"gc{i}", "CCCCCCCC", pos=900, extra="\tGX:Z:G2") for i in range(3))
    )
    bam = write_bam(work, "group_gene", body)
    reference = work / "group_gene_umi.bam"
    subject = work / "group_gene_rust.bam"
    ref_tsv = work / "group_gene_umi.tsv"
    sub_tsv = work / "group_gene_rust.tsv"

    def args(out: str, tsv: Path) -> list[str]:
        return ["group", "-I", str(bam), "-S", out, "--output-bam", "--group-out", str(tsv),
                "--per-gene", "--gene-tag", "GX"]

    ref_result = umi(args(str(reference), ref_tsv))
    sub_result = rust(args(str(subject), sub_tsv))
    if ref_result.returncode != 0 or sub_result.returncode != 0:
        check("group --per-gene", False,
              f"exit umi={ref_result.returncode} rust={sub_result.returncode} "
              f"stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}")
        return
    compare("group --per-gene bam", reference, subject, binary=True)
    compare("group --per-gene tsv", ref_tsv, sub_tsv, binary=False)


def count_cell_cases(work: Path) -> None:
    body = "".join(
        rec(f"cc{cell}{i}", "AAAAAAAA", pos=100,
            extra=f"\tRX:Z:AAAAAAAA\tGX:Z:G1\tCB:Z:{cell}")
        for cell in ("CB1", "CB2") for i in range(2)
    )
    bam = write_bam(work, "count_cell", body)
    reference = work / "count_cell_umi.tsv"
    subject = work / "count_cell_rust.tsv"

    def args(out: str) -> list[str]:
        return ["count", "-I", str(bam), "-S", out, "--per-gene", "--gene-tag", "GX",
                "--per-cell", "--cell-tag", "CB", "--extract-umi-method=tag", "--umi-tag=RX"]

    wide_reference = work / "count_cell_wide_umi.tsv"
    wide_subject = work / "count_cell_wide_rust.tsv"

    def wide_args(out: str) -> list[str]:
        return ["count", "-I", str(bam), "-S", out, "--per-gene", "--gene-tag", "GX",
                "--per-cell", "--cell-tag", "CB", "--wide",
                "--extract-umi-method=tag", "--umi-tag=RX"]

    ref_result = umi(wide_args(str(wide_reference)))
    sub_result = rust(wide_args(str(wide_subject)))
    if ref_result.returncode != 0 or sub_result.returncode != 0:
        check("count --wide", False,
              f"exit umi={ref_result.returncode} rust={sub_result.returncode} "
              f"stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}")
    else:
        compare("count --wide", wide_reference, wide_subject, binary=False)

    ref_result = umi(args(str(reference)))
    sub_result = rust(args(str(subject)))
    if ref_result.returncode != 0 or sub_result.returncode != 0:
        check("count --per-cell", False,
              f"exit umi={ref_result.returncode} rust={sub_result.returncode} "
              f"stderr={sub_result.stderr.decode(errors='replace')[-200:]!r}")
        return
    compare("count --per-cell", reference, subject, binary=False)



def new_option_cases(work: Path) -> None:
    """Options added after the initial 70-case suite."""
    # --buffer-whole-contig: two reads share the 5' start but are 2 kb apart in
    # alignment order, so the default 1 kb window splits them.
    body = (
        rec("wc1", "AAAAAAAA", pos=100)
        + rec("wc2", "CCCCCCCC", pos=1500)
        + rec("wc3", "AAAAAAAA", pos=2100, cigar="2000S10M", seq="A" * 2010)
    )
    bam = write_bam(work, "whole_contig", body)
    for label, extra in [
        ("default window", []),
        ("whole contig", ["--buffer-whole-contig"]),
    ]:
        run_bam_case(
            f"dedup soft-clip {label}",
            work,
            bam,
            lambda out, extra=extra: [
                "dedup", "-I", str(bam), "-S", out, "--random-seed", "1", *extra
            ],
        )

    # --filter-umi keeps only clusters whose representative is whitelisted.
    body = "".join(
        rec(f"fu{i}", umi, pos=100 + i * 10)
        for i, umi in enumerate(["AAAA", "CCCC", "GGTT"])
    )
    bam = write_bam(work, "filter_umi", body)
    single = work / "whitelist_single.tsv"
    single.write_text("AAAA\tcomment\n")
    run_bam_case(
        "dedup --filter-umi single whitelist",
        work,
        bam,
        lambda out: [
            "dedup", "-I", str(bam), "-S", out, "--random-seed", "1",
            "--filter-umi", "--umi-whitelist", str(single),
        ],
    )
    left = work / "whitelist_left.tsv"
    right = work / "whitelist_right.tsv"
    left.write_text("AA\n")
    right.write_text("AA\n")
    run_bam_case(
        "dedup --filter-umi paired whitelist",
        work,
        bam,
        lambda out: [
            "dedup", "-I", str(bam), "-S", out, "--random-seed", "1",
            "--filter-umi", "--umi-whitelist", str(left),
            "--umi-whitelist-paired", str(right),
        ],
    )

    # --compresslevel changes only the BGZF encoding, not the records.
    body = "".join(rec(f"cl{i}", "AAAAAAAA", pos=100 + i * 10) for i in range(3))
    bam = write_bam(work, "compresslevel", body)
    run_bam_case(
        "dedup --compresslevel 1",
        work,
        bam,
        lambda out: [
            "dedup", "-I", str(bam), "-S", out, "--random-seed", "1",
            "--compresslevel", "1",
        ],
    )
    run_bam_case(
        "group --compresslevel 1",
        work,
        bam,
        lambda out: [
            "group", "-I", str(bam), "-S", out, "--output-bam",
            "--group-out", out + ".tsv", "--compresslevel", "1",
        ],
    )

    # --gene-transcript-map groups transcripts of one gene across contigs.
    body = (
        rec("tm1", "AAAAAAAA", pos=100, rname="chr1")
        + rec("tm2", "AAAAAAAA", pos=100, rname="chr2")
        + rec("tm3", "CCCCCCCC", pos=200, rname="chr1")
    )
    bam = write_bam(work, "transcript_map", body)
    mapping = work / "gene_transcript.tsv"
    mapping.write_text("gene1\tchr1\ngene1\tchr2\n")
    run_bam_case(
        "count --gene-transcript-map",
        work,
        bam,
        lambda out: [
            "count", "-I", str(bam), "-S", out, "--per-gene", "--per-contig",
            "--gene-transcript-map", str(mapping),
        ],
        suffix=".tsv",
    )

    # --per-contig directly: two UMIs on each of two contigs must be counted
    # independently, with no gene-tag dependency.
    body = (
        rec("pc1", "AAAAAAAA", pos=100, rname="chr1")
        + rec("pc2", "AAAAAAAA", pos=110, rname="chr1")
        + rec("pc3", "CCCCCCCC", pos=100, rname="chr2")
        + rec("pc4", "CCCCCCCC", pos=110, rname="chr2")
    )
    bam = write_bam(work, "per_contig", body)
    run_bam_case(
        "count --per-contig",
        work,
        bam,
        lambda out: [
            "count", "-I", str(bam), "-S", out, "--per-contig",
        ],
        suffix=".tsv",
    )

    # --extract-umi-method=umis reads UMI_ fields from the query name (the
    # format emitted by umis), rather than using the final read-id field.
    def umis_rec(name: str, umi: str, pos: int) -> str:
        return (
            f"{name}:UMI_{umi}\t0\tchr1\t{pos}\t60\t10M\t*\t0\t0\t"
            "AAAAAAAAAA\tIIIIIIIIII\n"
        )

    body = "".join(
        umis_rec(f"u{index}", umi, pos)
        for index, (umi, pos) in enumerate(
            [("AAAAAAAA", 100), ("AAAAAAAA", 110), ("CCCCCCCC", 120)]
        )
    )
    bam = write_bam(work, "umis_source", body)
    run_bam_case(
        "dedup --extract-umi-method=umis",
        work,
        bam,
        lambda out: [
            "dedup", "-I", str(bam), "-S", out, "--random-seed", "1",
            "--extract-umi-method=umis",
        ],
    )


def main() -> int:
    if shutil.which("umi_tools") is None or shutil.which("samtools") is None:
        print("ERROR: umi_tools and samtools must be on PATH", file=sys.stderr)
        return 2
    if not BINARY.exists():
        print(f"ERROR: {BINARY} not built", file=sys.stderr)
        return 2
    version = run(["umi_tools", "--version"]).stdout.decode().strip()
    print(f"exactness harness: {version}")

    with tempfile.TemporaryDirectory(prefix="umitools-exact-") as directory:
        work = Path(directory)
        families = write_bam(work, "families", families_body())
        paired = write_bam(work, "paired", paired_body())
        paired_repeat = write_bam(work, "paired_repeat", paired_repeat_body())
        tagged = write_bam(work, "tagged", tagged_body())

        dedup_cases(work, families, paired, tagged)
        paired_repeat_cases(work, paired_repeat)
        edge_cases(work)
        gene_group_cases(work)
        count_cell_cases(work)
        group_cases(work, families)
        count_cases(work, tagged)
        count_tab_cases(work)
        new_option_cases(work)

    total = len(PASSED) + len(FAILED)
    print(f"\n{len(PASSED)}/{total} byte-identical, {len(FAILED)} differ")
    for name, detail in FAILED:
        print(f"  DIFF {name}: {detail}")
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main())
