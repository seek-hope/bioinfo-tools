#!/usr/bin/env python3
"""Subprocess/EOF/count/timeout controls, separate from real-BAM crosschecks."""
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest

from scale_validation import bam_summary

EOF = bytes.fromhex('1f8b08040000000000ff0600424302001b0003000000000000000000')
SAM = b'@SQ\tSN:c\tLN:9\nr\t0\tc\t1\t60\t1M\t*\t0\t0\tA\tI\n'


class ValidationFailures(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='scale-validation-')
        self.root = Path(self.temp.name)
        self.input = self.root/'input.bam'
        # Deliberately a transport fixture, not a real BAM. Real format coverage
        # is provided by test_canonical_sam and the independently checked BAM.
        self.input.write_bytes(b'fixture' + EOF)

    def tearDown(self):
        self.temp.cleanup()

    def executable(self, name, body):
        path = self.root/name
        path.write_text(f'#!{sys.executable}\nimport os,sys,time\nfrom pathlib import Path\n' + body)
        path.chmod(0o755)
        return path

    def tools(self, quick=0, view=0, canonical=0, count='records=1\n', sleep=False):
        samtools = self.executable('samtools',
            f"if sys.argv[1]=='quickcheck': sys.exit({quick})\n"
            f"Path({str(self.root/'source.pid')!r}).write_text(str(os.getpid()))\n"
            f"sys.stdout.buffer.write({SAM!r});sys.stdout.buffer.flush()\n"
            f"sys.exit({view})\n")
        helper = self.executable('canonicalizer',
            f"Path({str(self.root/'canonical.pid')!r}).write_text(str(os.getpid()))\n"
            f"data=sys.stdin.buffer.read()\n"
            + ('time.sleep(10)\n' if sleep else '')
            + f"sys.stdout.buffer.write(data)\nsys.stderr.write({count!r})\nsys.exit({canonical})\n")
        return samtools, helper

    def test_transport_success_keeps_all_bytes(self):
        result = bam_summary(self.input, *self.tools())
        self.assertEqual(result['records'], 1)
        self.assertEqual(result['normalized_sha256'], hashlib.sha256(SAM).hexdigest())

    def test_either_process_and_quickcheck_failure_are_rejected(self):
        for options in ({'quick': 7}, {'view': 7}, {'canonical': 7}):
            with self.assertRaises(ValueError): bam_summary(self.input, *self.tools(**options))

    def test_missing_or_changed_eof_is_rejected(self):
        for data in (b'x', b'x' * 28):
            self.input.write_bytes(data)
            with self.assertRaises(ValueError): bam_summary(self.input, *self.tools())

    def test_malformed_count_is_rejected(self):
        for count in ('', 'records=NaN\n', 'records=1\nextra\n', 'records=-1\n'):
            with self.assertRaises(ValueError): bam_summary(self.input, *self.tools(count=count))

    def test_timeout_reaps_both_processes(self):
        with self.assertRaisesRegex(ValueError, 'timeout=True'):
            bam_summary(self.input, *self.tools(sleep=True), timeout=0.3)
        for name in ('source', 'canonical'):
            pid = int((self.root/f'{name}.pid').read_text())
            with self.assertRaises(ProcessLookupError): os.kill(pid, 0)


if __name__ == '__main__':
    unittest.main()
