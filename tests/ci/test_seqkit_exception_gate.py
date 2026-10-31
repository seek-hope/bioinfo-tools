#!/usr/bin/env python3
"""Regression controls for exception validation, including crashes and stale reports."""

import hashlib
import json
from pathlib import Path
from types import SimpleNamespace
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tests/oracle"))
import check_seqkit_exceptions as gate


def digest(data):
    return hashlib.sha256(data).hexdigest()


def observations(stdout, code=0):
    return {"code": code, "stdout_sha256": digest(stdout), "stderr_sha256": digest(b""), "artifact_sha256": None}


def valid_stream_report():
    rows = [{"passed": True} for _ in range(302)]
    for width in (0, 2, 60):
        q = b"@q\nAC\nGT\n+\nII\nII\n" if width == 2 else b"@q\nACGT\n+\nIIII\n"
        f = b">f\nAA\nAA\n" if width == 2 else b">f\nAAAA\n"
        upstream_f = b"@f\nAA\nAA\n+\nII\nII\n" if width == 2 else b"@f\nAAAA\n+\nIIII\n"
        rows.append({"command": "range", "case": f"mixed-formats/{width}",
                     "options": ["-r", "1:-1", "-w", str(width)], "passed": False,
                     "inputs_sha256": [digest(b"@q\nACGT\n+x\nIIII\n"), digest(b">f\nAAAA\n")],
                     "observations": [observations(q + upstream_f), observations(q + f)]})
    return {"reference": "seqkit v2.14.0", "counts": {"range": 206, "tab2fx": 99},
            "failures": {"range": 3}, "cases": rows}


class GateControls(unittest.TestCase):
    def test_expected_format_preservation_is_accepted(self):
        gate.check_streaming_report(valid_stream_report())

    def test_range_crash_cannot_hide_under_an_approved_label(self):
        report = valid_stream_report()
        report["cases"][-3]["observations"][1] = observations(b"", code=42)
        with self.assertRaisesRegex(ValueError, "candidate exit/diagnostic/output"):
            gate.check_streaming_report(report)

    def test_successful_wrong_quality_is_rejected(self):
        report = valid_stream_report()
        report["cases"][-3]["observations"][1] = observations(b"@q\nACGT\n+\nIIII\n@f\nAAAA\n+\nIIII\n")
        with self.assertRaisesRegex(ValueError, "candidate exit/diagnostic/output"):
            gate.check_streaming_report(report)

    def test_changed_upstream_failure_is_not_assumed_to_be_the_known_bug(self):
        report = valid_stream_report()
        report["cases"][-3]["observations"][0] = observations(b"", code=42)
        with self.assertRaisesRegex(ValueError, "upstream no longer shows"):
            gate.check_streaming_report(report)

    def test_wrong_input_order_is_not_an_approved_exception(self):
        report = valid_stream_report()
        report["cases"][-3]["inputs_sha256"].reverse()
        with self.assertRaisesRegex(ValueError, "changed input/order"):
            gate.check_streaming_report(report)

    def test_aborted_comparison_cannot_reuse_a_previous_report(self):
        manifest = json.loads((ROOT / "compat/seqkit-translate-approved-differences.json").read_text())
        stale = {"reference": manifest["reference"], "total": manifest["total_raw_cases"],
                 "passed": 710, "cases": [{"case": row["case"], "passed": False,
                                           "observations": row["observations"]} for row in manifest["differences"]]}
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "translate-raw.json"
            path.write_text(json.dumps(stale))
            responses = [SimpleNamespace(returncode=0, stdout="PASS proof\n", stderr=""),
                         SimpleNamespace(returncode=1, stdout="", stderr="injected child abort")]
            with patch.object(sys, "argv", ["gate", "--evidence-dir", tmp]), patch.object(gate.subprocess, "run", side_effect=responses) as run, patch("builtins.print"):
                with self.assertRaisesRegex(SystemExit, "unexpected raw translate result"):
                    gate.main()
                self.assertEqual(run.call_count, 2)
            self.assertFalse(path.exists(), "stale evidence must be removed before the child starts")


if __name__ == "__main__":
    unittest.main()
