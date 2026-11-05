#!/usr/bin/env python3
"""Independent process/file controls for sampled benchmark resource guards."""
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from superiority_resources import Guard, Limits, files, parse_process


@unittest.skipUnless(sys.platform == 'linux', 'Linux procfs observer')
class ResourceGuardTests(unittest.TestCase):
    def exercise(self, code, **overrides):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            limits = Limits(**dict({'rss_bytes': 1024**3, 'files_bytes': 1024**3,
                                   'headroom_bytes': 1, 'seconds': 5.0,
                                   'interval_seconds': .02}, **overrides))
            started = time.monotonic()
            with (root/'stdout').open('wb') as out, (root/'stderr').open('wb') as err:
                process = subprocess.Popen([sys.executable, '-c', code], cwd=root,
                                           stdout=out, stderr=err, start_new_session=True)
                guard = Guard(process.pid, root, limits, started)
                try:
                    guard.start()
                    status = process.wait(timeout=8)
                finally:
                    result = guard.stop()
                    if process.poll() is None:
                        guard.kill()
                        process.wait(timeout=3)
            return status, result, (root/'stdout').read_bytes()

    def test_real_descendant_sum_triggers_when_individual_processes_do_not(self):
        alloc = 'b=bytearray(24*1024*1024); b[::4096]=b"x"*len(b[::4096]);'
        child = 'import time;'+alloc+'time.sleep(4)'
        code = ('import subprocess,sys,time; p=subprocess.Popen([sys.executable,"-c",'+repr(child)+']);'
                'print(p.pid,flush=True);'+alloc+'time.sleep(4)')
        limit = 56 * 1024**2
        status, result, output = self.exercise(code, rss_bytes=limit)
        self.assertNotEqual(status, 0)
        self.assertIn('observed aggregate RSS exceeds limit', result['failures'])
        triggering = [s for s in result['samples'] if s['processes']['rss_bytes'] > limit]
        self.assertTrue(triggering)
        self.assertTrue(all(p['rss_bytes'] < limit for p in triggering[0]['processes']['members']))
        pid = int(output.strip())
        try:
            child_stat = parse_process(pid, Path(f'/proc/{pid}/stat').read_text())
        except FileNotFoundError:
            pass
        else:
            self.assertEqual(child_stat['state'], 'Z', 'descendant survived guard termination')

    def test_real_file_growth_aborts_and_keeps_evidence(self):
        status, result, _ = self.exercise('from pathlib import Path; import time; Path("payload").write_bytes(b"x"*16384); time.sleep(4)', files_bytes=8192)
        self.assertNotEqual(status, 0)
        self.assertIn('observed temporary/output bytes exceed limit', result['failures'])
        self.assertGreaterEqual(result['observed_peak_logical_file_bytes'], 16384)

    def test_timeout_is_independent_of_sampling_interval(self):
        status, result, _ = self.exercise('import time; time.sleep(4)', seconds=.2, interval_seconds=.2)
        self.assertNotEqual(status, 0)
        self.assertIn('invocation timeout', result['failures'])
        self.assertLess(result['samples'][-1]['elapsed_seconds'], 2)

    def test_joining_a_slow_observer_is_not_charged_to_the_cli_timeout(self):
        original = files
        def slow_scan(root):
            time.sleep(.35)
            return original(root)
        with patch('superiority_resources.files', side_effect=slow_scan):
            status, result, _ = self.exercise('import time; time.sleep(.04)', seconds=.3, interval_seconds=.1)
        self.assertEqual(status, 0)
        self.assertFalse(result['failures'], result)
        self.assertGreater(result['samples'][-1]['elapsed_seconds'], .3)
        self.assertLess(result['samples'][-1]['invocation_elapsed_seconds'], .3)

    def test_insufficient_headroom_aborts(self):
        status, result, _ = self.exercise('import time; time.sleep(4)', headroom_bytes=2**63)
        self.assertNotEqual(status, 0)
        self.assertIn('filesystem headroom below limit', result['failures'])

    def test_observer_failure_aborts_instead_of_recording_a_pass(self):
        status, result, _ = self.exercise('import os,time; os.symlink("/", "escape"); time.sleep(4)')
        self.assertNotEqual(status, 0)
        self.assertIn('resource observer failed', result['failures'])
        self.assertIn('symlink', result['observer_error'])

    def test_success_keeps_final_short_lived_output_observation(self):
        status, result, _ = self.exercise('from pathlib import Path; Path("result").write_bytes(b"x"*2048)')
        self.assertEqual(status, 0)
        self.assertFalse(result['failures'])
        self.assertGreaterEqual(result['observed_peak_logical_file_bytes'], 2048)
        self.assertTrue(result['limitations'])

    def test_hard_links_are_counted_once_and_sparse_files_keep_both_sizes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root/'sparse'
            with path.open('wb') as stream:
                stream.seek(1024**2-1)
                stream.write(b'x')
            os.link(path, root/'alias')
            result = files(root)
            self.assertEqual(result['logical_bytes'], 1024**2)
            self.assertEqual(result['file_count'], 1)
            self.assertLessEqual(result['allocated_bytes'], result['logical_bytes'])

    def test_stat_parser_handles_closing_parentheses_and_newlines_in_comm(self):
        pid = os.getpid()
        original = Path(f'/proc/{pid}/stat').read_text()
        expected = parse_process(pid, original)
        transformed = f'{pid} (strange ) name\n)'+original[original.rfind(')')+1:]
        self.assertEqual(parse_process(pid, transformed), expected)
        with self.assertRaises(ValueError):
            parse_process(pid+1, original)

    def test_invalid_limits_are_rejected(self):
        for field in ('rss_bytes', 'files_bytes', 'headroom_bytes', 'seconds', 'interval_seconds'):
            for value in (0, -1, True, math.nan, math.inf, '1'):
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    Limits(**{field: value})


if __name__ == '__main__':
    unittest.main()
