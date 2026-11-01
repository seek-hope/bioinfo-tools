#!/usr/bin/env python3
"""Deterministic checks for expanded input preparation (no installed oracle required)."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import scale_inputs as scale
from readme_inputs import scan_fastq


class ExpandedInputs(unittest.TestCase):
    def source(self, root, data=b'@one\nACGT\n+\nIIII\n@two\nNN\n+\n55\n'):
        source = root / 'source.fastq'
        source.write_bytes(data)
        return dict(id='source', kind='fastq', path=str(source), **scan_fastq(source))

    def test_exact_repetition_and_counts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.source(root)
            out = root / 'repeated.fastq'
            result = scale.repeat_fastq(source, 3, out)
            self.assertEqual(out.read_bytes(), Path(source['path']).read_bytes() * 3)
            for key in ['records', 'bases', 'gc_bases', 'bytes']:
                self.assertEqual(result[key], source[key] * 3)
            self.assertEqual(result['sha256'], scan_fastq(out)['sha256'])
            self.assertTrue(result['independent_fastq_validation'])
            with self.assertRaises(FileExistsError):
                scale.repeat_fastq(source, 3, out)

    def test_changed_source_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.source(root)
            Path(source['path']).write_bytes(b'@changed\nA\n+\nI\n')
            out = root / 'out.fastq'
            with self.assertRaises(ValueError):
                scale.repeat_fastq(source, 2, out)
            self.assertFalse(out.exists())

    def test_repetition_requires_record_boundary_and_positive_count(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.source(root, b'@one\nAC\n+\nII')
            out = root / 'out.fastq'
            for count in [0, -1, 2]:
                with self.assertRaises(ValueError):
                    scale.repeat_fastq(source, count, out)
                self.assertFalse(out.exists())

    def test_verified_reuse_does_not_reserve_inputs_twice(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.source(root)
            data = scale.repeat_fastq(source, 3, root/'expanded.fastq')
            data.update(id='source-x3', source_population='source', multiplicity=3,
                        source_metadata=source, generator={'method': 'exact frozen population repetition'})
            manifest = root/'inputs.json'
            manifest.write_text(json.dumps({'passed': True, 'datasets': [data]}))
            groups = {('source', 3): [{'case': 'fx_fq2fa'}]}
            reused = scale.reusable_inputs([manifest], groups, {'source': source})
            self.assertEqual(reused[('source', 3)]['path'], data['path'])
            estimate = scale.storage_estimate(groups, {'source': source}, reused)
            self.assertEqual(estimate['new_input_upper_bound_bytes'], 0)
            self.assertEqual(estimate['reused_actual_input_bytes'], data['bytes'])
            self.assertGreater(estimate['execution_outputs_and_runs_upper_bound_bytes'], data['bytes'])
            Path(data['path']).write_bytes(b'changed')
            with self.assertRaises(ValueError): scale.reusable_inputs([manifest], groups, {'source': source})
            manifest.write_text(json.dumps({'passed': False, 'datasets': []}))
            with self.assertRaises(ValueError): scale.reusable_inputs([manifest], groups, {'source': source})

    def test_space_failure_precedes_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.source(root)
            out = root / 'out.fastq'
            with patch.object(scale, 'require_space', side_effect=OSError('no space')):
                with self.assertRaises(OSError):
                    scale.repeat_fastq(source, 2, out)
            self.assertFalse(out.exists())


if __name__ == '__main__':
    unittest.main()
