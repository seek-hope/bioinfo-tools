#!/usr/bin/env python3
"""Gate approved SeqKit exceptions without changing raw differential verdicts.

First prove corrected semantics independently, then run the original exact
comparisons. Every translate mismatch must match a reviewed case AND both
observation hashes. No new mismatch, changed output or disappeared exception is
accepted automatically. Range keeps its three raw mixed-format failures visible;
its candidate output is independently checked for provenance and composition.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def check_streaming_report(report):
    """Validate every permitted mismatch against these exact typed inputs."""
    if report.get("reference") != "seqkit v2.14.0" or report.get("counts") != {"range": 206, "tab2fx": 99}:
        raise ValueError("raw streaming suite/version changed")
    rows = report.get("cases", [])
    failed = [row for row in rows if not row["passed"]]
    if len(rows) != 305 or report.get("failures") != {"range": 3} or len(failed) != 3:
        raise ValueError("raw streaming failure count changed")
    empty_hash = hashlib.sha256(b"").hexdigest()
    inputs = [b"@q\nACGT\n+x\nIIII\n", b">f\nAAAA\n"]
    input_hashes = [hashlib.sha256(data).hexdigest() for data in inputs]

    def wrap(data, width):
        return (b"\n".join(data[i:i + width] for i in range(0, len(data), width)) if width else data) + b"\n"

    for row, width in zip(failed, (0, 2, 60)):
        if (row["command"] != "range" or row["case"] != f"mixed-formats/{width}"
                or row["options"] != ["-r", "1:-1", "-w", str(width)] or row["inputs_sha256"] != input_hashes):
            raise ValueError("unregistered range failure or changed input/order")
        q = b"@q\n" + wrap(b"ACGT", width) + b"+\n" + wrap(b"IIII", width)
        correct = q + b">f\n" + wrap(b"AAAA", width)
        expected = {"code": 0, "stdout_sha256": hashlib.sha256(correct).hexdigest(),
                    "stderr_sha256": empty_hash, "artifact_sha256": None}
        upstream, candidate = row["observations"]
        if candidate != expected:
            raise ValueError(f"range {width}: candidate exit/diagnostic/output differs from typed-input expectation")
        permitted = {hashlib.sha256(q + b"@f\n" + wrap(b"AAAA", width) + b"+\n" + wrap(quality, width)).hexdigest()
                     for quality in (b"IIII", b"")}
        if (upstream["code"] != 0 or upstream["stderr_sha256"] != empty_hash
                or upstream["artifact_sha256"] is not None or upstream["stdout_sha256"] not in permitted):
            raise ValueError(f"range {width}: upstream no longer shows the proven format/quality defect")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seqkit", default="tests/ci/env/bin/seqkit")
    parser.add_argument("--rust", default="target/debug/rust-fx")
    parser.add_argument("--fq", default="tests/ci/env/bin/fq")
    parser.add_argument("--evidence-dir", type=Path)
    args = parser.parse_args()
    shared = ["--seqkit", str(Path(args.seqkit).resolve()), "--rust", str(Path(args.rust).resolve())]
    with tempfile.TemporaryDirectory(prefix="seqkit-policy-") as temporary:
        evidence = args.evidence_dir or Path(temporary)
        evidence.mkdir(parents=True, exist_ok=True)
        proof = subprocess.run([sys.executable, str(ROOT / "tests/oracle/seqkit_correctness.py"),
                                *shared, "--fq", str(Path(args.fq).resolve()),
                                "--report", str(evidence / "correctness.json")],
                               capture_output=True, text=True, timeout=180)
        (evidence / "correctness.log").write_text(proof.stdout + proof.stderr)
        if proof.returncode:
            raise SystemExit("FAIL independent correctness proof:\n" + proof.stdout + proof.stderr)
        print(proof.stdout.splitlines()[0])

        observed_path = evidence / "translate-raw.json"
        # A failed/aborted child must never be validated against an older run.
        observed_path.unlink(missing_ok=True)
        raw = subprocess.run([sys.executable, str(ROOT / "tests/oracle/seqkit_translate_diff.py"),
                              *shared, "--report", str(observed_path)],
                             capture_output=True, text=True, timeout=180)
        (evidence / "translate-raw.log").write_text(raw.stdout + raw.stderr)
        if raw.returncode != 1 or not observed_path.exists():
            raise SystemExit(f"FAIL unexpected raw translate result {raw.returncode}:\n{raw.stdout[-4000:]}{raw.stderr}")
        observed = json.loads(observed_path.read_text())
        accepted = json.loads((ROOT / "compat/seqkit-translate-approved-differences.json").read_text())
        if observed["reference"] != accepted["reference"] or observed["total"] != accepted["total_raw_cases"]:
            raise SystemExit("FAIL raw suite/version changed; review the exception evidence")
        actual = {row["case"]: row["observations"] for row in observed["cases"] if not row["passed"]}
        expected = {row["case"]: row["observations"] for row in accepted["differences"]}
        if actual != expected:
            missing = sorted(expected.keys() - actual.keys())
            new = sorted(actual.keys() - expected.keys())
            changed = sorted(k for k in actual.keys() & expected.keys() if actual[k] != expected[k])
            raise SystemExit(f"FAIL exception mismatch: missing={missing}; new={new}; changed={changed}")
        print(f"translate raw: {observed['passed']}/{observed['total']} exact; {len(actual)} reviewed differences with matching hashes")

        streaming_path = evidence / "streaming-raw.json"
        streaming_path.unlink(missing_ok=True)
        streaming = subprocess.run([sys.executable, str(ROOT / "tests/oracle/seqkit_streaming_diff.py"),
                                    *shared, "--report", str(streaming_path)],
                                   capture_output=True, text=True, timeout=120)
        (evidence / "streaming-raw.log").write_text(streaming.stdout + streaming.stderr)
        if streaming.returncode != 1 or not streaming_path.exists():
            raise SystemExit("FAIL incomplete raw streaming comparison:\n" + streaming.stdout + streaming.stderr)
        try:
            check_streaming_report(json.loads(streaming_path.read_text()))
        except (ValueError, KeyError, TypeError) as error:
            raise SystemExit(f"FAIL raw streaming exception: {error}") from error
        print("range raw: 203/206 exact; 3 approved quality-provenance differences")
        print("tab2fx raw: 99/99 exact")
        print("PASS approved exceptions: independent correctness + unchanged raw comparisons")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
