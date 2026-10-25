#!/usr/bin/env python3
"""Regression tests for the acceptance entry point; never record real goldens."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class AcceptanceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="acceptance-test-")
        self.addCleanup(temporary.cleanup)
        self.work = Path(temporary.name)
        gate = self.work / "tests/gate"
        gate.mkdir(parents=True)
        self.script = gate / "acceptance.sh"
        shutil.copyfile(ROOT / "tests/gate/acceptance.sh", self.script)
        self.golden = gate / "golden/sentinel.json"
        self.golden.parent.mkdir()
        self.golden.write_bytes(b'{"exit_code": 7}\n')
        self.before = hashlib.sha256(self.golden.read_bytes()).digest()
        self.log = self.work / "invoked"
        (gate / "m3.sh").write_text(
            '#!/usr/bin/env bash\nset -eu\n'
            'printf "%s\\n%s\\n%s\\n%s\\n" "$PWD" "${CUTADAPT_BIN:-}" '
            '"${GOLDEN_DIR:-}" "$*" > "$ACCEPTANCE_TEST_LOG"\n'
            'exit "${ACCEPTANCE_TEST_EXIT:-0}"\n'
        )
        self.env = dict(os.environ, ACCEPTANCE_TEST_LOG=str(self.log),
                        ACCEPTANCE_TEST_EXIT="0")

    def run_gate(self, *args, env=None):
        result = subprocess.run(
            ["bash", str(self.script), *args], cwd=self.work.parent,
            env=self.env if env is None else env, text=True,
            capture_output=True, timeout=10,
        )
        self.assertTrue(self.golden.is_file(), "golden was deleted")
        self.assertEqual(hashlib.sha256(self.golden.read_bytes()).digest(), self.before)
        self.assertEqual(list(self.golden.parent.iterdir()), [self.golden])
        return result

    def test_no_arguments_is_a_usage_error_not_a_pass(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("Usage:", result.stderr)
        self.assertFalse(self.log.exists())

    def test_help_without_invoking_tools(self):
        for flag in ("-h", "--help"):
            with self.subTest(flag=flag):
                result = self.run_gate(flag)
                self.assertEqual(result.returncode, 0)
                self.assertIn("Usage:", result.stdout)
                self.assertFalse(self.log.exists())

    def test_record_and_unknown_arguments_are_rejected(self):
        for flag in ("--record", "--unknown", "", "--"):
            with self.subTest(flag=flag):
                self.assertEqual(self.run_gate(flag).returncode, 2)
                self.assertFalse(self.log.exists())

    def test_extra_arguments_never_start_verification(self):
        for args in (("--verify", "--record"), ("--help", "--record"),
                     ("--verify", "--help"), ("--verify", "--verify")):
            with self.subTest(args=args):
                self.assertEqual(self.run_gate(*args).returncode, 2)
                self.assertFalse(self.log.exists())

    def test_golden_directory_and_manifest_overrides_are_rejected(self):
        for variable in ("GOLDEN_DIR", "GOLDEN_MANIFEST"):
            with self.subTest(variable=variable):
                env = dict(self.env)
                env[variable] = str(self.work / "forged")
                self.assertEqual(self.run_gate("--verify", env=env).returncode, 2)
                self.assertFalse(self.log.exists())

    def test_verify_delegates_from_workspace_root_without_arguments(self):
        env = dict(self.env, CUTADAPT_BIN="/tmp/binary with spaces")
        result = self.run_gate("--verify", env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.log.read_text().splitlines(),
                         [str(self.work), "/tmp/binary with spaces", "", ""])

    def test_verify_propagates_failures(self):
        for code in (1, 3, 47):
            with self.subTest(code=code):
                env = dict(self.env, ACCEPTANCE_TEST_EXIT=str(code))
                self.assertEqual(self.run_gate("--verify", env=env).returncode, code)

    def test_missing_verifier_fails_closed(self):
        (self.script.parent / "m3.sh").unlink()
        self.assertEqual(self.run_gate("--verify").returncode, 127)
        self.assertFalse(self.log.exists())

    def test_snapshot_record_requires_explicit_confirmation(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "tests/gate/snapshot.py"),
             "--record", "--out", str(self.work / "out"),
             "--cutadapt", "/bin/true", "--umitools", "/bin/true"],
            cwd=ROOT, text=True, capture_output=True, timeout=10,
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("--confirm-record", result.stderr)


if __name__ == "__main__":
    unittest.main()
