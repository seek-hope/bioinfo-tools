#!/usr/bin/env python3
"""External integration controls for BAM repetition; run with pinned --samtools."""
from collections import Counter
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from readme_inputs import digest
from scale_inputs import repeat_bam

parser = argparse.ArgumentParser(add_help=False)
parser.add_argument('--samtools', type=Path)
args, remaining = parser.parse_known_args()
SAMTOOLS = args.samtools.resolve() if args.samtools else None


@unittest.skipUnless(SAMTOOLS, 'requires explicit pinned --samtools')
class BamInputs(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        banner = subprocess.check_output([str(SAMTOOLS), '--version'], text=True).splitlines()[0]
        if banner != 'samtools 1.24':
            raise ValueError(f'wrong pinned samtools: {banner}')

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='scale-bam-test-')
        self.root = Path(self.temp.name)
        sam = self.root/'input.sam'
        text = '@HD\tVN:1.6\tSO:unsorted\n@SQ\tSN:c\tLN:100\n'
        index = 0
        for pos in (2, 1):
            for umi, count in [('AAAAAAAAAA', 6), ('CAAAAAAAAA', 2), ('CCAAAAAAAA', 1), ('CCCCCCCCCC', 1)]:
                for copy in range(count):
                    mapq = 60 if copy == 0 and umi in ('AAAAAAAAAA', 'CCCCCCCCCC') else 30
                    text += f'r{index:04}\t0\tc\t{pos}\t{mapq}\t2M\t*\t0\t0\tAC\tII\tUB:Z:{umi}\n'
                    index += 1
        sam.write_text(text)
        bam = self.root/'unsorted.bam'
        subprocess.run([str(SAMTOOLS), 'view', '--no-PG', '-b', '-o', str(bam), str(sam)], check=True, capture_output=True)
        expected = self.root/'dedup-qnames.txt'
        expected.write_text('r0000\nr0009\nr0010\nr0019\n')
        self.source = {'id': 'toy-bam', 'kind': 'bam', 'unsorted': str(bam), 'records': index,
                       'expected_qnames': str(expected), 'expected_dedup_records': 4,
                       'generator': {'umi_counts_per_scale': [6, 2, 1, 1],
                                     'umi_distance_from_major': [0, 1, 2, 10],
                                     'independent_representative': 'unique highest MAPQ'},
                       'sha256': {p.name: digest(p) for p in (bam, expected)}}

    def tearDown(self):
        self.temp.cleanup()

    def records(self, path):
        return subprocess.check_output([str(SAMTOOLS), 'view', str(path)]).splitlines()

    def test_exact_records_global_sort_index_and_representatives(self):
        result = repeat_bam(self.source, 3, self.root/'expanded', SAMTOOLS, True)
        expected = {record: count * 3 for record, count in Counter(self.records(self.source['unsorted'])).items()}
        for key in ('unsorted', 'sorted'):
            self.assertEqual(Counter(self.records(result[key])), expected)
        positions = [int(line.split(b'\t')[3]) for line in self.records(result['sorted'])]
        self.assertEqual(positions, sorted(positions))
        self.assertEqual(result['records'], 60)
        self.assertEqual(result['expected_dedup_records'], 4)
        self.assertEqual(Path(result['expected_qnames']).read_bytes(), Path(self.source['expected_qnames']).read_bytes())
        self.assertTrue(Path(result['sorted'] + '.bai').is_file())
        for name, checksum in result['sha256'].items():
            self.assertEqual(digest(self.root/'expanded'/name), checksum)
        with self.assertRaises(FileExistsError):
            repeat_bam(self.source, 3, self.root/'expanded', SAMTOOLS, True)

    def test_changed_source_and_space_failure_leave_no_output(self):
        changed = dict(self.source, sha256=dict(self.source['sha256']))
        changed['sha256']['unsorted.bam'] = '0' * 64
        with self.assertRaises(ValueError): repeat_bam(changed, 2, self.root/'changed', SAMTOOLS, False)
        self.assertFalse((self.root/'changed').exists())
        with patch('scale_inputs.require_space', side_effect=OSError('full')):
            with self.assertRaises(OSError): repeat_bam(self.source, 2, self.root/'full', SAMTOOLS, False)
        self.assertFalse((self.root/'full').exists())

    def test_failed_generation_is_retained(self):
        failure = subprocess.CompletedProcess([], 7, stdout='', stderr='injected failure')
        with patch('scale_inputs.subprocess.run', return_value=failure):
            with self.assertRaises(RuntimeError): repeat_bam(self.source, 2, self.root/'failed', SAMTOOLS, False)
        log = json.loads((self.root/'failed/generation.json').read_text())
        self.assertEqual(log[0]['exit'], 7)
        self.assertEqual(log[0]['stderr'], 'injected failure')
        self.assertIn('cat', log[0]['command'])


if __name__ == '__main__':
    unittest.main(argv=[__file__, *remaining])
