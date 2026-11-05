#!/usr/bin/env python3
"""Real fresh-process and failure-population checks for the batch primitive."""
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest

from superiority_batch import calibrated_count, run_batch, validate_spec
from superiority_resources import Limits

ROOT = Path(__file__).resolve().parents[1]
BASE = {'schema_version': 1, 'count': 3, 'argv': [sys.executable, '-c', 'print("ok")'],
        'stdin': None, 'expected_exit': 0, 'cores': 1}


@unittest.skipUnless(sys.platform == 'linux' and 4 in os.sched_getaffinity(0), 'requires local CPU4 topology')
class BatchTests(unittest.TestCase):
    def execute(self, argv, *, count=3, expected_exit=0, **limit_changes):
        (ROOT/'target').mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix='batch-control-', dir=ROOT/'target')
        self.addCleanup(self.temporary.cleanup)
        directory = Path(self.temporary.name)/'batch'
        spec = dict(BASE, argv=argv, count=count, expected_exit=expected_exit)
        limits = Limits(**dict({'rss_bytes': 1024**3, 'files_bytes': 1024**3,
                               'headroom_bytes': 1, 'seconds': 5, 'interval_seconds': .02}, **limit_changes))
        report = run_batch(spec, directory, dict(os.environ), limits)
        self.assertEqual(json.loads((directory/'report.json').read_text()), report)
        return report, directory

    def test_every_invocation_is_fresh_and_all_outputs_are_retained(self):
        code = 'import os,sys; from pathlib import Path; print(os.getpid()); Path(sys.argv[1]).write_bytes(b"ACGT\\n")'
        report, directory = self.execute([sys.executable, '-c', code, {'run_path': 'out.fa'}])
        self.assertTrue(report['execution_passed'], report['failures'])
        self.assertFalse(report['correctness_validated'])
        self.assertEqual(len(report['events']), 3)
        for event in report['events']:
            output = directory/f"run-{event['index']:06d}"
            self.assertEqual(int((output/'stdout').read_text()), event['pid'])
            self.assertEqual((output/'out.fa').read_bytes(), b'ACGT\n')
            self.assertEqual(event['initial_affinity'], [4])
            self.assertGreater(event['start_ticks'], 0)
        self.assertEqual(report['mean_invocation_seconds'], report['elapsed_seconds']/3)
        self.assertGreater(report['gnu_time']['max_rss_kib'], 0)
        self.assertTrue(report['resources']['samples'][0]['system'])

    def test_nonzero_failure_preserves_incomplete_population_without_rates(self):
        code = 'from pathlib import Path; Path("partial").write_text("kept"); raise SystemExit(7)'
        report, directory = self.execute([sys.executable, '-c', code])
        self.assertFalse(report['execution_passed'])
        self.assertEqual(len(report['events']), 1)
        self.assertEqual(report['events'][0]['returncode'], 7)
        self.assertIn('incomplete fresh-process population', report['failures'])
        self.assertIsNone(report['mean_invocation_seconds'])
        self.assertIsNone(report['invocations_per_second'])
        self.assertEqual((directory/'run-000000/partial').read_text(), 'kept')

    def test_expected_error_still_executes_and_preserves_every_invocation(self):
        report, _ = self.execute([sys.executable, '-c', 'import sys; print("expected",file=sys.stderr); sys.exit(7)'], expected_exit=7)
        self.assertTrue(report['execution_passed'], report['failures'])
        self.assertEqual([e['returncode'] for e in report['events']], [7]*3)

    def test_actual_file_cap_aborts_the_batch_and_keeps_partial_output(self):
        code = 'from pathlib import Path; import time; Path("large").write_bytes(b"x"*65536); time.sleep(4)'
        report, directory = self.execute([sys.executable, '-c', code], files_bytes=32768)
        self.assertFalse(report['execution_passed'])
        self.assertIn('observed temporary/output bytes exceed limit', report['failures'])
        self.assertEqual((directory/'run-000000/large').stat().st_size, 65536)
        self.assertIsNone(report['mean_invocation_seconds'])

    def test_timeout_does_not_turn_a_shortened_population_into_fast_result(self):
        report, _ = self.execute([sys.executable, '-c', 'import time; time.sleep(4)'], seconds=.2)
        self.assertFalse(report['execution_passed'])
        self.assertIn('invocation timeout', report['failures'])
        self.assertIsNone(report['mean_invocation_seconds'])

    def test_descendant_outliving_cli_is_rejected_and_terminated(self):
        code = 'import subprocess,sys; p=subprocess.Popen([sys.executable,"-c","import time; time.sleep(4)"]); print(p.pid)'
        report, directory = self.execute([sys.executable, '-c', code], count=1)
        self.assertFalse(report['execution_passed'])
        self.assertIn('descendants outlived the batch helper', report['failures'])
        pid = int((directory/'run-000000/stdout').read_text())
        for _ in range(100):
            try:
                text = Path(f'/proc/{pid}/stat').read_text()
            except FileNotFoundError:
                break
            if text[text.rfind(')')+1:].split()[0] == 'Z':
                break
            time.sleep(.01)
        else:
            self.fail('orphaned descendant survived batch cleanup')

    def test_mutated_plan_is_not_accepted(self):
        code = 'from pathlib import Path; Path("../plan.json").write_text("{}")'
        report, _ = self.execute([sys.executable, '-c', code])
        self.assertFalse(report['execution_passed'])
        self.assertIn('batch plan changed', report['failures'])

    def test_literal_shell_syntax_is_passed_as_an_argument(self):
        text = '$(touch SHOULD_NOT_EXIST) `id`; echo no'
        report, directory = self.execute([sys.executable, '-c', 'import sys; print(sys.argv[1])', text])
        self.assertTrue(report['execution_passed'], report['failures'])
        self.assertEqual((directory/'run-000000/stdout').read_text().strip(), text)
        self.assertFalse((directory/'run-000000/SHOULD_NOT_EXIST').exists())


class BatchPlanTests(unittest.TestCase):
    def test_count_calibration_uses_fastest_arm_and_only_grows_count(self):
        self.assertEqual(calibrated_count(10, [2, 7, 3], 15), 75)
        self.assertEqual(calibrated_count(10, [16, 25], 15), 10)
        for values in ([], [0], [float('nan')], [True], [-1]):
            with self.assertRaises(ValueError):
                calibrated_count(10, values)

    def test_rejects_path_escape_unknown_fields_and_invalid_counts(self):
        for changes in ({'count': 0}, {'count': True}, {'schema_version': True}, {'cores': True},
                        {'extra': 1}, {'stdin': 'relative'}, {'expected_exit': -1},
                        {'argv': ['relative-executable']}, {'argv': [sys.executable, {'run_path': '../escape'}]},
                        {'argv': [sys.executable, {'run_path': '/escape'}]}, {'argv': [sys.executable, {'unknown': 'x'}]}):
            spec = copy.deepcopy(BASE)
            spec.update(changes)
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                validate_spec(spec)


if __name__ == '__main__':
    unittest.main()
