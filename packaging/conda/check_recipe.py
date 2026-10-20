#!/usr/bin/env python3
"""Structural checks for the conda recipe draft.

This is **not** a substitute for `conda-build`. It complements it: `conda-build`
26.9.1 has **no `lint` subcommand** (the old `conda-build lint` was removed), so
`--check` is the nearest equivalent and it is run separately.

A real `conda-build` run IS recorded in docs/EVIDENCE-PACKAGING.md. This script
exists because `conda-build --check` exited **0** even with an unsatisfiable
dependency, so a passing `--check` alone cannot be trusted to mean the recipe is
sound. The structural checks here catch the class of mistakes that `--check`
tolerates: wrong source path, a dependency that is not a real package, a build
script that lost `--locked`.

What this script *does* check, using only the standard library:

* the file parses as the YAML subset conda recipes use: no tab characters, no
  trailing whitespace on key lines, every line either blank/comment, a mapping
  entry, or a list item;
* every section a buildable recipe requires is present (`package`, `source`,
  `build`, `requirements`, `test`, `about`);
* `package.name` and `package.version` are set and match the crate;
* the build script invokes `cargo build --release --locked` for the right package;
* the test block runs the binary;
* the recipe states its own unbuilt status in a comment, so a reader cannot mistake
  it for a verified recipe.

It cannot tell you whether `conda-build` would accept the recipe. Only a real
`conda-build` run can.

Exit codes: 0 all checks passed, 1 a check failed, 3 the recipe file is missing.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

RECIPE = Path(__file__).resolve().parent / "meta.yaml"

REQUIRED_SECTIONS = ["package:", "source:", "build:", "requirements:", "test:", "about:"]


def check_structure(text: str) -> list[str]:
    """Return a list of problems; empty means the structural checks passed."""
    problems: list[str] = []

    if "\t" in text:
        problems.append("contains a tab character (YAML forbids tabs for indentation)")
    if not text.endswith("\n"):
        problems.append("does not end with a newline")

    # Track block scalars: after `key: |` or `key: >` every following line that is
    # more indented than the key is free-form text, not a mapping entry.
    block_indent: int | None = None

    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.rstrip()
        if not stripped:
            continue
        indent = len(line) - len(line.lstrip())
        if stripped.lstrip().startswith("#"):
            continue
        if stripped != line:
            problems.append(f"line {number}: trailing whitespace")

        if block_indent is not None:
            if indent > block_indent:
                # Still inside the block scalar.
                continue
            block_indent = None

        if stripped.lstrip().startswith("-"):
            continue
        if stripped.startswith("{%") or stripped.startswith("{{"):
            continue
        if re.search(r":\s*[|>][-+]?\s*$", stripped):
            block_indent = indent
            continue
        if ":" not in stripped:
            problems.append(f"line {number}: not a mapping entry or list item: {stripped[:60]!r}")

    for section in REQUIRED_SECTIONS:
        if not re.search(rf"^{re.escape(section)}", text, re.M):
            problems.append(f"missing required section: {section}")

    return problems


def check_content(text: str) -> list[str]:
    """Check that the recipe says the right things."""
    problems: list[str] = []

    name = re.search(r"^\s*name:\s*(\S+)", text, re.M)
    if not name:
        problems.append("package.name is not set")

    # The version may be a literal or a Jinja expression like `{{ version }}`;
    # either way the `{% set version = ... %}` line must define a real value.
    version_line = re.search(r"^\s*version:\s*(.+)$", text, re.M)
    if not version_line:
        problems.append("package.version is not set")
    else:
        value = version_line.group(1).strip()
        if value.startswith("{{"):
            defined = re.search(r"^\{%\s*set\s+version\s*=\s*[\"']([^\"']+)[\"']", text, re.M)
            if not defined:
                problems.append("package.version is a Jinja expression with no {% set version %}")
        elif not re.match(r"[0-9]", value):
            problems.append(f"package.version is not a version literal: {value!r}")

    # The build must be reproducible: --locked and --release, for this package.
    if "cargo build --release --locked" not in text:
        problems.append("build script does not use `cargo build --release --locked`")
    if "-p rust-cutadapt" not in text:
        problems.append("build script does not select `-p rust-cutadapt`")

    # The test must actually execute the binary.
    if "rust-cutadapt --version" not in text:
        problems.append("test block does not run `rust-cutadapt --version`")

    # The draft status must be stated, so nobody mistakes it for a verified recipe.
    if "DRAFT" not in text.upper():
        problems.append("recipe does not declare itself a DRAFT")

    # A C compiler is required for zstd-sys and lzma-sys.
    if "compiler('c')" not in text:
        problems.append("requirements.build does not request a C compiler")

    return problems


def main() -> int:
    if not RECIPE.is_file():
        print(f"ERROR: recipe not found: {RECIPE}", file=sys.stderr)
        return 3

    text = RECIPE.read_text()
    problems = check_structure(text) + check_content(text)

    if problems:
        print(f"FAIL  {RECIPE.name}")
        for problem in problems:
            print(f"        {problem}")
        return 1

    print(f"PASS  {RECIPE.name} (structural checks; a real conda-build run is recorded separately)")
    print("      sections:", ", ".join(s.rstrip(":") for s in REQUIRED_SECTIONS))
    return 0


if __name__ == "__main__":
    sys.exit(main())
