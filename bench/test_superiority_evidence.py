#!/usr/bin/env python3
"""Real-batch controls for strict complete-byte evidence receipts."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

import superiority_batch as batch
from superiority_evidence import decode, validate_byte_batch
from superiority_resources import Limits

ROOT = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


@unittest.skipUnless(sys.platform == 'linux' and 4 in os.sched_getaffinity(0), 'requires local CPU4 topology')
class EvidenceTests(unittest.TestCase):
    def setUp(self):
        (ROOT/'target').mkdir(exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix='evidence-control-', dir=ROOT/'target')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)/'batch'
        self.limits = Limits(rss_bytes=1024**3, files_bytes=1024**3, headroom_bytes=1,
                             seconds=10, interval_seconds=.02)
        self.spec = {'schema_version': 1, 'count': 2, 'cores': 1, 'stdin': None, 'expected_exit': 0,
                     'argv': [sys.executable, '-c', 'from pathlib import Path; Path("out").mkdir(); Path("out/data").write_bytes(b"ACGT\\n"); print("ok")']}
        # Freeze measurement identities before execution, not from its report.
        paths = [batch.CHILD, Path(batch.__file__).resolve(), Path(batch.__file__).with_name('superiority_resources.py').resolve(),
                 Path(sys.executable).resolve(), batch.TIME, Path(shutil.which('taskset')), Path(self.spec['argv'][0])]
        self.identities = {str(p): batch.digest(p) for p in paths}
        self.outputs = {'stdout': sha(b'ok\n'), 'stderr': sha(b''), 'out/data': sha(b'ACGT\n')}
        self.directories = ['tmp', 'out']
        self.raw = batch.run_batch(self.spec, self.root, dict(os.environ), self.limits)
        self.assertTrue(self.raw['execution_passed'], self.raw['failures'])

    def verify(self, **kwargs):
        args = dict(root=self.root, spec=self.spec, expected_identities=self.identities,
                    expected_files=self.outputs, expected_directories=self.directories, limits=self.limits)
        args.update(kwargs)
        return validate_byte_batch(**args)

    def report(self, raw):
        (self.root/'report.json').write_text(json.dumps(raw)+'\n')

    def test_complete_byte_receipt_and_determinism_without_formal_claim(self):
        receipt = self.verify()
        self.assertEqual(receipt['validated_invocations'], 2)
        self.assertEqual(len(receipt['invocations']), 2)
        self.assertEqual(len({x['signature_sha256'] for x in receipt['invocations']}), 1)
        self.assertFalse(receipt['formal_eligibility_established'])
        self.assertEqual(receipt['batch_report_sha256'], batch.digest(self.root/'report.json'))
        self.assertFalse(json.loads((self.root/'report.json').read_text())['correctness_validated'])

    def test_flags_alone_cannot_attest_outputs_or_resources(self):
        mutations = [lambda r: r.update(correctness_validated=True),
                     lambda r: r.update(execution_passed=1),
                     lambda r: r.update(failures=['failure']),
                     lambda r: r.update(returncode=False),
                     lambda r: r.update(elapsed_seconds=True),
                     lambda r: r['resources'].update(observer_error='lost observation'),
                     lambda r: r['resources'].update(samples=[]),
                     lambda r: r['resources'].update(observed_peak_logical_file_bytes=0),
                     lambda r: r['resources']['samples'][0]['processes'].update(rss_bytes=True),
                     lambda r: r['resources']['samples'][0]['files'].update(available_bytes=0),
                     lambda r: r['resources']['samples'][0]['files'].update(logical_bytes=2*1024**3),
                     lambda r: r['resources']['samples'][0]['processes'].update(rss_bytes=2*1024**3),
                     lambda r: r['gnu_time'].update(max_rss_kib=0),
                     lambda r: r['filesystem'].update(fstype='tmpfs'),
                     lambda r: r['limits'].update(seconds=20)]
        for mutate in mutations:
            raw = copy.deepcopy(self.raw)
            mutate(raw)
            self.report(raw)
            with self.subTest(mutation=mutations.index(mutate)), self.assertRaises(ValueError):
                self.verify()
        self.report(self.raw)
        self.verify()

    def test_missing_or_extra_events_rejected_even_with_passed_flag(self):
        original = (self.root/'events.jsonl').read_bytes()
        for events in [self.raw['events'][:1], self.raw['events']*2]:
            raw = copy.deepcopy(self.raw)
            raw['events'] = events
            self.report(raw)
            (self.root/'events.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in events))
            with self.assertRaises(ValueError):
                self.verify()
        (self.root/'events.jsonl').write_bytes(original)

    def test_typed_event_command_affinity_exit_and_pid_changes_rejected(self):
        for field, value in [('index', False), ('pid', True), ('start_ticks', False),
                             ('command', ['wrong']), ('returncode', False), ('initial_affinity', [0])]:
            raw = copy.deepcopy(self.raw)
            raw['events'][0][field] = value
            self.report(raw)
            (self.root/'events.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in raw['events']))
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.verify()
        raw = copy.deepcopy(self.raw)
        for field in ['pid', 'start_ticks']:
            raw['events'][1][field] = raw['events'][0][field]
        self.report(raw)
        (self.root/'events.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in raw['events']))
        with self.assertRaises(ValueError):
            self.verify()

    def test_changed_plan_and_self_consistent_wrong_spec_rejected(self):
        raw = copy.deepcopy(self.raw)
        raw['spec']['count'] = 1
        (self.root/'plan.json').write_text(json.dumps(raw['spec']))
        raw['plan_sha256_before'] = raw['plan_sha256_after'] = batch.digest(self.root/'plan.json')
        self.report(raw)
        with self.assertRaises(ValueError):
            self.verify()

    def test_frozen_executable_identity_required(self):
        identities = dict(self.identities)
        identities[self.spec['argv'][0]] = '0'*64
        with self.assertRaises(ValueError):
            self.verify(expected_identities=identities)
        raw = copy.deepcopy(self.raw)
        for field in ['identities_before', 'identities_after']:
            raw[field][self.spec['argv'][0]] = '0'*64
        self.report(raw)
        with self.assertRaises(ValueError):
            self.verify(expected_identities=identities)

    def test_every_invocation_output_and_diagnostic_is_checked(self):
        for name in ['out/data', 'stdout', 'stderr']:
            path = self.root/'run-000001'/name
            original = path.read_bytes()
            path.write_bytes(original+b'changed')
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.verify()
            path.write_bytes(original)
        path = self.root/'batch.stderr'
        path.write_text('helper warning')
        with self.assertRaises(ValueError):
            self.verify()

    def test_extra_file_directory_and_missing_artifact_rejected(self):
        for parent in [self.root, self.root/'run-000000', self.root/'run-000001/tmp']:
            path = parent/'unexpected'
            path.write_bytes(b'')
            with self.assertRaises(ValueError):
                self.verify()
            path.unlink()
            path.mkdir()
            with self.assertRaises(ValueError):
                self.verify()
            path.rmdir()
        (self.root/'run-000001/out/data').unlink()
        with self.assertRaises(ValueError):
            self.verify()

    def test_symlink_fifo_and_hardlink_artifacts_rejected(self):
        path = self.root/'run-000001/out/data'
        path.unlink()
        path.symlink_to(self.root/'run-000000/out/data')
        with self.assertRaises(ValueError):
            self.verify()
        path.unlink()
        os.mkfifo(path)
        with self.assertRaises(ValueError):
            self.verify()
        path.unlink()
        os.link(self.root/'run-000000/out/data', path)
        with self.assertRaises(ValueError):
            self.verify()

    def test_contradictory_gnu_time_metrics_and_exit_are_rejected(self):
        path = self.root/'time.txt'
        original = path.read_text()
        for text in ['Maximum resident set size (kbytes): 999999999999\n'+original,
                     original.replace('Exit status: 0', 'Exit status: 7'),
                     original+'Command terminated by signal 9\n']:
            path.write_text(text)
            with self.subTest(text=text[:50]), self.assertRaises(ValueError):
                self.verify()
        path.write_text(original)
        self.verify()

    def test_terminal_observation_marker_is_required_only_at_end(self):
        for mode in ['missing', 'earlier', 'before_completion']:
            raw = copy.deepcopy(self.raw)
            last = raw['resources']['samples'][-1]
            if mode == 'missing':
                del last['invocation_elapsed_seconds']
            elif mode == 'earlier':
                # The observer may legitimately retain only its final sample.
                # Insert a distinct earlier entry before marking it, so this
                # control never merely rewrites the sole terminal marker.
                raw['resources']['samples'].insert(
                    0, copy.deepcopy(raw['resources']['samples'][0]))
                raw['resources']['samples'][0]['invocation_elapsed_seconds'] = raw['elapsed_seconds']
            else:
                last['elapsed_seconds'] = raw['elapsed_seconds'] / 2
            self.report(raw)
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                self.verify()

    def test_mistyped_empty_process_collections_are_not_observations(self):
        for members in [{}, '']:
            raw = copy.deepcopy(self.raw)
            raw['resources']['samples'][-1]['processes']['members'] = members
            raw['resources']['samples'][-1]['processes']['rss_bytes'] = 0
            self.report(raw)
            with self.subTest(members=members), self.assertRaises(ValueError):
                self.verify()

    def test_expected_nonzero_cli_exit_keeps_partial_files_and_every_invocation(self):
        root = self.root.parent/'expected-error'
        spec = dict(self.spec, expected_exit=7, argv=[sys.executable, '-c',
                    'from pathlib import Path; import sys; Path("partial").write_bytes(b"kept"); print("expected",file=sys.stderr); sys.exit(7)'])
        result = batch.run_batch(spec, root, dict(os.environ), self.limits)
        self.assertTrue(result['execution_passed'], result['failures'])
        receipt = self.verify(root=root, spec=spec, expected_directories=['tmp'],
                              expected_files={'stdout': sha(b''), 'stderr': sha(b'expected\n'), 'partial': sha(b'kept')})
        self.assertEqual(receipt['validated_invocations'], 2)
        self.assertFalse(receipt['formal_eligibility_established'])

    def test_incomplete_contract_and_duplicate_json_fail_closed(self):
        for changes in [dict(expected_files={'stdout': sha(b'ok\n')}),
                        dict(expected_files={**self.outputs, '../escape': sha(b'')}),
                        dict(expected_directories=['tmp', 'out', 'out']),
                        dict(expected_files={**self.outputs, 'out/data': 'not-a-hash'})]:
            with self.assertRaises(ValueError):
                self.verify(**changes)
        (self.root/'report.json').write_text('{"execution_passed":false,"execution_passed":true}')
        with self.assertRaisesRegex(ValueError, 'duplicate JSON key'):
            self.verify()
        for value in ['NaN', 'Infinity', '-Infinity']:
            with self.assertRaises(ValueError):
                decode('{"time":'+value+'}')


if __name__ == '__main__':
    unittest.main()
