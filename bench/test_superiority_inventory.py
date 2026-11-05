#!/usr/bin/env python3
"""Inventory controls only; no command execution or performance evidence."""
import copy
from pathlib import Path
import tempfile
import unittest

from superiority_inventory import FAMILIES, audit, bam_short, historical, load_matrix, planned


def row(tool='rust-bam', surface='sort', status='verified', kind='command', unachievable=''):
    return {'tool': tool, 'upstream': 'test', 'surface': surface, 'kind': kind,
            'status': status, 'unachievable': unachievable, 'evidence': '',
            'deviation': '', 'next_action': ''}


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.cases = historical() + planned()

    def test_real_matrix_is_reconciled_without_dropping_supported_rows(self):
        rows = load_matrix()
        result = audit(rows, self.cases)
        self.assertEqual(len(result['matrix_mapping']), len(rows))
        self.assertFalse(result['complete'])
        self.assertEqual(sum(x['disposition'] == 'performance-family' for x in result['matrix_mapping']),
                         sum(r['status'] != 'unsupported' and not r['unachievable'] and
                             r['surface'] not in ('-h --help', '-v --version', '--version', 'global:--help/--version') and
                             not (r['kind'] == 'feature' and r['surface'].startswith(('performance:', 'determinism:')))
                             for r in rows))
        self.assertTrue(all(x['family'] is not None for x in result['matrix_mapping']
                            if x['disposition'] == 'performance-family'))

    def test_historical_winning_and_losing_families_remain(self):
        names = {x['id'] for x in historical()}
        self.assertEqual(names, {'real-cutadapt', 'synthetic-cutadapt', 'real-cutadapt-gzip',
                                 'real-stats', 'synthetic-stats', 'synthetic-fq2fa', 'synthetic-lint',
                                 'bam-flagstat', 'bam-sort', 'umi-dedup', 'real-fastqc'})
        self.assertTrue(all(x['comparison'] is None for x in historical()))

    def test_required_families_and_modes_cannot_be_pruned(self):
        for family in FAMILIES:
            with self.subTest(family=family), self.assertRaises(ValueError):
                audit([row()], [x for x in self.cases if x['family'] != family])
        for mode in ('linked', '1000-patterns', 'multilevel', 'cram-reference', 'report-assets'):
            cases = copy.deepcopy(self.cases)
            target = next(x for x in cases if mode in x['modes'])
            target['modes'].remove(mode)
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                audit([row()], cases)

    def test_unknown_modes_and_duplicate_ids_or_modes_fail(self):
        for mutation in (
            lambda c: c[0]['modes'].append('invented'),
            lambda c: c[1].update(id=c[0]['id']),
            lambda c: c[0]['modes'].append(c[0]['modes'][0]),
            lambda c: c[0]['modes'].append(True),
            lambda c: c[0].update(id='../escape'),
            lambda c: c.pop(),
        ):
            cases = copy.deepcopy(self.cases)
            mutation(cases)
            with self.assertRaises(ValueError):
                audit([row()], cases)

    def test_supported_surfaces_must_map_but_unsupported_are_not_timed(self):
        for changed in [row(surface='new-command'),
                        row(tool='rust-fx', surface='seqkit:new-command'),
                        row(tool='rust-umitools', surface='unknown:thing')]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                audit([changed], self.cases)
        for status, unachievable, disposition in [('unsupported', '', 'excluded-not-implemented'),
                                                   ('verified', 'pure Rust exclusion', 'excluded-unachievable')]:
            result = audit([row(surface='new-command', status=status, unachievable=unachievable)], self.cases)
            self.assertEqual(result['matrix_mapping'][0]['disposition'], disposition)

    def test_metadata_and_validation_rows_have_explicit_nonworkload_scope(self):
        for changed, disposition in [(row(surface='-h --help', kind='option'), 'excluded-metadata'),
                                     (row(surface='determinism:threads', kind='feature'), 'excluded-validation-assertion')]:
            self.assertEqual(audit([changed], self.cases)['matrix_mapping'][0]['disposition'], disposition)

    def test_comparison_state_cannot_fabricate_eligibility(self):
        cases = copy.deepcopy(self.cases)
        target = next(x for x in cases if x['id'] == 'planned-fastqc')
        target['equal_work'] = True
        with self.assertRaises(ValueError):
            audit([row()], cases)
        cases = copy.deepcopy(self.cases)
        target = next(x for x in cases if x['id'] == 'planned-bam-sorting')
        target['comparison'] = 'exact'
        target['equal_work'] = True
        # A declared exact/equal-work case still has no commands, so it is not
        # an executable formal manifest. This plan does not claim otherwise.
        self.assertFalse(audit([row()], cases)['complete'])

    def test_exact_case_schema_and_provenance(self):
        for change in [{'new': True}, {'origin': 'invented'}, {'evidence': None},
                       {'scope': ''}, {'profile': []}, {'workloads': 'large'},
                       {'commands': []}]:
            cases = copy.deepcopy(self.cases)
            cases[0].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                audit([row()], cases)

    def test_short_family_expands_all17_cases_and_all_budgets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ['bam', 'no-eof', 'empty', 'sam', 'gzip', 'bgzf', 'header']
            inputs = {name: root/name for name in names}
            for path in inputs.values():
                path.write_bytes(b'x')
            inputs['distinct'] = []
            for index in range(100):
                path = root/f'distinct-{index}'
                path.write_bytes(b'x')
                inputs['distinct'].append(path)
            executables = {'current': root/'current', 'upstream': root/'upstream'}
            for path in executables.values():
                path.write_bytes(b'x')
            cases = bam_short(inputs, executables)
            result = audit([row()], self.cases+cases)
            self.assertEqual(len(cases), 18)
            self.assertEqual(sum(len(x['commands']) for x in cases), 36)
            self.assertTrue(all(set(x['commands']['current']) == {'1', '2', '4'} for x in cases))
            self.assertEqual(len({arg for x in cases if x['id'] == 'quickcheck-100-distinct'
                                  for arg in x['commands']['current']['1']['argv'][2:]}), 100)
            self.assertEqual(len({arg for x in cases if x['id'] == 'quickcheck-mixed-distinct'
                                  for arg in x['commands']['current']['1']['argv'][2:-1]}), 99)
            self.assertEqual(len({path.name for path in inputs['distinct']}), 100)
            self.assertTrue(all(x['comparison'] == 'exact' and x['equal_work'] for x in cases))
            header=next(x for x in cases if x['id']=='quickcheck-header-only')
            self.assertEqual(header['commands']['current']['1']['argv'][-1],str(inputs['header']))
            self.assertIn('header-only',header['modes'])
            self.assertFalse(result['complete'])

    def test_short_family_rejects_missing_or_shared_inputs_and_partial_commands(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = {name: root/name for name in ['bam', 'no-eof', 'empty', 'sam', 'gzip', 'bgzf', 'header']}
            for path in inputs.values():
                path.write_bytes(b'x')
            inputs['distinct'] = [root/str(i) for i in range(100)]
            for path in inputs['distinct']:
                path.write_bytes(b'x')
            executables = {'current': root/'current', 'upstream': root/'upstream'}
            for path in executables.values():
                path.write_bytes(b'x')
            for change in ['missing-input', 'short-distinct', 'duplicate-paths', 'relative-exe', 'hardlink-alias']:
                changed = copy.deepcopy(inputs)
                arms = dict(executables)
                if change == 'missing-input':
                    del changed['sam']
                elif change == 'short-distinct':
                    changed['distinct'] = changed['distinct'][:99]
                elif change == 'duplicate-paths':
                    changed['distinct'][-1] = changed['distinct'][0]
                elif change == 'hardlink-alias':
                    changed['distinct'][-1].unlink()
                    changed['distinct'][-1].hardlink_to(changed['distinct'][0])
                else:
                    arms['current'] = Path('relative')
                with self.subTest(change=change), self.assertRaises(ValueError):
                    bam_short(changed, arms)
            # Restore the negative-control hardlink before checking valid input.
            inputs['distinct'][-1].unlink()
            inputs['distinct'][-1].write_bytes(b'x')
            cases = bam_short(inputs, executables)
            cases[0]['commands']['current'].pop('4')
            with self.assertRaises(ValueError):
                audit([row()], self.cases+cases)

    def test_listing_family_obligations_is_not_executable_proof(self):
        result = audit([row()], self.cases)
        self.assertEqual(result['kind'], 'performance-coverage-inventory')
        self.assertFalse(result['complete'])
        self.assertTrue(all(not case['commands'] for case in result['cases']))
        self.assertIn('not executable', next(x for x in result['cases'] if x['id'] == 'planned-bam-sorting')['scope'])


if __name__ == '__main__':
    unittest.main()
