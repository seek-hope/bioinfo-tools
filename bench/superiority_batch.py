#!/usr/bin/env python3
"""Resource-observed fixed-input batches of fresh CLI processes.

This is an execution primitive, not a formal benchmark acceptance gate. The
experiment must separately freeze identities/inputs/commands and calibrated batch
counts, balance arm order, validate every invocation and compare determinism.
"""
from __future__ import annotations
from dataclasses import asdict
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

from superiority_resources import Guard, Limits, files

CHILD = Path(__file__).with_name('superiority_batch_child.py').resolve()
TIME = Path('/usr/bin/time')
AFFINITIES = {1: '4', 2: '4,6', 4: '0,2,4,6'}


def digest(path):
    checksum = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024*1024), b''):
            checksum.update(block)
    return checksum.hexdigest()


def validate_spec(spec):
    if set(spec) != {'schema_version', 'count', 'argv', 'stdin', 'expected_exit', 'cores'} or type(spec['schema_version']) is not int or spec['schema_version'] != 1:
        raise ValueError('expected exact batch schema1')
    if type(spec['count']) is not int or spec['count'] <= 0:
        raise ValueError('batch count must be a positive integer')
    if type(spec['cores']) is not int or spec['cores'] not in AFFINITIES:
        raise ValueError('explicit supported physical-core budget required')
    if type(spec['expected_exit']) is not int or not 0 <= spec['expected_exit'] <= 255:
        raise ValueError('invalid expected CLI exit code')
    if not isinstance(spec['argv'], list) or not spec['argv'] or not isinstance(spec['argv'][0], str) or not Path(spec['argv'][0]).is_absolute():
        raise ValueError('absolute executable and argument list required')
    for part in spec['argv']:
        if isinstance(part, str):
            if '\0' in part:
                raise ValueError('NUL in command argument')
        elif isinstance(part, dict) and set(part) == {'run_path'} and isinstance(part['run_path'], str):
            path = Path(part['run_path'])
            if not part['run_path'] or path.is_absolute() or '..' in path.parts:
                raise ValueError('artifact argument must stay inside its invocation')
        else:
            raise ValueError('unknown argument expansion')
    source = spec['stdin']
    if source is not None and (not isinstance(source, str) or not Path(source).is_absolute() or '\0' in source):
        raise ValueError('stdin must be an absolute input path or null')


def calibrated_count(previous: int, elapsed: list[float], target=15.0) -> int:
    """Only grow count; the caller must retain every arm/budget observation."""
    if type(previous) is not int or previous <= 0 or not elapsed:
        raise ValueError('positive previous count and complete observations required')
    if any(type(value) not in (int, float) or not math.isfinite(value) or value <= 0 for value in [*elapsed, target]):
        raise ValueError('finite positive elapsed times and target required')
    return max(previous, math.ceil(previous * target / min(elapsed)))


def disk_info(root):
    value = json.loads(subprocess.check_output(['findmnt', '-J', '-T', str(root), '-o', 'TARGET,SOURCE,FSTYPE,OPTIONS'], text=True))
    rows = value.get('filesystems', [])
    if len(rows) != 1 or rows[0].get('fstype') in (None, 'tmpfs', 'ramfs'):
        raise ValueError('batch measurements require a recorded disk filesystem')
    return rows[0]


def parse_time(path):
    labels = {'User time (seconds)': 'user_seconds', 'System time (seconds)': 'system_seconds',
              'Maximum resident set size (kbytes)': 'max_rss_kib',
              'File system inputs': 'filesystem_inputs', 'File system outputs': 'filesystem_outputs'}
    result = {}
    for line in path.read_text().splitlines():
        key, separator, value = line.strip().rpartition(': ')
        if separator and key in labels:
            result[labels[key]] = float(value) if 'time' in key else int(value)
    if set(result) != set(labels.values()) or any(not math.isfinite(v) or v < 0 for v in result.values()):
        raise ValueError('incomplete or invalid GNU-time metrics')
    return result


def run_batch(spec: dict, root: Path, env: dict, limits: Limits = Limits()) -> dict:
    """Prepare, run, retain and report one batch; no correctness pass is implied.

    Artifacts from all invocations coexist until the external verifier checks
    them. The resource guard therefore applies its file cap to the whole batch,
    including retained earlier outputs, which is stricter than a single CLI cap.
    All outputs/temporary files must be routed into this exclusive workspace.
    """
    validate_spec(spec)
    spec = json.loads(json.dumps(spec))
    root = root.absolute()
    root.mkdir(parents=True, exist_ok=False)
    report = {'execution_passed': False, 'correctness_validated': False, 'spec': spec,
              'limits': asdict(limits), 'failures': [], 'events': [],
              'timing_scope': 'fresh CLI processes plus Python startup, launch/redirection and event-write overhead; no comparisons or fsync',
              'resource_scope': 'batch helper and observed descendants; all retained workspace artifacts across the batch',
              'mean_invocation_seconds': None, 'invocations_per_second': None}
    env = dict(env, LC_ALL='C')
    try:
        report['filesystem'] = disk_info(root)
        taskset = shutil.which('taskset', path=env.get('PATH', os.defpath))
        if not taskset:
            raise ValueError('taskset unavailable')
        paths = [CHILD, Path(__file__).resolve(), Path(__file__).with_name('superiority_resources.py').resolve(),
                 Path(sys.executable).resolve(), TIME, Path(taskset), Path(spec['argv'][0])]
        report['identities_before'] = {str(path): digest(path) for path in paths}
        space = files(root)
        if space['available_bytes'] < limits.headroom_bytes:
            raise ValueError('insufficient filesystem headroom before preparation')
        cpus = {int(cpu) for cpu in AFFINITIES[spec['cores']].split(',')}
        if not cpus <= os.sched_getaffinity(0):
            raise ValueError('requested CPU affinity is unavailable')
        for index in range(spec['count']):
            (root/f'run-{index:06d}'/'tmp').mkdir(parents=True, exist_ok=False)
        plan = root/'plan.json'
        plan.write_text(json.dumps(spec, indent=2)+'\n')
        report['plan_sha256_before'] = digest(plan)
        prepared = files(root)
        if prepared['available_bytes'] < limits.headroom_bytes:
            raise ValueError('insufficient filesystem headroom after preparation')
        if max(prepared['logical_bytes'], prepared['allocated_bytes']) > limits.files_bytes:
            raise ValueError('prepared batch already exceeds artifact limit')
        command = [str(TIME), '-v', '-o', str(root/'time.txt'), taskset, '-c', AFFINITIES[spec['cores']],
                   sys.executable, str(CHILD), str(plan)]
        report['command'] = command
        with (root/'batch.stdout').open('xb') as stdout, (root/'batch.stderr').open('xb') as stderr:
            started = time.monotonic()
            process = subprocess.Popen(command, env=env, cwd=root, stdin=subprocess.DEVNULL,
                                       stdout=stdout, stderr=stderr, start_new_session=True)
            guard = None
            elapsed = None
            try:
                guard = Guard(process.pid, root, limits, started, tuple(sorted(cpus)))
                guard.start()
                code = process.wait()
                elapsed = time.monotonic()-started
            except BaseException:
                if guard is not None:
                    guard.kill()
                else:
                    os.killpg(process.pid, 9)
                process.wait()
                raise
            finally:
                if guard is not None:
                    report['resources'] = guard.stop(elapsed)
        report.update(returncode=code, elapsed_seconds=elapsed)
        report['failures'].extend(report['resources']['failures'])
        live = [p for p in report['resources']['samples'][-1]['processes']['members'] if p['state'] != 'Z'] if report['resources']['samples'] else []
        if live:
            report['failures'].append('descendants outlived the batch helper')
            guard.kill()
        try:
            report['gnu_time'] = parse_time(root/'time.txt')
            report['effective_cpus'] = (report['gnu_time']['user_seconds'] + report['gnu_time']['system_seconds']) / elapsed
            if report['gnu_time']['max_rss_kib']*1024 > limits.rss_bytes:
                report['failures'].append('GNU-time process high-water RSS exceeds limit')
        except (OSError, ValueError) as error:
            report['failures'].append(str(error))
        if (root/'events.jsonl').exists():
            for line in (root/'events.jsonl').read_text().splitlines():
                report['events'].append(json.loads(line))
        if code:
            report['failures'].append('batch helper failed')
        if len(report['events']) != spec['count']:
            report['failures'].append('incomplete fresh-process population')
        for index, event in enumerate(report['events']):
            directory = root/f'run-{index:06d}'
            expected = [str(directory/part['run_path']) if isinstance(part, dict) else part for part in spec['argv']]
            if event != {'index': index, 'pid': event.get('pid'), 'start_ticks': event.get('start_ticks'), 'command': expected,
                         'returncode': spec['expected_exit'], 'initial_affinity': sorted(cpus)} or type(event.get('pid')) is not int or type(event.get('start_ticks')) is not int:
                report['failures'].append(f'invocation contract mismatch: {index}')
        if len({(e['pid'], e['start_ticks']) for e in report['events']}) != len(report['events']):
            report['failures'].append('duplicate CLI process identity')
        begin_flush = time.monotonic()
        for path in root.rglob('*'):
            if path.is_file() and not path.is_symlink():
                with path.open('rb') as stream:
                    os.fsync(stream.fileno())
        report['untimed_fsync_seconds'] = time.monotonic()-begin_flush
        report['identities_after'] = {str(path): digest(path) for path in paths}
        report['plan_sha256_after'] = digest(plan)
        if report['plan_sha256_after'] != report['plan_sha256_before']:
            report['failures'].append('batch plan changed')
        if report['identities_before'] != report['identities_after']:
            report['failures'].append('batch executable or scripts changed')
        report['execution_passed'] = not report['failures']
        if report['execution_passed']:
            report['mean_invocation_seconds'] = elapsed/spec['count']
            report['invocations_per_second'] = spec['count']/elapsed
    except BaseException as error:
        report['failures'].append(repr(error))
        raise
    finally:
        (root/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    return report
