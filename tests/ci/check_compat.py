#!/usr/bin/env python3
"""Validate the unified compatibility matrix and its pinned upstream surface.

The matrix is intentionally conservative: every top-level upstream command is
represented, and every option in the cutadapt/FastQC help snapshots has a row.
New upstream help text or an unclassified gap fails this check until the matrix
is updated deliberately.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
COMPAT = ROOT / "compat"
UPSTREAM = COMPAT / "upstream"
TSV = COMPAT / "matrix.tsv"
JSON_PATH = COMPAT / "matrix.json"

FIELDS = [
    "tool",
    "upstream",
    "surface",
    "kind",
    "status",
    "unachievable",
    "evidence",
    "deviation",
    "next_action",
]

TARGETS = {
    "rust-cutadapt": ("cutadapt 4.6", "cutadapt.version.txt", "4.6"),
    "rust-umitools": (
        "UMI-tools 1.1.6",
        "umi_tools.version.txt",
        "UMI-tools version: 1.1.6",
    ),
    "rust-bam": ("samtools 1.24", "samtools.version.txt", "samtools 1.24"),
    "rust-fastqc": ("FastQC 0.13.0", "fastqc.version.txt", "FastQC v0.13.0"),
    "rust-fx": (
        "SeqKit 2.14.0 / fq 0.12.0",
        "seqkit.version.txt",
        "seqkit v2.14.0",
    ),
}

ALLOWED_STATUS = {"verified", "experimental", "unsupported", "exception"}
ALLOWED_KIND = {"command", "option", "feature", "report"}


def read_tsv() -> list[dict[str, str]]:
    with TSV.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if reader.fieldnames != FIELDS:
            raise SystemExit(f"unexpected matrix header: {reader.fieldnames!r}")
        return [{key: (value or "") for key, value in row.items()} for row in reader]


def option_groups(path: Path) -> list[list[str]]:
    """Extract option-token groups from a help snapshot (same rule as builder)."""
    groups: list[list[str]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.lstrip().startswith("-"):
            continue
        spec = re.split(r"\s{2,}", line.strip(), maxsplit=1)[0]
        tokens = re.findall(r"(?<![\w-])(--?[A-Za-z][A-Za-z0-9_-]*)", spec)
        tokens = list(dict.fromkeys(tokens))
        if tokens:
            groups.append(tokens)
    return groups


def commands_after(path: Path, start: str, stop: str | None = None) -> list[str]:
    """Extract command names from the first matching help section."""
    commands: list[str] = []
    active = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if not active:
            if line.strip() == start or line.startswith(start):
                active = True
            continue
        if stop and line.startswith(stop):
            break
        match = re.match(r"^\s{2,}([a-z][a-z0-9-]+)\s+", line)
        if match and match.group(1) not in {"help", "version"}:
            commands.append(match.group(1))
    return list(dict.fromkeys(commands))


def top_level_commands(path: Path) -> list[str]:
    """Extract command names from a top-level help snapshot."""
    if path.name == "seqkit.help.txt":
        return commands_after(path, "Commands for ", "Flags:")
    return commands_after(path, "Commands:", "Options:")


def umi_commands(path: Path) -> list[str]:
    return re.findall(r"^\s+- ([a-z][a-z0-9_]+)$", path.read_text(encoding="utf-8"), re.MULTILINE)


def fq_commands(path: Path) -> list[str]:
    return top_level_commands(path)


def has_surface(rows: list[dict[str, str]], tool: str, surface: str) -> bool:
    return any(row["tool"] == tool and row["surface"] == surface for row in rows)


def check_exceptions(rows: list[dict[str, str]]) -> list[str]:
    """An exception is an explicit approved difference, never implied support."""
    registry = COMPAT / "approved-exceptions.json"
    try:
        policy = json.loads(registry.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return [f"approved-exceptions.json: {error}"]
    errors: list[str] = []
    if policy.get("schema_version") != 1 or not policy.get("decision_quote", "").strip():
        errors.append("approved-exceptions.json: missing schema or explicit decision")
    declared: set[tuple[str, str]] = set()
    ids: set[str] = set()
    constrained: dict[tuple[str, str], str] = {}
    for entry in policy.get("exceptions", []):
        identifier = entry.get("id", "")
        if not identifier or identifier in ids:
            errors.append(f"invalid or duplicate exception id: {identifier!r}")
        ids.add(identifier)
        if entry.get("category") not in {"upstream-defect", "native-runtime"} or not entry.get("rule", "").strip():
            errors.append(f"{identifier}: exception needs category and behavior rule")
        for field in ("proof", "verifier"):
            path = entry.get(field, "")
            if not path or not (ROOT / path).is_file():
                errors.append(f"{identifier}: missing {field}: {path}")
        reason = entry.get("unachievable", "")
        if not isinstance(reason, str) or (reason and entry.get("category") != "native-runtime"):
            errors.append(f"{identifier}: unachievable needs an approved native-runtime constraint, not a bug or missing implementation")
        if entry.get("category") == "native-runtime" and (not isinstance(reason, str) or not reason.strip()):
            errors.append(f"{identifier}: native-runtime exception must state its unachievable constraint")
        if not entry.get("surfaces"):
            errors.append(f"{identifier}: exception must name at least one surface")
        for tool, surface in entry.get("surfaces", []):
            key = (tool, surface)
            if isinstance(reason, str) and reason:
                constrained[key] = reason
            if key in declared:
                errors.append(f"duplicate exception surface: {key}")
            declared.add(key)
    actual = {(row["tool"], row["surface"]) for row in rows if row["status"] == "exception"}
    for key in sorted(actual - declared):
        errors.append(f"{key}: exception lacks explicit approval and proof registration")
    for key in sorted(declared - actual):
        errors.append(f"{key}: approved deviation must remain visible as an exception")
    for row in rows:
        key = (row["tool"], row["surface"])
        if row.get("unachievable", "") != constrained.get(key, ""):
            errors.append(f"{key}: unachievable must match its registered constraint; unfinished work is not unachievable")
        if row.get("unachievable") and row["status"] != "exception":
            errors.append(f"{key}: unachievable must remain an explicit exception")
        if row["status"] == "exception" and not row["deviation"].strip():
            errors.append(f"{(row['tool'], row['surface'])}: exception needs an explicit deviation")
    return errors


def check_surface(rows: list[dict[str, str]]) -> list[str]:
    errors: list[str] = check_exceptions(rows)
    keys = set()
    seen = set()
    for row in rows:
        key = (row["tool"], row["surface"])
        if key in seen:
            errors.append(f"duplicate surface: {key}")
        seen.add(key)
        if row["status"] not in ALLOWED_STATUS:
            errors.append(f"{key}: invalid status {row['status']!r}")
        if row["kind"] not in ALLOWED_KIND:
            errors.append(f"{key}: invalid kind {row['kind']!r}")
        if not row["evidence"].strip():
            errors.append(f"{key}: empty evidence")
        if row["status"] != "verified" and not row["next_action"].strip():
            errors.append(f"{key}: non-verified row needs next_action")
        keys.add(row["tool"])

    missing_tools = set(TARGETS) - keys
    if missing_tools:
        errors.append(f"tools missing from matrix: {sorted(missing_tools)}")
    extra_tools = keys - set(TARGETS)
    if extra_tools:
        errors.append(f"unexpected tools in matrix: {sorted(extra_tools)}")

    for tool, (upstream, version_file, marker) in TARGETS.items():
        version = (UPSTREAM / version_file).read_text(encoding="utf-8")
        if marker not in version:
            errors.append(f"{version_file}: missing expected marker {marker!r}")
        for row in rows:
            if row["tool"] == tool and row["upstream"] != upstream:
                errors.append(f"{tool}: upstream {row['upstream']!r} != {upstream!r}")
                break

    # Every top-level command in the upstream snapshots must be represented.
    command_surfaces = {
        "rust-bam": ("samtools.help.txt", ""),
        "rust-fx": ("fq.help.txt", "fq:"),
        "rust-umitools": ("umi_tools.help.txt", ""),
    }
    for tool, (source, prefix) in command_surfaces.items():
        for command in top_level_commands(UPSTREAM / source):
            if not has_surface(rows, tool, prefix + command):
                errors.append(f"{tool}: missing command surface {prefix + command}")

    for command in umi_commands(UPSTREAM / "umi_tools.help.txt"):
        if not has_surface(rows, "rust-umitools", command):
            errors.append(f"rust-umitools: missing UMI command {command}")

    # SeqKit commands are prefixed to distinguish them from fq commands.
    for command in top_level_commands(UPSTREAM / "seqkit.help.txt"):
        if command == "version":
            continue
        if not has_surface(rows, "rust-fx", f"seqkit:{command}"):
            errors.append(f"rust-fx: missing SeqKit command seqkit:{command}")

    # cutadapt/FastQC option tokens must all occur in a row. This catches a
    # help snapshot changing while the matrix stays stale.
    for tool, source in (
        ("rust-cutadapt", "cutadapt.help.txt"),
        ("rust-fastqc", "fastqc.help.txt"),
    ):
        surfaces = [row["surface"] for row in rows if row["tool"] == tool]
        for group in option_groups(UPSTREAM / source):
            for token in group:
                if not any(re.search(rf"(?<![\w-]){re.escape(token)}(?![\w-])", surface) for surface in surfaces):
                    errors.append(f"{tool}: help option not represented: {token}")

    # Current CLI commands must be classified as implemented, not unsupported.
    for tool, source in (
        ("rust-bam", "rust-bam.help.txt"),
        ("rust-fx", "rust-fx.help.txt"),
        ("rust-umitools", "rust-umitools.help.txt"),
    ):
        for command in top_level_commands(COMPAT / "current" / source):
            if tool == "rust-fx":
                surface = "fq:lint" if command == "lint" else f"seqkit:{command}"
            else:
                surface = command
            row = next((r for r in rows if r["tool"] == tool and r["surface"] == surface), None)
            if row is None:
                errors.append(f"{tool}: current command {surface} missing from matrix")
            elif row["status"] == "unsupported":
                errors.append(f"{tool}: current command {surface} is classified unsupported")

    return errors


def snapshot_hashes() -> dict[str, str]:
    return {
        path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(UPSTREAM.glob("*"))
        if path.is_file()
    }


def document(rows: list[dict[str, str]]) -> dict:
    return {
        "schema_version": 3,
        "targets": {tool: target[0] for tool, target in TARGETS.items()},
        "snapshots": snapshot_hashes(),
        "rows": rows,
    }


def wire(rows: list[dict[str, str]]) -> None:
    JSON_PATH.write_text(
        json.dumps(document(rows), indent=2, sort_keys=False) + "\n",
        encoding="utf-8",
    )


def check_json(rows: list[dict[str, str]]) -> list[str]:
    if not JSON_PATH.is_file():
        return [f"{JSON_PATH.relative_to(ROOT)}: missing; run with --write"]
    try:
        saved = json.loads(JSON_PATH.read_text(encoding="utf-8"))
    except ValueError as error:
        return [f"{JSON_PATH.relative_to(ROOT)}: invalid JSON: {error}"]
    expected = document(rows)
    errors: list[str] = []
    if saved.get("schema_version") != 3:
        errors.append("matrix.json: schema_version must be 3 (unachievable constraint column)")
    if saved.get("targets") != expected["targets"]:
        errors.append("matrix.json: targets are stale")
    if saved.get("snapshots") != expected["snapshots"]:
        errors.append("matrix.json: snapshot hashes are stale")
    if saved.get("rows") != rows:
        errors.append("matrix.json: rows are stale; run with --write")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="write/refresh matrix.json")
    args = parser.parse_args()

    if not TSV.is_file():
        print(f"ERROR: missing {TSV.relative_to(ROOT)}; run compat/build_matrix.py", file=sys.stderr)
        return 1
    rows = read_tsv()
    errors = check_surface(rows)

    if args.write:
        wire(rows)
        print(f"wrote {JSON_PATH.relative_to(ROOT)} with {len(rows)} rows")
    else:
        errors.extend(check_json(rows))

    if errors:
        for error in errors:
            print(f"FAIL {error}", file=sys.stderr)
        return 1
    print(
        f"PASS compatibility matrix: {len(rows)} rows, "
        f"{len(TARGETS)} tools, {len(snapshot_hashes())} upstream snapshots"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
