#!/usr/bin/env python3
"""Cross-check the compiled streaming canonicalizer against Python tuple ordering."""
import hashlib
from itertools import groupby
from pathlib import Path
import random
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def reference(headers, records):
    result = b''.join(h for h in headers if not h.startswith(b'@PG\t'))
    for _, group in groupby(records, key=lambda r: (r[2], r[3])):
        result += b''.join(b'\t'.join(row) + b'\n' for row in sorted(group))
    return result


class CanonicalSam(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='canonical-sam-test-')
        cls.binary = Path(cls.temp.name) / 'canonical-sam'
        subprocess.run(['rustc', '--edition=2021', '-O', str(ROOT / 'bench/canonical_sam.rs'),
                        '-o', str(cls.binary)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def run_helper(self, data):
        return subprocess.run([str(self.binary)], input=data, capture_output=True, timeout=30)

    def fixture(self):
        rng = random.Random(20261102)
        headers = [b'@HD\tVN:1.6\tSO:coordinate\n', b'@SQ\tSN:c1\tLN:100\n',
                   b'@PG\tID:first\n', b'@SQ\tSN:c2\tLN:100\n', b'@PG\tID:second\n']
        records = []
        for ref in [b'c1', b'c2']:
            for pos in range(1, 21):
                for _ in range(30):
                    name = f'r{rng.randrange(100)}'.encode()
                    records.append([name, b'16' if rng.randrange(2) else b'0', ref, str(pos).encode(),
                                    b'60', b'2M', b'*', b'0', b'0', rng.choice([b'AC', b'TG', b'NN']),
                                    b'II', rng.choice([b'XX:Z:a', b'XX:Z:ab', b'XX:Z:']), b'NM:i:0'])
        return headers, records

    def encode(self, headers, records):
        return b''.join(headers) + b''.join(b'\t'.join(row) + b'\n' for row in records)

    def test_matches_independent_python_tuple_order_and_retains_duplicates(self):
        headers, records = self.fixture()
        result = self.run_helper(self.encode(headers, records))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, reference(headers, records))
        self.assertEqual(result.stderr, f'records={len(records)}\n'.encode())

    def test_fields_record_deletion_and_duplication_remain_visible(self):
        headers, records = self.fixture()
        original = self.run_helper(self.encode(headers, records)).stdout
        variants = [records[:-1], records + [records[-1]], [r[:] for r in records]]
        variants[-1][0][-1] = b'NM:i:1'
        for altered in variants:
            result = self.run_helper(self.encode(headers, altered))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotEqual(hashlib.sha256(result.stdout).digest(), hashlib.sha256(original).digest())

    def test_invalid_coordinates_or_layout_do_not_report_success(self):
        headers, records = self.fixture()
        for altered in [list(reversed(records)), [records[0][:3]], [records[0][:]]]:
            if len(altered) == 1 and len(altered[0]) > 3:
                altered[0][3] = b'-1'
            result = self.run_helper(self.encode(headers, altered))
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(b'records=', result.stderr)


if __name__ == '__main__':
    unittest.main()
