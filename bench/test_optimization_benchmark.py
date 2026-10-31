#!/usr/bin/env python3
"""Reject incomplete or failed optimization evidence before reporting success."""
import copy
import unittest

import optimization_benchmark as benchmark
import summarize_optimization as summary
import merge_optimization as merger


def valid_report():
    row = {'dataset': 'synthetic-1000000', 'case': 'fx_stats', 'passed': True,
           'checks': [{'block': block, 'passed': True} for block in range(-1, 5)],
           'warmups': [{arm: {'exit_code': 0} for arm in benchmark.ARMS}],
           'samples': {arm: [{'block': block, 'exit_code': 0} for block in range(5)] for arm in benchmark.ARMS}}
    return {'planned_conditions': [{'dataset': row['dataset'], 'case': row['case']}],
            'parameters': {'repeats': 5}, 'results': [row]}


def split_reports():
    datasets = [{'id': name, 'kind': kind, 'records': count}
                for name, kind, count in [
                    ('real-1000000', 'fastq', 1_000_000),
                    ('synthetic-1000000', 'fastq', 1_000_000),
                    ('synthetic-bam-1000000', 'bam', 1_000_000),
                    ('synthetic-10000000', 'fastq', 10_000_000),
                    ('synthetic-bam-10000000', 'bam', 10_000_000)]]
    inputs = [{'datasets': datasets}]
    core = valid_report()
    core.update(kind='optimization-benchmark', passed=False, inputs=inputs,
                versions={'binary': 'abc'}, source_sha256={'code': 'abc'},
                input_manifest_sha256={'input': 'abc'}, topology='same host',
                source={'commit': 'e125519'}, started_utc='start',
                script_sha256={'optimization_benchmark.py': 'first', 'readme_benchmark.py': 'shared'})
    for key in ('cpu', 'warmups', 'cooldown', 'seed', 'baseline_dir', 'native_dir',
                'oracle_dir', 'toolchain_cache', 'pilot'):
        core['parameters'][key] = 'same'
    rows = []
    for dataset, case in benchmark.plan(inputs, False):
        row = copy.deepcopy(valid_report()['results'][0])
        row.update(dataset=dataset['id'], case=case)
        rows.append(row)
    core['planned_conditions'] = [{'dataset': row['dataset'], 'case': row['case']} for row in rows]
    core['results'] = copy.deepcopy(rows[:11])
    core['results'][10].update(passed=False, checks=[], samples={arm: [] for arm in benchmark.ARMS})
    streaming = copy.deepcopy(core)
    streaming.update(passed=True, completed_utc='end', planned_conditions=core['planned_conditions'][10:], results=rows[10:])
    streaming['script_sha256']['optimization_benchmark.py'] = 'retry'
    return core, streaming


class EvidenceTests(unittest.TestCase):
    def test_complete_population(self):
        self.assertTrue(benchmark.complete(valid_report()))

    def test_missing_measurement_or_arm(self):
        for arm in benchmark.ARMS:
            report = valid_report()
            report['results'][0]['samples'][arm].pop()
            self.assertFalse(benchmark.complete(report))

    def test_failed_or_incomplete_warmup(self):
        for arm in benchmark.ARMS:
            report = valid_report()
            report['results'][0]['warmups'][0][arm]['exit_code'] = 1
            self.assertFalse(benchmark.complete(report))
            del report['results'][0]['warmups'][0][arm]
            self.assertFalse(benchmark.complete(report))

    def test_failed_verification_and_duplicate_block(self):
        report = valid_report()
        report['results'][0]['checks'][0]['passed'] = False
        self.assertFalse(benchmark.complete(report))
        report = valid_report()
        report['results'][0]['samples']['after'][-1]['block'] = 3
        self.assertFalse(benchmark.complete(report))

    def test_summary_rejects_pilot_and_recomputes_from_samples(self):
        report = valid_report()
        report.update(kind='optimization-pilot', passed=True)
        with self.assertRaises(ValueError):
            summary.render(report)
        report['kind'] = 'optimization-benchmark'
        row = report['results'][0]
        row['records'] = 1_000_000
        row['paired_before_after_ratio'] = {'median': 99}
        row['summary'] = {'before': {'median_s': 999}}
        for arm, elapsed in [('before', 1), ('after', .5), ('upstream', 2)]:
            for sample in row['samples'][arm]:
                sample.update(elapsed_s=elapsed, max_rss_kb=1024, user_s=.1, sys_s=.01,
                              temp_peak_bytes_sampled=0, output_bytes=10, payload_bytes=10)
        markdown, _ = summary.render(report)
        self.assertIn('2.00×', markdown)
        self.assertNotIn('99.00×', markdown)
        row['samples']['after'][0]['elapsed_s'] = float('nan')
        with self.assertRaises(ValueError):
            summary.render(report)

    def test_retry_requires_identical_artifacts_and_inputs(self):
        core, streaming = split_reports()
        self.assertTrue(merger.merge(core, streaming)['passed'])
        for key in ('versions', 'source_sha256', 'input_manifest_sha256', 'inputs'):
            changed = copy.deepcopy(streaming)
            changed[key] = {'changed': True}
            with self.assertRaises(ValueError):
                merger.merge(core, changed)

    def test_retry_never_discards_measured_samples_or_failed_checks(self):
        core, streaming = split_reports()
        core['results'][10]['samples']['after'].append({'block': 0, 'exit_code': 1})
        with self.assertRaises(ValueError):
            merger.merge(core, streaming)
        core, streaming = split_reports()
        core['results'][0]['checks'][0]['passed'] = False
        with self.assertRaises(ValueError):
            merger.merge(core, streaming)

    def test_missing_condition(self):
        report = valid_report()
        report['planned_conditions'].append({'dataset': 'real-1000000', 'case': 'fq_lint'})
        self.assertFalse(benchmark.complete(report))
        report = valid_report()
        report['results'].append(copy.deepcopy(report['results'][0]))
        self.assertFalse(benchmark.complete(report))


if __name__ == '__main__':
    unittest.main()
