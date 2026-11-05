#!/usr/bin/env python3
"""Synthetic protocol controls, not timing or biological-performance evidence."""
import copy
import unittest

from superiority_plan import (ARMS, calibration, fingerprint, freeze, schedule,
                              summarize, validate_manifest)

H = 'a' * 64
J = 'b' * 64


def manifest():
    return {'schema_version': 1, 'seed': 20261103, 'identities': {'test-only': H},
            'cases': [{'id': name, 'budgets': [1, 2, 4], 'count': 20,
                       'input_sha256': H, 'commands_sha256': {f'{arm}:{c}': H for c in [1, 2, 4] for arm in ARMS},
                       'comparison': 'exact', 'equal_work': True, 'scope': 'synthetic protocol unit test'}
                      for name in ['fast', 'slow']]}


def observation(m, case, cores, block, arm, elapsed=15.0):
    return {'case': case, 'cores': cores, 'block': block, 'arm': arm,
            'count': next(c['count'] for c in m['cases'] if c['id'] == case),
            'elapsed_seconds': elapsed, 'manifest_sha256': fingerprint(m),
            'batch_report_sha256': H, 'validation_report_sha256': H,
            'input_sha256': H, 'command_sha256': H, 'signature_sha256': H,
            'execution_passed': True, 'correctness_passed': True, 'resources_passed': True,
            'validated_invocations': next(c['count'] for c in m['cases'] if c['id'] == case)}


def evidence(m):
    cal = [observation(m, c['id'], cores, 0, arm) for c in m['cases'] for cores in c['budgets'] for arm in ARMS]
    frozen = freeze(m, cal)
    formal = [observation(m, **r, elapsed=18.0 if r['arm'] == 'current' else 20.0) for r in frozen['schedule']]
    return cal, frozen, formal


class PlanTests(unittest.TestCase):
    def test_complete_population_rejects_missing_duplicate_extra_and_invalid_values(self):
        m = manifest(); cal, _, _ = evidence(m)
        for changed in [cal[:-1], cal + [cal[0]], [dict(cal[0], cores=8), *cal[1:]],
                        [dict(cal[0], cores=True), *cal[1:]],
                        [dict(cal[0], elapsed_seconds=float('nan')), *cal[1:]],
                        [dict(cal[0], elapsed_seconds=True), *cal[1:]],
                        [dict(cal[0], correctness_passed=1), *cal[1:]]]:
            with self.subTest(changed=changed[0]), self.assertRaises(ValueError):
                calibration(m, changed)

    def test_self_consistent_manifest_cannot_omit_cpu_budgets(self):
        for budgets in ([1], [1, 2], [1, 4], [1, 2, 2, 4]):
            m = manifest()
            for c in m['cases']:
                c['budgets'] = list(budgets)
                c['commands_sha256'] = {f'{arm}:{cores}': H for cores in budgets for arm in ARMS}
            with self.subTest(budgets=budgets), self.assertRaises(ValueError):
                validate_manifest(m)

    def test_fastest_arm_budget_controls_fixed_input_count_growth(self):
        m = manifest(); cal, _, _ = evidence(m)
        cal[5]['elapsed_seconds'] = 2
        result = calibration(m, cal)
        self.assertFalse(result['qualified'])
        self.assertEqual(result['conditions'][0]['proposed_count'], 150)
        self.assertEqual(result['proposed_manifest']['cases'][0]['input_sha256'], H)
        with self.assertRaises(ValueError):
            freeze(m, cal)
        cal[5]['elapsed_seconds'] = 10
        frozen = freeze(m, cal)
        self.assertEqual(frozen['manifest']['cases'][0]['count'], 20)
        self.assertEqual(calibration(m, cal)['proposed_manifest']['cases'][0]['count'], 30)

    def test_failed_or_incompletely_validated_calibration_has_no_proposal(self):
        for change in [{'execution_passed': False}, {'resources_passed': False},
                       {'correctness_passed': False}, {'validated_invocations': 19},
                       {'elapsed_seconds': None}, {'signature_sha256': None}]:
            m = manifest(); cal, _, _ = evidence(m); cal[0].update(change)
            result = calibration(m, cal)
            self.assertFalse(result['qualified'])
            self.assertIsNone(result['proposed_manifest'])
            with self.assertRaises(ValueError):
                freeze(m, cal)

    def test_calibration_rejects_cross_arm_or_cross_budget_output_mismatch(self):
        m = manifest(); cal, _, _ = evidence(m); cal[0]['signature_sha256'] = J
        self.assertFalse(calibration(m, cal)['qualified'])
        m['cases'][0]['comparison'] = 'proved-correction'
        for r in cal:
            r['manifest_sha256'] = fingerprint(m)
        self.assertFalse(calibration(m, cal)['qualified'])

    def test_schedule_is_frozen_balanced_and_keeps_warmup_outside_three_pairs(self):
        m = manifest(); _, f, rows = evidence(m)
        self.assertEqual(schedule(m), f['schedule'])
        self.assertEqual(len(rows), 48)
        for case in m['cases']:
            for cores in case['budgets']:
                subset = [r for r in f['schedule'] if r['case'] == case['id'] and r['cores'] == cores]
                self.assertEqual([r['block'] for r in subset], [-1, -1, 0, 0, 1, 1, 2, 2])
                self.assertEqual(sum(r['arm'] == 'current' for r in subset[::2]), 2)
        altered = copy.deepcopy(m); altered['seed'] += 1
        self.assertNotEqual(schedule(m), schedule(altered))

    def test_each_condition_must_meet_exact_ten_percent_margin(self):
        m = manifest(); cal, f, rows = evidence(m)
        self.assertTrue(summarize(f, cal, rows)['passed'])
        for r in rows:
            if r['case'] == 'slow' and r['cores'] == 4 and r['arm'] == 'current':
                r['elapsed_seconds'] = 18.01
        report = summarize(f, cal, rows)
        self.assertFalse(report['passed'])
        self.assertEqual(sum(c['margin_passed'] for c in report['conditions']), 5)
        for r in rows:
            if r['case'] == 'slow' and r['cores'] == 4 and r['arm'] == 'current':
                r['elapsed_seconds'] = 20/1.10
        self.assertFalse(summarize(f, cal, rows)['passed'])

    def test_median_is_of_within_block_ratios_not_ratio_of_medians(self):
        m = manifest(); cal, f, rows = evidence(m)
        for r in rows:
            if r['block'] >= 0:
                times = [10, 1000, 1010] if r['arm'] == 'current' else [10, 20, 1000]
                r['elapsed_seconds'] = times[r['block']]
            else:
                r['elapsed_seconds'] = 500 if r['arm'] == 'current' else 10
        report = summarize(f, cal, rows)
        self.assertEqual(report['conditions'][0]['paired_current_upstream'], [1, 50, 1.01])
        self.assertEqual(report['conditions'][0]['median_current_upstream'], 1.01)

    def test_failed_short_or_incomplete_condition_has_no_rates_and_is_retained(self):
        for change in [{'correctness_passed': False}, {'resources_passed': False},
                       {'execution_passed': False}, {'validated_invocations': 19},
                       {'elapsed_seconds': 9.999}, {'elapsed_seconds': None}]:
            m = manifest(); cal, f, rows = evidence(m); rows[0].update(change)
            report = summarize(f, cal, rows)
            affected = next(c for c in report['conditions'] if (c['case'], c['cores']) == (rows[0]['case'], rows[0]['cores']))
            self.assertFalse(report['passed'])
            self.assertFalse(affected['eligible'])
            self.assertIsNone(affected['paired_current_upstream'])
            self.assertIsNone(affected['median_current_upstream'])
            self.assertTrue(affected['failures'])

    def test_changed_count_command_input_plan_calibration_or_order_is_rejected(self):
        for field, value in [('count', 21), ('command_sha256', J), ('input_sha256', J), ('manifest_sha256', J)]:
            m = manifest(); cal, f, rows = evidence(m); rows[0][field] = value
            with self.assertRaises(ValueError):
                summarize(f, cal, rows)
        m = manifest(); cal, f, rows = evidence(m)
        rows[0], rows[1] = rows[1], rows[0]
        with self.assertRaises(ValueError):
            summarize(f, cal, rows)
        rows[0], rows[1] = rows[1], rows[0]
        for field, value in [('schema_version', True), ('threshold', 1.0)]:
            modified = copy.deepcopy(f); modified[field] = value
            with self.assertRaises(ValueError):
                summarize(modified, cal, rows)
        cal[0]['elapsed_seconds'] = 16
        with self.assertRaises(ValueError):
            summarize(f, cal, rows)

    def test_frozen_identity_distinguishes_json_boolean_and_integer(self):
        m = manifest(); cal, f, rows = evidence(m)
        modified = copy.deepcopy(f)
        next(r for r in modified['schedule'] if r['block'] == 0)['block'] = False
        with self.assertRaises(ValueError):
            summarize(modified, cal, rows)

    def test_cross_block_and_cross_budget_native_determinism_includes_calibration(self):
        m = manifest(); cal, f, rows = evidence(m)
        changed = next(r for r in rows if r['arm'] == 'current')
        changed['signature_sha256'] = J
        report = summarize(f, cal, rows)
        self.assertFalse(report['passed'])
        affected = [c for c in report['conditions'] if c['case'] == changed['case']]
        self.assertEqual(len(affected), 3)
        self.assertTrue(all(not c['eligible'] for c in affected))

    def test_descriptive_conditions_remain_visible_and_cannot_pass_overall(self):
        m = manifest(); m['cases'][1].update(comparison='descriptive', equal_work=False)
        cal, f, rows = evidence(m); report = summarize(f, cal, rows)
        self.assertFalse(report['passed'])
        self.assertEqual(len(report['conditions']), 6)
        self.assertTrue(all(c['median_current_upstream'] is None for c in report['conditions'] if c['case'] == 'slow'))

    def test_proved_correction_requires_validation_not_identical_cross_arm_signatures(self):
        m = manifest(); m['cases'][0]['comparison'] = 'proved-correction'
        cal, _, _ = evidence(m)
        for row in cal:
            if row['case'] == 'fast' and row['arm'] == 'upstream':
                row['signature_sha256'] = J
        f = freeze(m, cal)
        rows = [observation(m, **r, elapsed=18 if r['arm'] == 'current' else 20) for r in f['schedule']]
        for row in rows:
            if row['case'] == 'fast' and row['arm'] == 'upstream':
                row['signature_sha256'] = J
        self.assertTrue(summarize(f, cal, rows)['passed'])
        next(r for r in rows if r['case'] == 'fast')['correctness_passed'] = False
        self.assertFalse(summarize(f, cal, rows)['passed'])

    def test_proved_correction_does_not_establish_equal_work(self):
        m = manifest(); m['cases'][0].update(comparison='proved-correction', equal_work=False)
        cal, f, rows = evidence(m)
        report = summarize(f, cal, rows)
        self.assertFalse(report['passed'])
        self.assertTrue(all(c['median_current_upstream'] is None for c in report['conditions'] if c['case'] == 'fast'))

    def test_strict_manifest_rejects_unknown_fields_bool_budgets_and_duplicate_cases(self):
        for mutate in [lambda m: m.update(extra=1), lambda m: m.update(seed=True),
                       lambda m: m['cases'][0].update(count=True),
                       lambda m: m['cases'][0].update(budgets=[True]),
                       lambda m: m['cases'].append(copy.deepcopy(m['cases'][0])),
                       lambda m: m['cases'][0].update(input_sha256='bad'),
                       lambda m: m['cases'][0].update(scope=''),
                       lambda m: m['cases'][0].update(equal_work=1)]:
            m = manifest(); mutate(m)
            with self.assertRaises(ValueError):
                validate_manifest(m)


if __name__ == '__main__':
    unittest.main()
