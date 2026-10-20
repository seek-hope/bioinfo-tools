#!/usr/bin/env python3
"""Generate a deterministic CycloneDX 1.5 SBOM for the workspace from Cargo.lock.

## Why this script exists

No SBOM generator is installed in the environment where packaging was prepared:
`cargo-sbom`, `cargo-cyclonedx`, `cyclonedx-py`, `syft`, `trivy` and `bom` are all
absent. Rather than claim an SBOM from a tool that was never run, this script
produces one from first principles using only the standard library, driven by
`cargo metadata --locked --offline`, which reads the committed `Cargo.lock`.

## Determinism

Two runs on the same `Cargo.lock` produce byte-identical output. Guarantees:

* components are sorted by `(name, version)`;
* every dictionary key is emitted in a fixed order (`json.dumps` with
  `sort_keys=True` at the end, plus an explicit serial number derived from the
  *content*, not from the clock);
* **no timestamps**, no hostnames, no absolute paths, no random UUIDs. CycloneDX
  normally carries a `timestamp` in `metadata`; it is deliberately omitted here
  because a clock value would make every run differ. `metadata` instead carries the
  input hash, which is strictly more useful for verification.
* the `serialNumber` is `urn:uuid:` plus a UUIDv5-derived value computed from the
  SHA-256 of the input `Cargo.lock` and the sorted component list. Same inputs ->
  same serial; different lockfile -> different serial.

## What it records

* every package in the resolved dependency graph, with version, license, and the
  checksum Cargo recorded for it (crates.io packages carry a `checksum` in
  `Cargo.lock`);
* the workspace's own members, marked as such;
* the dependency edges (`dependencies` per component), so the graph is usable for
  vulnerability triage rather than being a flat list.

## Usage

    python3 packaging/sbom.py            # writes packaging/sbom.cdx.json
    python3 packaging/sbom.py --stdout   # writes to stdout
    python3 packaging/sbom.py --check    # verifies the committed SBOM is current

Exit codes: 0 success, 1 `--check` found drift, 3 cargo metadata failed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUT = ROOT / "packaging" / "sbom.cdx.json"

# A fixed namespace for the UUIDv5 serial, so the same inputs always map to the
# same URN on any machine.
SERIAL_NAMESPACE = uuid.UUID("6ba7b811-9dad-11d1-80b4-00c04fd430c8")


def cargo_metadata() -> dict:
    """Run `cargo metadata --locked --offline` and return the parsed JSON."""
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        print(
            "ERROR: cargo metadata failed; cannot build an SBOM without the "
            f"resolved graph:\n{result.stderr}",
            file=sys.stderr,
        )
        raise SystemExit(3)
    return json.loads(result.stdout)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def lock_checksums(lock: Path) -> dict[tuple[str, str], str]:
    """Extract `(name, version) -> checksum` from Cargo.lock.

    `cargo metadata` does NOT expose the `checksum` field (it comes back as None for
    every package), so the only authoritative source is the lockfile itself. Parsing
    it is a handful of lines and avoids a TOML dependency.
    """
    checksums: dict[tuple[str, str], str] = {}
    name = None
    version = None
    for line in lock.read_text().splitlines():
        stripped = line.strip()
        if stripped == "[[package]]":
            name = version = None
        elif stripped.startswith("name = "):
            name = stripped.split("=", 1)[1].strip().strip('"')
        elif stripped.startswith("version = "):
            version = stripped.split("=", 1)[1].strip().strip('"')
        elif stripped.startswith("checksum = ") and name and version:
            checksums[(name, version)] = stripped.split("=", 1)[1].strip().strip('"')
    return checksums


def purl(name: str, version: str) -> str:
    """A Package URL for a crates.io package."""
    return f"pkg:cargo/{name}@{version}"


def build_sbom(metadata: dict, lock_sha: str, checksums: dict[tuple[str, str], str]) -> dict:
    workspace_ids = {member for member in metadata.get("workspace_members", [])}

    # Map id -> (name, version) so dependency edges can be emitted as purls.
    by_id: dict[str, tuple[str, str]] = {}
    for package in metadata["packages"]:
        by_id[package["id"]] = (package["name"], package["version"])

    components = []
    for package in metadata["packages"]:
        name = package["name"]
        version = package["version"]
        is_workspace = package["id"] in workspace_ids

        component: dict = {
            "type": "application" if is_workspace else "library",
            "name": name,
            "version": version,
            "purl": purl(name, version),
            "scope": "required",
        }

        # License: crates.io metadata, when the crate declares one.
        #
        # Almost every crate declares an SPDX *expression* ("MIT OR Apache-2.0"),
        # not a single identifier. CycloneDX allows `expression` for those and
        # reserves `id` for a lone SPDX identifier, so the two cases are emitted
        # differently rather than forcing an expression into `id` (which would be
        # invalid CycloneDX).
        license_value = package.get("license")
        if license_value:
            if " OR " in license_value or " AND " in license_value or " WITH " in license_value:
                component["licenses"] = [{"expression": license_value}]
            else:
                component["licenses"] = [{"license": {"id": license_value}}]

        # The checksum Cargo recorded for this exact version, read from Cargo.lock.
        # Workspace members have no checksum, which is itself informative.
        checksum = checksums.get((name, version))
        if checksum:
            component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        elif is_workspace:
            component["properties"] = [
                {"name": "bioinfo-tools:workspace-member", "value": "true"}
            ]

        if package.get("description"):
            component["description"] = package["description"]

        components.append(component)

    # Deterministic order: by name, then version.
    components.sort(key=lambda component: (component["name"], component["version"]))

    # Dependency edges, as purls, sorted.
    dependencies = []
    for node in metadata.get("resolve", {}).get("nodes", []):
        node_name, node_version = by_id[node["id"]]
        ref = purl(node_name, node_version)
        dep_refs = sorted(
            purl(*by_id[dep]) for dep in node.get("dependencies", []) if dep in by_id
        )
        dependencies.append({"ref": ref, "dependsOn": dep_refs})
    dependencies.sort(key=lambda entry: entry["ref"])

    # The serial is derived from the content, so it is a stable fingerprint rather
    # than a random identifier.
    fingerprint = hashlib.sha256(
        (lock_sha + json.dumps(components, sort_keys=True)).encode()
    ).hexdigest()
    serial = uuid.uuid5(SERIAL_NAMESPACE, fingerprint)

    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{serial}",
        "version": 1,
        # NOTE: no `timestamp`, deliberately. See the module docstring.
        "metadata": {
            "tools": [
                {
                    "vendor": "bioinfo-tools",
                    "name": "packaging/sbom.py",
                    "version": "1",
                }
            ],
            "component": {
                "type": "application",
                "name": "bioinfo-tools",
                "version": "0.1.0",
                "description": "Rust bioinformatics tools workspace",
            },
            "properties": [
                {"name": "bioinfo-tools:input", "value": "Cargo.lock"},
                {"name": "bioinfo-tools:input-sha256", "value": lock_sha},
                {
                    "name": "bioinfo-tools:component-count",
                    "value": str(len(components)),
                },
                {
                    "name": "bioinfo-tools:generator-note",
                    "value": "stdlib generator; no SBOM tooling was installed",
                },
            ],
        },
        "components": components,
        "dependencies": dependencies,
    }


def render(sbom: dict) -> str:
    """Serialise deterministically: sorted keys, two-space indent, trailing newline."""
    return json.dumps(sbom, indent=2, sort_keys=True) + "\n"


def self_test() -> int:
    """Prove the generator is deterministic and that its output is meaningful.

    Uses the real Cargo.lock, because a synthetic graph would not catch a change in
    how `cargo metadata` reports packages.
    """
    ok = True
    lock = ROOT / "Cargo.lock"
    lock_sha = sha256_file(lock)

    # Two independent builds must be byte-identical.
    first = render(build_sbom(cargo_metadata(), lock_sha, lock_checksums(lock)))
    second = render(build_sbom(cargo_metadata(), lock_sha, lock_checksums(lock)))
    if first != second:
        print("SELFTEST FAIL: two runs produced different output", file=sys.stderr)
        ok = False
    if "timestamp" in json.loads(first)["metadata"]:
        print("SELFTEST FAIL: a timestamp crept in", file=sys.stderr)
        ok = False

    sbom = json.loads(first)
    if len(sbom["components"]) < 100:
        print(
            f"SELFTEST FAIL: only {len(sbom['components'])} components; the graph "
            "looks truncated",
            file=sys.stderr,
        )
        ok = False

    # Every component must have a purl and a type.
    for component in sbom["components"]:
        for field in ("type", "name", "version", "purl"):
            if field not in component:
                print(f"SELFTEST FAIL: {component.get('name')} lacks {field}", file=sys.stderr)
                ok = False

    # At least one workspace member must be present and typed as an application.
    applications = [c for c in sbom["components"] if c["type"] == "application"]
    if not applications:
        print("SELFTEST FAIL: no workspace members marked as applications", file=sys.stderr)
        ok = False

    # Checksums must come through for registry packages.
    with_hash = sum(1 for c in sbom["components"] if "hashes" in c)
    if with_hash < 100:
        print(f"SELFTEST FAIL: only {with_hash} components carry a checksum", file=sys.stderr)
        ok = False

    # Component order must be strictly sorted by (name, version), which is what
    # makes the output stable regardless of metadata ordering.
    keys = [(c["name"], c["version"]) for c in sbom["components"]]
    if keys != sorted(keys):
        print("SELFTEST FAIL: components are not sorted", file=sys.stderr)
        ok = False

    print("SELFTEST PASS" if ok else "SELFTEST FAIL")
    return 0 if ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stdout", action="store_true", help="write to stdout")
    parser.add_argument("--check", action="store_true",
                        help="verify the committed SBOM matches what would be generated")
    parser.add_argument("--out", default=str(DEFAULT_OUT), help="output path")
    parser.add_argument("--self-test", dest="self_test", action="store_true",
                        help="prove the generator is deterministic and non-trivial")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    lock = ROOT / "Cargo.lock"
    if not lock.is_file():
        print(f"ERROR: {lock} not found", file=sys.stderr)
        return 3

    lock_sha = sha256_file(lock)
    sbom = build_sbom(cargo_metadata(), lock_sha, lock_checksums(lock))
    text = render(sbom)

    if args.check:
        out = Path(args.out)
        if not out.is_file():
            print(f"FAIL: {out} does not exist; run packaging/sbom.py", file=sys.stderr)
            return 1
        current = out.read_text()
        if current != text:
            print(
                f"FAIL: {out} is stale relative to Cargo.lock (sha256 {lock_sha[:16]}...)\n"
                "      re-run: python3 packaging/sbom.py",
                file=sys.stderr,
            )
            return 1
        print(f"PASS: {out.name} is current (Cargo.lock sha256 {lock_sha[:16]}...)")
        return 0

    if args.stdout:
        sys.stdout.write(text)
        return 0

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text)
    print(
        f"wrote {out.relative_to(ROOT)}: {len(sbom['components'])} components, "
        f"serial {sbom['serialNumber']}"
    )
    print(f"Cargo.lock sha256: {lock_sha}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
