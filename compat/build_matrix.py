#!/usr/bin/env python3
"""Build the unified compatibility matrix from pinned upstream help snapshots.

The matrix is deliberately conservative: a surface is only `verified` when the
repository has executable evidence for it. Commands that are not implemented are
kept as `unsupported` rows rather than being silently omitted.
"""

from __future__ import annotations

import csv
import re
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = ROOT / "compat" / "upstream"
OUT = ROOT / "compat" / "matrix.tsv"

UPSTREAM_NAMES = {
    "rust-cutadapt": "cutadapt 4.6",
    "rust-umitools": "UMI-tools 1.1.6",
    "rust-bam": "samtools 1.24",
    "rust-fastqc": "FastQC 0.13.0",
    "rust-fx": "SeqKit 2.14.0 / fq 0.12.0",
}

FIELDS = [
    "tool",
    "upstream",
    "surface",
    "kind",
    "status",
    "evidence",
    "deviation",
    "next_action",
]


def text(value: str | None) -> str:
    return (value or "").strip()


def option_groups(path: Path) -> list[list[str]]:
    """Return option-token groups from a help snapshot.

    A group is one help line, so aliases such as ``-e/--error-rate`` stay
    together. Values such as ``-1`` are not options and are ignored.
    """
    groups: list[list[str]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.lstrip().startswith("-"):
            continue
        # Stop at the description column; otherwise value/description text can
        # introduce unrelated option-looking tokens (for example "-U" in the
        # -u/--cut description).
        spec = re.split(r"\s{2,}", line.strip(), maxsplit=1)[0]
        tokens = re.findall(r"(?<![\w-])(--?[A-Za-z][A-Za-z0-9_-]*)", spec)
        tokens = list(dict.fromkeys(tokens))
        if tokens:
            groups.append(tokens)
    return groups


def main() -> int:
    rows: list[dict[str, str]] = []
    keys: set[tuple[str, str]] = set()

    def add(
        tool: str,
        surface: str,
        kind: str,
        status: str,
        evidence: str,
        deviation: str = "",
        next_action: str = "",
    ) -> None:
        key = (tool, surface)
        if key in keys:
            return
        keys.add(key)
        rows.append(
            {
                "tool": tool,
                "upstream": UPSTREAM_NAMES[tool],
                "surface": surface,
                "kind": kind,
                "status": status,
                "evidence": evidence,
                "deviation": deviation,
                "next_action": next_action,
            }
        )

    # cutadapt: the pinned 4.6 help is the complete option surface. All options
    # are covered by the 119/119 differential suite.
    for tokens in option_groups(UPSTREAM / "cutadapt.help.txt"):
        add(
            "rust-cutadapt",
            " ".join(tokens),
            "option",
            "verified",
            "cutadapt 4.6: 119/119 differential; rust-cutadapt/docs/FULL-COMPATIBILITY.md",
        )
    for surface, evidence in [
        ("report:text", "119/119 differential; text report cases"),
        ("report:json", "119/119 differential; JSON schema cases"),
        ("compression:gzip/bzip2/xz", "differential and conformance fixtures"),
        ("paired/interleaved", "119/119 differential; paired cases"),
        ("determinism:thread-count", "tests/cli_e2e.rs parallel/single-thread"),
        ("performance:5x-target", "rust-cutadapt/docs/FULL-COMPATIBILITY.md measured 6.60x"),
    ]:
        add("rust-cutadapt", surface, "feature", "verified", evidence)

    # UMI-tools: import the existing component matrix, preserving its evidence
    # and explicit unsupported/experimental classifications.
    umi_status = {
        "verified": "verified",
        "experimental": "experimental",
        "unsupported": "unsupported",
        "extension": "verified",
    }
    with (ROOT / "rust-umitools" / "docs" / "compatibility.tsv").open(
        encoding="utf-8", newline=""
    ) as handle:
        for record in csv.DictReader(handle, delimiter="\t"):
            command = text(record.get("command"))
            option = text(record.get("option"))
            if not option:
                continue
            surface = f"{command}:{option}" if command != "all" else f"global:{option}"
            raw_status = text(record.get("status"))
            status = umi_status[raw_status]
            evidence = "; ".join(
                part
                for part in (
                    text(record.get("oracle")),
                    text(record.get("fixture")),
                    text(record.get("comparison")),
                )
                if part
            )
            if raw_status == "extension":
                evidence = f"{evidence}; extension, not upstream".lstrip("; ")
            next_action = (
                ""
                if status == "verified"
                else "T4: close with upstream evidence or re-scope"
            )
            if command == "dedup" and option == "--gene-transcript-map":
                next_action = "Accepted upstream nondeterminism; retain experimental"
            add(
                "rust-umitools",
                surface,
                "feature",
                status,
                evidence or "rust-umitools/docs/compatibility.tsv",
                text(record.get("note")),
                next_action,
            )
    for command, evidence in [
        ("whitelist", "oracle whitelist cases; exact 9"),
        ("extract", "oracle extract 11; exact cases"),
        ("dedup", "exact 77/77 and semantic oracle cases"),
        ("group", "exact group cases + oracle C2"),
        ("count", "exact count/count_tab cases + oracle"),
        ("count_tab", "exact count_tab cases"),
    ]:
        add("rust-umitools", command, "command", "verified", evidence)
    add(
        "rust-umitools",
        "global:--help/--version",
        "option",
        "verified",
        "compat/current/rust-umitools.help.txt",
    )

    # samtools: all top-level commands are visible. Implemented commands are
    # verified; every other command is an explicit T5 row.
    samtools_implemented = {
        "view",
        "flagstat",
        "idxstats",
        "quickcheck",
        "index",
        "sort",
        "depth",
        "flags",
        "head",
    }
    samtools_experimental = {"coverage"}
    samtools_commands: list[str] = []
    in_commands = False
    for line in (UPSTREAM / "samtools.help.txt").read_text(encoding="utf-8").splitlines():
        if line.strip() == "Commands:":
            in_commands = True
            continue
        if not in_commands:
            continue
        match = re.match(r"^\s{2,}([a-z][a-z0-9-]+)\s+", line)
        if match and match.group(1) not in {"help", "version"}:
            samtools_commands.append(match.group(1))
    for command in dict.fromkeys(samtools_commands):
        if command in samtools_implemented:
            add(
                "rust-bam",
                command,
                "command",
                "verified",
                (
                    "rust-bam/tests/flags.rs + samtools 1.24 29/29 differential"
                    if command == "flags"
                    else (
                        "rust-bam/tests/head.rs + samtools 1.24 36/36 exact file/stdin differential"
                        if command == "head"
                        else "target/release/rust-bam + tests/view_filters.rs"
                    )
                ),
            )
        elif command in samtools_experimental:
            add(
                "rust-bam",
                command,
                "command",
                "experimental",
                "rust-bam/tests/depth.rs or tests/coverage.rs + samtools 1.24 manual diff",
                "subset; coverage: all current tabular/histogram options implemented; CRAM unsupported; -r is sequential",
                "T5: expand depth/coverage options",
            )
        else:
            add(
                "rust-bam",
                command,
                "command",
                "unsupported",
                "absent from compat/current/rust-bam.help.txt",
                "samtools command not implemented",
                "T5: implement with samtools differential",
            )
    for surface, evidence in [
        ("view:filters -f/-F/-q", "tests/view_filters.rs + samtools 1.24 differential"),
        ("view:--region BAI/CSI", "tests/view_filters.rs + samtools differential"),
        ("index:--csi", "tests/view_filters.rs"),
        ("sort:external-memory", "tests/external_sort.rs + samtools differential"),
    ]:
        add("rust-bam", surface, "feature", "verified", evidence)
    add(
        "rust-bam",
        "depth:basic -a/-aa/-s/-f/-g/-G/--incl-flags/--require-flags/-H/-q/-Q/-l/-J/-r/-b/-o",
        "feature",
        "verified",
        "rust-bam/tests/depth.rs (10 tests) + samtools 1.24 32/32 -a/-aa/-s + 35/35 flag differential",
        "missing -X/CRAM; -r is sequential; BTreeMap memory is covered positions",
        "T5: custom index and CRAM support",
    )
    add(
        "rust-bam",
        "coverage:basic -b/--min-depth/-r/-q/-Q/-l/-H/-o/--rf/--ff/-d/-m/-D/-w/-A/multiple inputs",
        "feature",
        "verified",
        "rust-bam/tests/coverage.rs (10 tests) + samtools 1.24 57/57 histogram/depth/region/width differential + 35/35 flag differential",
        "CRAM unsupported; -r is sequential; BTreeMap memory is covered positions",
        "T5: CRAM support or re-scope",
    )

    # FastQC: all pinned CLI options plus report artifacts and modules.
    for tokens in option_groups(UPSTREAM / "fastqc.help.txt"):
        surface = " ".join(tokens)
        if "--adapters" in tokens:
            add(
                "rust-fastqc", surface, "option", "exception",
                "tests/oracle/fastqc_adapters_diff.py: 35 exact + 6 counting + 3 invalid-config cases; read-order invariant; 6 process/library tests",
                "FASTQC-ADAPTER-CORRECTNESS: native custom lists; equal-length hits are counted, independent of read order; empty lists fail before output",
                "Maintain adapter differential and independent boundary/order proof",
            )
            continue
        if "--java" in tokens or "--debug" in tokens:
            add(
                "rust-fastqc", surface, "option", "exception",
                "rust-fastqc/tests/native_runtime.rs; docs/EVIDENCE-UPSTREAM-DEFECTS.md",
                "FASTQC-NATIVE-LAUNCHER: JVM launcher controls explicitly rejected with exit 2; native QC does not execute Java",
                "Maintain native-runtime regression and explicit approved exception",
            )
            continue
        if "--outdir" in tokens or "--phred64" in tokens:
            status = "verified"
        elif (
            "--extract" in tokens
            or "--delete" in tokens
            or "--min_length" in tokens
            or "--max_length" in tokens
            or "--dup_length" in tokens
            or "--nogroup" in tokens
            or "--quiet" in tokens
        ):
            status = "verified"
        elif "--contaminants" in tokens:
            status = "verified"
        elif "-h" in tokens or "--help" in tokens or "-v" in tokens or "--version" in tokens:
            status = "verified"
        else:
            status = "unsupported"
        if "--extract" in tokens or "--delete" in tokens:
            evidence = "FastQC 0.13.0 default/extract/delete layout differential (4/4)"
        elif "--contaminants" in tokens:
            evidence = "FastQC 0.13.0 default-list 22/22 + custom source 7/7 differential"
        elif "--quiet" in tokens:
            evidence = "FastQC 0.13.0 quiet/progress behavior and artifact smoke"
        elif "--min_length" in tokens or "--max_length" in tokens or "--dup_length" in tokens or "--nogroup" in tokens:
            evidence = "FastQC 0.13.0 5/5 option differential; rust-fastqc 20 integration tests"
        else:
            evidence = (
                "docs/EVIDENCE-QC.md; 22/22 fixtures"
                if status != "unsupported"
                else "absent from compat/current/rust-fastqc.help.txt"
            )
        deviation = (
            "custom name<TAB>sequence lists replace the built-in FastQC 0.13.0 list"
            if "--contaminants" in tokens
            else ""
        )
        add(
            "rust-fastqc",
            surface,
            "option",
            status,
            evidence,
            deviation,
            "" if status == "verified" else "T6: implement or re-scope",
        )
    for surface, status, evidence, next_action in [
        ("report:fastqc_data.txt", "verified", "22 QC fixtures compare nine modules (Adapter Content excluded); separate adapter proof; MultiQC parses data", ""),
        (
            "report:HTML",
            "experimental",
            "data-table HTML with every module; no plot images or FastQC auxiliary assets",
            "T6: implement plots or retain the documented data-table scope",
        ),
        (
            "report:ZIP",
            "experimental",
            "stored ZIP with data, summary and HTML; no plot images or FastQC auxiliary assets",
            "T6: implement plots or retain the documented data-table scope",
        ),
        (
            "module:adapter-content",
            "exception",
            "tests/oracle/fastqc_adapters_diff.py: 44 cases; default/custom lists, short-read warnings and independent read-order proof",
            "Maintain adapter differential and independent boundary/order proof",
        ),
        (
            "module:kmer-content",
            "unsupported",
            "rust-fastqc/src/absent.rs; FastQC 0.13.0 KmerContent module is not implemented",
            "T6: implement position-specific enrichment before enabling via --limits",
        ),
        (
            "module:per-tile-quality",
            "unsupported",
            "rust-fastqc/src/absent.rs; FastQC 0.13.0 PerTileQualityScores module is not implemented",
            "T6: implement header/tile aggregation before enabling via --limits",
        ),
        (
            "module:contaminants",
            "verified",
            "default FastQC 0.13.0 contaminant list plus custom overrides; 22/22 baseline + 7/7 custom source differential",
            "",
        ),
        (
            "module:custom-contaminants",
            "verified",
            "rust-fastqc/src/contaminants.rs + FastQC 0.13.0 7/7 source differential; forward/reverse/longest/one-mismatch cases",
            "",
        ),
    ]:
        add(
            "rust-fastqc",
            surface,
            "report" if surface.startswith("report:") else "feature",
            status,
            evidence,
            "FASTQC-ADAPTER-CORRECTNESS: correct equal-length/read-order defect and reject empty config before report creation" if surface == "module:adapter-content" else "",
            next_action,
        )

    # SeqKit: all top-level commands. Each implemented command has a direct
    # command-specific differential or an explicit remaining deviation.
    fx_implemented = {"stats", "seq", "fq2fa", "fx2tab", "sample", "split"}
    seqkit_commands: list[str] = []
    in_commands = False
    for line in (UPSTREAM / "seqkit.help.txt").read_text(encoding="utf-8").splitlines():
        if line.startswith("Commands for ") or line.startswith("Additional Commands"):
            in_commands = True
            continue
        if line.startswith("Flags:"):
            break
        if not in_commands:
            continue
        match = re.match(r"^\s{2,}([a-z][a-z0-9-]+)\s+", line)
        if match and match.group(1) not in {"seqkit", "version"}:
            seqkit_commands.append(match.group(1))
    for command in dict.fromkeys(seqkit_commands):
        if command == "head":
            add(
                "rust-fx",
                "seqkit:head",
                "command",
                "verified",
                "rust-fx/tests/head.rs (5 tests) + SeqKit 2.14 10/10 head differential + compression tests",
                "",
                "",
            )
        elif command == "grep":
            add(
                "rust-fx",
                "seqkit:grep",
                "command",
                "verified",
                "rust-fx/tests/grep.rs (7 tests) + SeqKit 2.14 33/33 grep differential + compression tests",
                "",
                "",
            )
        elif command == "sort":
            add(
                "rust-fx",
                "seqkit:sort",
                "command",
                "experimental",
                "rust-fx/tests/sort.rs (7 tests) + SeqKit 2.14 direct natural/prefix/two-pass/index differential",
                "two-pass by-seq is broken in upstream 2.14; compressed two-pass is unsupported",
                "T7: complete sort options and bounded-memory mode",
            )
        elif command == "subseq":
            add(
                "rust-fx",
                "seqkit:subseq",
                "command",
                "experimental",
                "rust-fx/tests/subseq*.rs + SeqKit 2.14 9/9 multi-file BED/GTF/flank differential",
                "records are buffered per input; -U is unsupported",
                "T7: complete subseq BED/GTF/flank semantics",
            )
        elif command == "range":
            add(
                "rust-fx",
                "seqkit:range",
                "command",
                "exception",
                "tests/oracle/check_seqkit_exceptions.py: 203/206 exact plus independent provenance and composition proof",
                "SEQKIT-RANGE-QUALITY: preserve input formats; upstream manufactures invalid/stale FASTQ quality for FASTA (3 raw differences)",
                "Maintain independent correctness proof and visible raw differences",
            )
        elif command == "sliding":
            add(
                "rust-fx",
                "seqkit:sliding",
                "command",
                "verified",
                "rust-fx/tests/sliding.rs (4 tests) + SeqKit 2.14 7/7 circular/greedy/global differential + compression tests",
                "",
                "",
            )
        elif command == "duplicate":
            add(
                "rust-fx",
                "seqkit:duplicate",
                "command",
                "verified",
                "rust-fx/tests/duplicate.rs + 3/3 repeated records SeqKit 2.14 diff",
                "",
                "",
            )
        elif command == "rmdup":
            add(
                "rust-fx",
                "seqkit:rmdup",
                "command",
                "verified",
                "rust-fx/tests/rmdup.rs (3 tests) + SeqKit 2.14 duplicate-report differential + compression tests",
                "",
                "",
            )
        elif command == "faidx":
            add(
                "rust-fx",
                "seqkit:faidx",
                "command",
                "experimental",
                "rust-fx/tests/faidx.rs (4 tests) + SeqKit 2.14 region/regex/index differential",
                "regex output follows input order; upstream Go map order is unstable",
                "T7: implement faidx index side effects and regex IDs",
            )
        elif command == "rename":
            add(
                "rust-fx",
                "seqkit:rename",
                "command",
                "verified",
                "rust-fx/tests/rename.rs (3 tests) + SeqKit 2.14 multiple-outfiles/force differential + compression tests",
                "",
                "",
            )
        elif command == "replace":
            add(
                "rust-fx",
                "seqkit:replace",
                "command",
                "experimental",
                "rust-fx/tests/replace.rs (4 tests) + SeqKit 2.14 15/15 special-symbol/filter differential",
                "UUID is checked for shape and uniqueness, not byte equality",
                "T7: complete replace filters and special symbols",
            )
        elif command == "tab2fx":
            add(
                "rust-fx",
                "seqkit:tab2fx",
                "command",
                "verified",
                "rust-fx/tests/tab2fx.rs + seqkit_streaming.rs; tests/oracle/seqkit_streaming_diff.py --command tab2fx: 99/99",
                "diagnostic prefix normalized; successful compressed output compared decoded; incomplete compressed byte prefixes are backend-dependent",
                "",
            )
        elif command == "restart":
            add(
                "rust-fx",
                "seqkit:restart",
                "command",
                "verified",
                "rust-fx/tests/restart.rs (2 tests) + SeqKit 2.14 start-with forward/reverse/mismatch differential + compression tests",
                "",
                "",
            )
        elif command == "fa2fq":
            add(
                "rust-fx",
                "seqkit:fa2fq",
                "command",
                "verified",
                "rust-fx/tests/fa2fq.rs (3 tests) + SeqKit 2.14 forward/reverse subsequence and quality differential + compression tests",
                "",
                "",
            )
        elif command == "translate":
            add(
                "rust-fx",
                "seqkit:translate",
                "command",
                "exception",
                "tests/oracle/check_seqkit_exceptions.py: 710/756 exact + 46 hash-checked differences; independent IUPAC/ORF/trim/header proof",
                "SEQKIT-TRANSLATE-CORRECTNESS: correct documented upstream strand, ORF endpoint, right-trim and description defects",
                "Maintain independent correctness proof and reviewed raw observation hashes",
            )
        elif command == "common":
            add(
                "rust-fx",
                "seqkit:common",
                "command",
                "verified",
                "rust-fx/tests/common.rs (8 tests) + 14/14 ID/name/sequence/embedded SeqKit 2.14 diff",
                "none observed on the command-specific surface (-n/-s/-e/-i/-P/-w/-o)",
                "",
            )
        elif command in fx_implemented:
            status = "verified"
            evidence = "50/50 W1-3 FASTX differential plus component tests"
            deviation = "selected surface; other global flags are not covered"
            next_action = ""
            add(
                "rust-fx",
                f"seqkit:{command}",
                "command",
                status,
                evidence,
                deviation,
                next_action,
            )
        else:
            add(
                "rust-fx",
                f"seqkit:{command}",
                "command",
                "unsupported",
                "absent from compat/current/rust-fx.help.txt",
                "",
                "T7: implement selected SeqKit command",
            )
    add(
        "rust-fx",
        "global:--compress-level",
        "feature",
        "verified",
        "rust-fx/tests/compression.rs (3 tests) + SeqKit 2.14 gzip/zstd/bzip2/xz level differential",
        "decompressed records match; compressed bytes use different backends",
        "",
    )
    add(
        "rust-fx",
        "head:basic -n/-l/-X/global raw-output flags",
        "feature",
        "verified",
        "rust-fx/tests/head.rs (5 tests) + SeqKit 2.14 10/10 head differential + compression tests",
        "",
        "",
    )
    add(
        "rust-fx",
        "head:global -X/--id-regexp/--id-ncbi/--seq-type/--quiet/--skip-file-check",
        "feature",
        "verified",
        "rust-fx/tests/head.rs infile-list/global test + SeqKit 2.14 10/10 head differential",
        "",
    )
    add(
        "rust-fx",
        "fa2fq:basic -f/-P/subsequence/quality",
        "feature",
        "verified",
        "rust-fx/tests/fa2fq.rs (3 tests) + SeqKit 2.14 forward/reverse subsequence and quality differential + compression tests",
        "",
        "",
    )
    add(
        "rust-fx",
        "faidx:index-and-regexp -r/-f/-U/.fai/.seqkit.fai",
        "feature",
        "verified",
        "rust-fx/tests/faidx.rs (4 tests) + SeqKit 2.14 region/regex/index differential",
        "regex output follows input order; upstream Go map order is unstable",
        "Accepted upstream order limitation; retain selected-surface scope",
    )
    add(
        "rust-fx",
        "grep:basic -p/-f/-n/-s/-v/-C/-i/-r/-d/-R/-P/-m/-D/-c",
        "feature",
        "verified",
        "rust-fx/tests/grep.rs (7 tests) + SeqKit 2.14 33/33 grep differential + compression tests",
        "",
        "",
    )
    add(
        "rust-fx",
        "grep:advanced --delete-matched/-I/-X/--id-regexp/--quiet",
        "feature",
        "verified",
        "rust-fx/tests/grep.rs + SeqKit 2.14 33/33 grep differential + compression tests",
        "",
    )
    add(
        "rust-fx",
        "rename:multiple-outfiles -m/-O/-f",
        "feature",
        "verified",
        "rust-fx/tests/rename.rs + SeqKit 2.14 multiple-outfiles/force differential + compression tests",
        "",
        "",
    )
    add(
        "rust-fx",
        "subseq:basic -r/--bed/--gtf/-u/-d/-f/--feature/--chr",
        "feature",
        "verified",
        "rust-fx/tests/subseq*.rs + SeqKit 2.14 9/9 multi-file BED/GTF/flank differential",
        "records are buffered per input; -U is unsupported",
        "T7: use FASTA index or document the memory bound",
    )
    add(
        "rust-fx",
        "subseq:multi-file BED/GTF/flank/gene-tag",
        "feature",
        "verified",
        "rust-fx/tests/subseq_gtf_flank.rs + SeqKit 2.14 9/9 multi-file differential including gene_id",
        "records are buffered per input; -U is unsupported",
    )
    add(
        "rust-fx",
        "translate:basic -f/-F/-s/--clean/-m/-e/-x/-M",
        "feature",
        "exception",
        "rust-fx/tests/translate*.rs (17 tests); tests/oracle/check_seqkit_exceptions.py",
        "SEQKIT-TRANSLATE-CORRECTNESS: full IUPAC/RNA complement, exact ORF nucleotide spans, right-end trim, lossless descriptions",
        "Maintain independent correctness proof and reviewed raw observation hashes",
    )
    add(
        "rust-fx",
        "translate:tables 1-6,9-14,16,21-31",
        "feature",
        "verified",
        "tests/oracle/seqkit_translate_diff.py: all 3375 IUPAC codons in each of 24 tables, plus initiation and six-frame modes",
        "unsupported IDs fail before output",
    )
    add(
        "rust-fx",
        "translate:listing -l/-L",
        "feature",
        "verified",
        "rust-fx/tests/translate.rs; tests/oracle/seqkit_translate_diff.py: 54 listing/precedence cases",
        "unknown positive IDs return an empty successful listing",
        "",
    )
    add(
        "rust-fx",
        "replace:special-and-filters {nr}/{fn}/{fbn}/{fbne}/{kv}/f-*",
        "feature",
        "verified",
        "rust-fx/tests/replace.rs (4 tests) + SeqKit 2.14 15/15 special-symbol/filter differential + UUID shape",
        "--compress-level is covered; UUID is not byte-compared",
    )
    add(
        "rust-fx",
        "tab2fx:comment-prefix -p",
        "feature",
        "verified",
        "rust-fx/tests/tab2fx.rs + SeqKit 2.14 CSV/repeated/empty/CRLF prefix differential in seqkit_streaming_diff.py",
        "invalid CSV option diagnostics use the Rust CLI format",
        "",
    )
    add(
        "rust-fx",
        "tab2fx:buffer-size -b",
        "feature",
        "verified",
        "rust-fx/tests/tab2fx.rs + seqkit_streaming_diff.py: fractional/lowercase units, EOF/newline boundaries, early/late failure",
        "64 KiB pending output is discarded on failure, not a whole-file pre-scan; non-finite/overflow buffer sizes are rejected",
        "",
    )
    add(
        "rust-fx",
        "range:streaming positive/negative/multi-file",
        "feature",
        "exception",
        "rust-fx/tests/range.rs + seqkit_streaming.rs; tests/oracle/check_seqkit_exceptions.py",
        "SEQKIT-RANGE-QUALITY: START:-1 streams with original record formats and qualities; no upstream cross-file quality contamination",
        "Maintain independent correctness proof and visible raw differences",
    )
    add(
        "rust-fx",
        "sliding:basic -W/-s/-S/-g/-c/-C/-X",
        "feature",
        "verified",
        "rust-fx/tests/sliding.rs (4 tests) + SeqKit 2.14 7/7 circular/greedy/global differential + compression tests",
        "",
    )
    add(
        "rust-fx",
        "rmdup:duplicate-reports -D/-d",
        "feature",
        "verified",
        "rust-fx/tests/rmdup.rs duplicate reports + SeqKit 2.14 duplicate-report differential + compression tests",
        "",
        "",
    )
    add(
        "rust-fx",
        "restart:start-with -s/-m/-I",
        "feature",
        "verified",
        "rust-fx/tests/restart.rs (2 tests) + SeqKit 2.14 start-with forward/reverse/mismatch differential + compression tests",
        "",
    )
    add(
        "rust-fx",
        "sort:basic -l/-n/-s/-b/-r/-i/-G/-N/-L",
        "feature",
        "verified",
        "rust-fx/tests/sort.rs (7 tests) + SeqKit 2.14 natural/prefix/two-pass/index differential",
        "two-pass by-seq is upstream-broken in 2.14; compressed two-pass is unsupported",
        "T7: document upstream defect or add a separate compatibility mode",
    )
    add(
        "rust-fx",
        "sort:two-pass -2/-U/-k (default/by-name/by-length)",
        "feature",
        "verified",
        "rust-fx/tests/sort.rs + SeqKit 2.14 two-pass/index and multi-input rejection differential",
        "two-pass by-seq is upstream-broken in 2.14; compressed two-pass unsupported",
    )
    for command, status, evidence, next_action in [
        ("lint", "verified", "50/50 W1-3 differential; fq 0.12.0 validator cases", ""),
        ("filter", "unsupported", "absent from compat/current/rust-fx.help.txt", "T7: implement or re-scope"),
        ("generate", "unsupported", "absent from compat/current/rust-fx.help.txt", "T7: implement or re-scope"),
        ("subsample", "unsupported", "absent from compat/current/rust-fx.help.txt", "T7: implement or re-scope"),
    ]:
        add("rust-fx", f"fq:{command}", "command", status, evidence, "", next_action)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=FIELDS,
            delimiter="\t",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)
    print(f"wrote {OUT.relative_to(ROOT)} with {len(rows)} rows")
    print("tools:", dict(Counter(row["tool"] for row in rows)))
    print("status:", dict(Counter(row["status"] for row in rows)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
