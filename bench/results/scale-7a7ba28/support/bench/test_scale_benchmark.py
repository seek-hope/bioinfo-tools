#!/usr/bin/env python3
"""Controls for benchmark population, order, commands, and numerical evidence."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from scale_common import ARMS, command, conditions, initial_plan, validate_complete
from scale_plan import propose
from summarize_scale import summarize


def fixture():
    plan = initial_plan()
    plan['frozen_for_formal_run'] = True
    plan['workloads'] = plan['workloads'][:1]
    planned = conditions(plan)
    invocations = []
    rows = []
    for condition in planned:
        observations = []
        for block in range(-1, 3):
            for arm in ARMS:
                invocation = {'condition': condition['id'], 'block': block, 'arm': arm,
                              'command': ['taskset', '-c', condition['cpu_affinity'], f'/bin/{arm}'],
                              'output': f'/run/{condition["id"]}/{block}/{arm}/out',
                              'directory': f'/run/{condition["id"]}/{block}/{arm}',
                              'cpu_affinity': condition['cpu_affinity'], 'input_bytes': 100000000,
                              'order_in_block': condition['orders'][block+1].index(arm)}
                invocations.append(invocation)
                observations.append(copy.deepcopy(invocation) | {'elapsed_s': 15.0, 'user_s': 14.0, 'sys_s': 0.1,
                                    'max_rss_kb': 1024, 'fsync_s': 0.1, 'exit_code': 0})
        rows.append({'condition': condition, 'observations': observations, 'input_bytes': 100000000,
                     'checks': [{'block': b, 'passed': True, 'sha256': {'rust': 'a'*64, 'upstream': 'a'*64}} for b in range(-1, 3)]})
    return {'kind': 'scale-benchmark', 'plan': plan, 'planned_conditions': planned,
            'planned_invocations': invocations,
            'invocations_sha256': hashlib.sha256(json.dumps(invocations, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
            'results': rows, 'passed': True, 'completed_utc': 'fixture', 'identities_unchanged': True,
            'inputs_unchanged': True, 'runtime_payloads_unchanged': True}


class EvidenceControls(unittest.TestCase):
    def test_complete_population_and_balanced_orders(self):
        report = fixture()
        validate_complete(report)
        self.assertEqual(len(conditions(initial_plan())), 28)
        for condition in report['planned_conditions']:
            self.assertEqual(condition['orders'].count(list(ARMS)), 2)
            self.assertEqual(condition['orders'].count(list(reversed(ARMS))), 2)

    def test_missing_rows_samples_warmups_or_checks_fail(self):
        for target in ('row', 'sample', 'warmup', 'check'):
            report = fixture()
            if target == 'row': report['results'].pop()
            elif target == 'sample': report['results'][0]['observations'].pop()
            elif target == 'warmup': report['results'][0]['observations'] = report['results'][0]['observations'][2:]
            else: report['results'][0]['checks'].pop()
            with self.assertRaises(ValueError): validate_complete(report)

    def test_nonfinite_negative_and_boolean_metrics_fail(self):
        for value in (None, float('nan'), float('inf'), -1, True, 0):
            report = fixture()
            report['results'][0]['observations'][0]['elapsed_s'] = value
            with self.assertRaises(ValueError): validate_complete(report)

    def test_order_commands_and_affinity_changes_fail(self):
        for key, value in [('order_in_block', 3), ('command', ['wrong']), ('cpu_affinity', '1'), ('input_bytes', 1), ('telemetry_error', 'injected')]:
            report = fixture()
            report['results'][0]['observations'][0][key] = value
            with self.assertRaises(ValueError): validate_complete(report)
        report = fixture()
        report['planned_invocations'][0]['command'].append('--different')
        with self.assertRaises(ValueError): validate_complete(report)

    def test_exit_validation_identity_and_completion_failures(self):
        for target in ('exit', 'check', 'identity', 'inputs', 'runtime', 'completion', 'passed'):
            report = fixture()
            if target == 'exit': report['results'][0]['observations'][0]['exit_code'] = 1
            elif target == 'check': report['results'][0]['checks'][0]['passed'] = False
            elif target == 'identity': report['identities_unchanged'] = False
            elif target == 'inputs': report['inputs_unchanged'] = False
            elif target == 'runtime': report['runtime_payloads_unchanged'] = False
            elif target == 'completion': report.pop('completed_utc')
            else: report['passed'] = False
            with self.assertRaises(ValueError): validate_complete(report)

    def test_changed_output_cannot_hide_behind_passed_marker(self):
        report = fixture()
        report['results'][0]['checks'][0]['sha256']['upstream'] = 'b' * 64
        with self.assertRaises(ValueError): validate_complete(report)

    def test_calibration_cannot_be_formal_evidence(self):
        report = fixture()
        report['kind'] = 'scale-calibration'
        with self.assertRaises(ValueError): validate_complete(report)

    def test_calibration_uses_fastest_arm_and_cpu_budget(self):
        report = fixture()
        report['kind'] = 'scale-calibration'
        report['plan']['frozen_for_formal_run'] = False
        report['planned_invocations'] = [i for i in report['planned_invocations'] if i['block'] == -1]
        report['invocations_sha256'] = hashlib.sha256(json.dumps(report['planned_invocations'], sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        for row in report['results']:
            row['observations'] = [o for o in row['observations'] if o['block'] == -1]
            row['checks'] = [c for c in row['checks'] if c['block'] == -1]
        report['results'][-1]['observations'][-1]['elapsed_s'] = 0.5
        self.assertEqual(propose(report, False)['workloads'][0]['multiplicity'], 30)
        with self.assertRaises(ValueError): propose(report, True)
        report['results'][-1]['observations'][-1]['elapsed_s'] = 15.0
        self.assertTrue(propose(report, True)['frozen_for_formal_run'])

    def test_summary_uses_within_block_ratios_and_keeps_short_runs(self):
        report = fixture()
        for row in report['results']:
            row.update(dataset='real-1000000-x1', records=1000000, input_bytes=100000000)
            for sample in row['observations']:
                sample['temp_peak_bytes_sampled'] = 0
                if sample['block'] >= 0:
                    sample['elapsed_s'] = {'upstream': [1, 100, 101], 'current': [1, 2, 100]}[sample['arm']][sample['block']]
        result = summarize(report)[0]
        self.assertEqual(result['paired_upstream_over_current']['median'], 1.01)
        self.assertEqual(result['arms']['current']['observations_below_10s'], 2)

    def test_native_determinism_and_frozen_plan_are_required(self):
        report = fixture()
        report['results'][0]['checks'][0]['sha256'] = dict.fromkeys(('rust', 'upstream'), 'b'*64)
        with self.assertRaises(ValueError): validate_complete(report)
        report = fixture()
        report['plan']['frozen_for_formal_run'] = False
        with self.assertRaises(ValueError): validate_complete(report)

    def test_cpu_and_memory_arguments_are_explicit(self):
        native = {'bam': Path('/native/bam'), 'fx': Path('/native/fx')}
        upstream = {'samtools': Path('/upstream/samtools'), 'seqkit': Path('/upstream/seqkit'), 'fq': Path('/upstream/fq')}
        dataset = {'unsorted': '/input.bam', 'path': '/input.fastq'}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for cores in (1, 2, 4):
                for arm in ARMS:
                    cmd, _, _, _ = command('bam_sort', dataset, arm, cores, root/f'{cores}-{arm}', native, upstream)
                    if arm == 'current':
                        self.assertEqual(cmd[cmd.index('--max-memory')+1], '512MiB')
                        self.assertEqual(cmd[cmd.index('--threads')+1], str(cores))
                    else:
                        self.assertEqual(cmd[cmd.index('-m')+1], f'{512//cores}M')
                        self.assertEqual(cmd[cmd.index('-@')+1], str(cores-1))
            with self.assertRaises(ValueError):
                command('bam_sort', dataset, 'phase4', 1, root/'bad', native, upstream)


if __name__ == '__main__':
    unittest.main()
