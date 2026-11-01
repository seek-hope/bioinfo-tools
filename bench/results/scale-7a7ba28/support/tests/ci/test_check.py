#!/usr/bin/env python3
"""Negative controls for the native quality gate.

The gate under test is the real tests/ci/check.sh. Toolchains, Cargo and oracles
are temporary shims so these tests are offline, fast and safe.
"""

import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
GATE = ROOT / "tests/ci/check.sh"
PIN = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]


class GateTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="quality-gate-test-")
        self.addCleanup(self.scratch.cleanup)
        self.work = Path(self.scratch.name)
        self.log = self.work / "cargo.log"
        self.unshare_log = self.work / "unshare.log"
        self.marker = self.work / "toolchain-installed"
        self.target = self.work / "target"
        self.env = dict(os.environ)
        self.env.pop("CARGO_NET_OFFLINE", None)
        self.env.update(
            PATH=f"{self.work}:{os.environ['PATH']}",
            GATE_TEST_LOG=str(self.log),
            GATE_TEST_UNSHARE_LOG=str(self.unshare_log),
            GATE_TEST_PIN=PIN,
            GATE_TEST_FAIL="fmt",
            GATE_TEST_RUST_VERSION=PIN,
            GATE_TEST_TOOLCHAIN_ABSENT="0",
            GATE_TEST_TOOLCHAIN_MARKER=str(self.marker),
            GATE_TEST_TARGET_DIR=str(self.target),
            GATE_TEST_SEQKIT_VERSION="2.14.0",
            CARGO_BUILD_TARGET="",
        )

        self.shim(
            "rustup",
            'if [ "$1" = toolchain ] && [ "$2" = install ]; then\n'
            '  printf "toolchain install %s\\n" "$3" >> "$GATE_TEST_LOG"\n'
            '  : > "$GATE_TEST_TOOLCHAIN_MARKER"\n'
            '  exit 0\n'
            'fi\n'
            'if [ "${GATE_TEST_TOOLCHAIN_ABSENT:-0}" = 1 ] && [ ! -f "$GATE_TEST_TOOLCHAIN_MARKER" ]; then exit 24; fi\n'
            '[ "$1" = run ] && [ "$2" = "$GATE_TEST_PIN" ] || exit 25\n'
            'shift 2\nexec "$@"\n',
        )
        self.shim("rustc", 'printf "rustc %s (test shim)\\n" "$GATE_TEST_RUST_VERSION"\n')
        self.shim(
            "cargo",
            'printf "%s\\n" "$*" >> "$GATE_TEST_LOG"\n'
            'if [ "$1" != fmt ] && [ "$1" != --version ]; then\n'
            '  case " $* " in\n'
            '    *" --offline "*) [ "${CARGO_NET_OFFLINE:-}" = true ] || exit 45 ;;\n'
            '    *) [ "${CARGO_NET_OFFLINE:-}" != true ] || exit 46 ;;\n'
            '  esac\n'
            'fi\n'
            '[ "$1" != "$GATE_TEST_FAIL" ] || exit 47\n'
            '[ "$1" != --version ] || printf "cargo test shim\\n"\n'
            'if [ "$1" = metadata ]; then printf \'{"target_directory":"%s"}\\n\' "$GATE_TEST_TARGET_DIR"; fi\n'
            'exit 0\n',
        )
        self.shim("unshare", 'printf "%s\\n" "$*" >> "$GATE_TEST_UNSHARE_LOG"\nshift\nexec "$@"\n')
        self.shim("seqkit", 'printf "seqkit v%s\\n" "$GATE_TEST_SEQKIT_VERSION"\n')
        self.shim("fq", 'printf "fq v0.12.0 (test shim)\\n"\n')
        self.shim("fastqc", 'printf "FastQC v0.13.0\\n"\n')
        self.shim("multiqc", 'printf "multiqc, version 1.35\\n"\n')
        for variable, tool in [
            ("SEQKIT_ORACLE", "seqkit"),
            ("FQ_ORACLE", "fq"),
            ("FASTQC_ORACLE", "fastqc"),
            ("MULTIQC_ORACLE", "multiqc"),
        ]:
            self.env[variable] = str(self.work / tool)

    def shim(self, name, body):
        path = self.work / name
        path.write_text("#!/bin/sh\nset -eu\n" + body)
        path.chmod(0o755)

    def run_gate(self, *args):
        return subprocess.run(
            ["bash", str(GATE), *args],
            cwd=self.work,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=20,
        )

    def assert_no_pass(self, result):
        self.assertNotIn("QUALITY PASS", result.stdout)

    def test_help_does_not_build(self):
        result = self.run_gate("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        for flag in ("--offline", "--isolate-network", "--with-fastx-qc-oracles"):
            self.assertIn(flag, result.stdout)
        self.assertFalse(self.log.exists())

    def test_unknown_argument_is_a_usage_error(self):
        result = self.run_gate("--unknown")
        self.assertEqual(result.returncode, 2)
        self.assert_no_pass(result)
        self.assertFalse(self.log.exists())

    def test_absent_toolchain_offline_fails_without_installing(self):
        self.env["GATE_TEST_TOOLCHAIN_ABSENT"] = "1"
        result = self.run_gate("--offline")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("not installed", result.stderr)
        self.assert_no_pass(result)
        self.assertFalse(self.log.exists())

    def test_absent_toolchain_online_installs_the_pin(self):
        self.env["GATE_TEST_TOOLCHAIN_ABSENT"] = "1"
        result = self.run_gate()
        self.assertEqual(result.returncode, 47, result.stderr)
        commands = self.log.read_text()
        self.assertIn(f"toolchain install {PIN}", commands)
        self.assertIn("fmt --all -- --check", commands)
        self.assert_no_pass(result)

    def test_wrong_rust_version_fails_preflight(self):
        self.env["GATE_TEST_RUST_VERSION"] = "0.0.0"
        result = self.run_gate()
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("expected Rust", result.stderr)
        self.assert_no_pass(result)
        self.assertFalse(self.log.exists())

    def test_default_mode_allows_network_and_does_not_force_offline(self):
        self.env["GATE_TEST_FAIL"] = "clippy"
        result = self.run_gate()
        self.assertEqual(result.returncode, 47, result.stderr)
        clippy = next(line for line in self.log.read_text().splitlines() if line.startswith("clippy "))
        self.assertNotIn("--offline", clippy)
        self.assert_no_pass(result)

    def test_offline_mode_sets_the_flag_and_environment(self):
        self.env["GATE_TEST_FAIL"] = "clippy"
        result = self.run_gate("--offline")
        self.assertEqual(result.returncode, 47, result.stderr)
        commands = self.log.read_text()
        clippy = next(line for line in commands.splitlines() if line.startswith("clippy "))
        self.assertIn("--offline", clippy)
        self.assert_no_pass(result)

    def test_wrong_oracle_version_fails_closed(self):
        self.env["GATE_TEST_SEQKIT_VERSION"] = "0.0.0"
        result = self.run_gate("--with-fastx-qc-oracles")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("version mismatch", result.stderr)
        self.assertNotIn("fmt", self.log.read_text())
        self.assert_no_pass(result)

    def test_missing_explicit_oracle_does_not_fall_back(self):
        self.env["FQ_ORACLE"] = str(self.work / "absent-fq")
        result = self.run_gate("--with-fastx-qc-oracles")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("FQ_ORACLE does not resolve", result.stderr)
        self.assertNotIn("fmt", self.log.read_text())
        self.assert_no_pass(result)

    def test_isolate_network_wraps_oracle_probes(self):
        result = self.run_gate("--with-fastx-qc-oracles", "--isolate-network")
        self.assertEqual(result.returncode, 47, result.stderr)
        self.assertIn("-Urn", self.unshare_log.read_text())
        self.assertIn("fmt --all -- --check", self.log.read_text())
        self.assert_no_pass(result)

    def test_fmt_failure_stops_before_clippy_or_tests(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 47, result.stderr)
        commands = self.log.read_text()
        self.assertIn("fmt --all -- --check", commands)
        self.assertNotIn("clippy", commands)
        self.assertNotIn("test --workspace", commands)
        self.assert_no_pass(result)

    def test_clippy_failure_stops_before_tests(self):
        self.env["GATE_TEST_FAIL"] = "clippy"
        result = self.run_gate()
        self.assertEqual(result.returncode, 47, result.stderr)
        commands = self.log.read_text()
        self.assertIn("clippy --workspace --all-targets --locked", commands)
        self.assertNotIn("test --workspace", commands)
        self.assert_no_pass(result)


if __name__ == "__main__":
    unittest.main()
