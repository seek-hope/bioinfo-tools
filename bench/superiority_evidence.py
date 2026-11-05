#!/usr/bin/env python3
"""Read-only raw batch verification for deterministic, complete-byte contracts.

This does not authenticate telemetry, establish equal work, check family coverage,
validate arbitrary report semantics, or prove input/environment isolation during
execution. It supplies an independently recomputed byte receipt, not formal
acceptance. Callers must retain rejected/incomplete observations and their errors.
"""
from __future__ import annotations

from dataclasses import asdict
import hashlib
import json
import math
from pathlib import Path
import re
import stat

from superiority_batch import AFFINITIES, digest, validate_spec
from superiority_plan import fingerprint
from superiority_resources import Limits, files, violations

SHA256 = re.compile(r'[0-9a-f]{64}\Z')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def exact(left, right):
    return fingerprint(left) == fingerprint(right)


def number(value, *, positive=False, integer=False):
    require(type(value) in ((int,) if integer else (int, float))
            and math.isfinite(value) and (value > 0 if positive else value >= 0),
            'invalid numeric evidence')
    return value


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def decode(text):
    def invalid(value):
        raise ValueError(f'nonfinite JSON constant: {value}')
    return json.loads(text, object_pairs_hook=_pairs, parse_constant=invalid)


def _time_metrics(path):
    labels = {'User time (seconds)': 'user_seconds', 'System time (seconds)': 'system_seconds',
              'Maximum resident set size (kbytes)': 'max_rss_kib',
              'File system inputs': 'filesystem_inputs', 'File system outputs': 'filesystem_outputs',
              'Exit status': 'exit_status'}
    result = {}
    for line in path.read_text().splitlines():
        require(not line.strip().startswith(('Command exited with non-zero status', 'Command terminated')),
                'GNU-time reports helper termination')
        key, separator, value = line.strip().rpartition(': ')
        if separator and key in labels:
            field = labels[key]
            require(field not in result, 'duplicate GNU-time metric')
            parsed = float(value) if 'time' in key else int(value)
            result[field] = number(parsed)
    require(set(result) == set(labels.values()), 'incomplete GNU-time evidence')
    require(result.pop('exit_status') == 0, 'GNU-time helper exit differs from successful batch')
    return result


def _relative(value):
    require(type(value) is str and bool(value) and '\0' not in value,
            'invalid artifact name')
    path = Path(value)
    require(not path.is_absolute() and '..' not in path.parts and '.' != value
            and path.as_posix() == value, 'artifact name must be canonical and relative')
    return value


def _hashes(value):
    require(type(value) is dict and bool(value), 'nonempty frozen hash map required')
    for path, checksum in value.items():
        require(type(path) is str and type(checksum) is str and SHA256.fullmatch(checksum),
                'invalid frozen identity')


def _inventory(root):
    require(root == root.resolve(strict=True), 'artifact root contains a symlink')
    device = root.stat().st_dev
    directories, regular, seen = set(), {}, set()
    for path in [root, *root.rglob('*')]:
        info = path.lstat()
        require(info.st_dev == device and not stat.S_ISLNK(info.st_mode),
                'symlink or cross-filesystem artifact')
        name = path.relative_to(root).as_posix()
        if stat.S_ISDIR(info.st_mode):
            if path != root:
                directories.add(name)
        else:
            require(stat.S_ISREG(info.st_mode), 'nonregular artifact')
            identity = info.st_dev, info.st_ino
            require(identity not in seen, 'hard-linked artifact aliases')
            seen.add(identity)
            regular[name] = digest(path)
    return directories, regular


def validate_byte_batch(root, spec, expected_identities, expected_files,
                        *, expected_directories=('tmp',), limits=Limits()):
    """Check every invocation against a caller-frozen complete file-byte contract.

    `expected_files` includes stdout/stderr and every result/partial file, even
    empty ones; expected directories include the prepared tmp directory. Outputs
    must be deterministic across invocations. Identities must be frozen before
    execution, not copied from the report to be checked. Return a receipt without
    modifying evidence. No timing minimum or comparison margin is implied.
    """
    validate_spec(spec)
    require(isinstance(limits, Limits), 'explicit Limits required')
    _hashes(expected_identities)
    _hashes(expected_files)
    spec = decode(json.dumps(spec))
    identities = dict(expected_identities)
    expected_files = dict(expected_files)
    expected_directories = list(expected_directories)
    for path in expected_files:
        _relative(path)
    for path in expected_directories:
        _relative(path)
    require(len(set(expected_directories)) == len(expected_directories), 'duplicate artifact directory')
    require({'stdout', 'stderr'} <= expected_files.keys() and 'tmp' in expected_directories,
            'complete stdout/stderr/tmp contract required')
    require(not expected_files.keys() & set(expected_directories), 'artifact kind conflict')
    root = Path(root).absolute()
    directories, inventory = _inventory(root)
    raw = decode((root/'report.json').read_text())
    require(raw['execution_passed'] is True and raw['correctness_validated'] is False
            and raw['failures'] == [] and type(raw['returncode']) is int and raw['returncode'] == 0,
            'failed or self-attested execution')
    require(exact(raw['spec'], spec) and exact(decode((root/'plan.json').read_text()), spec),
            'batch specification differs from frozen contract')
    plan_hash = inventory['plan.json']
    require(plan_hash == raw['plan_sha256_before'] == raw['plan_sha256_after'], 'plan digest changed')
    require(exact(raw['identities_before'], identities) and exact(raw['identities_after'], identities),
            'measurement identities differ from frozen contract')
    for path, checksum in identities.items():
        require(Path(path).is_absolute() and digest(path) == checksum, 'measurement identity changed')
    require(spec['argv'][0] in identities, 'CLI executable identity missing')
    cpus = sorted(map(int, AFFINITIES[spec['cores']].split(',')))
    events = [decode(line) for line in (root/'events.jsonl').read_text().splitlines()]
    require(exact(events, raw['events']) and len(events) == spec['count'], 'incomplete or changed events')
    wanted_files = {'report.json', 'plan.json', 'events.jsonl', 'time.txt', 'batch.stdout', 'batch.stderr'}
    wanted_dirs, process_ids, invocations = set(), set(), []
    signature = fingerprint({'exit': spec['expected_exit'], 'files': expected_files,
                             'directories': sorted(expected_directories)})
    for index, event in enumerate(events):
        name = f'run-{index:06d}'
        directory = root/name
        pid = number(event['pid'], positive=True, integer=True)
        start = number(event['start_ticks'], positive=True, integer=True)
        require(pid > 1 and (pid, start) not in process_ids, 'duplicate/invalid CLI process identity')
        process_ids.add((pid, start))
        command = [str(directory/part['run_path']) if isinstance(part, dict) else part for part in spec['argv']]
        require(exact(event, {'index': index, 'pid': pid, 'start_ticks': start, 'command': command,
                             'returncode': spec['expected_exit'], 'initial_affinity': cpus}),
                'invocation command/exit/affinity differs from contract')
        wanted_dirs.add(name)
        wanted_dirs.update(f'{name}/{path}' for path in expected_directories)
        for path, checksum in expected_files.items():
            full = f'{name}/{path}'
            wanted_files.add(full)
            require(inventory.get(full) == checksum, 'output bytes differ from complete contract')
        invocations.append({'index': index, 'pid': pid, 'start_ticks': start, 'signature_sha256': signature})
    require(set(inventory) == wanted_files and directories == wanted_dirs, 'unaccounted/missing artifacts')
    empty = hashlib.sha256(b'').hexdigest()
    require(inventory['batch.stdout'] == inventory['batch.stderr'] == empty, 'unexpected helper diagnostics')
    elapsed = number(raw['elapsed_seconds'], positive=True)
    require(elapsed <= limits.seconds, 'elapsed resource limit exceeded')
    metrics = _time_metrics(root/'time.txt')
    require(exact(metrics, raw['gnu_time']), 'GNU-time evidence changed')
    require(metrics['max_rss_kib']*1024 <= limits.rss_bytes, 'GNU-time RSS resource limit exceeded')
    require(exact(raw['limits'], asdict(limits)), 'resource envelope changed')
    resource = raw['resources']
    require(exact(resource['limits'], asdict(limits)) and resource['failures'] == []
            and resource['observer_error'] is None and resource['samples'], 'resource observation failed')
    samples = resource['samples']
    require(type(samples) is list and all(type(s) is dict for s in samples),
            'resource samples must be a nonempty list of objects')
    require([i for i, s in enumerate(samples) if 'invocation_elapsed_seconds' in s] == [len(samples)-1],
            'terminal resource observation required exactly once at the end')
    require(number(samples[-1]['elapsed_seconds']) >= elapsed, 'terminal observation precedes exit')
    last_elapsed = -1
    for sample in samples:
        current_elapsed = number(sample['elapsed_seconds'])
        require(current_elapsed >= last_elapsed, 'resource sample order changed')
        last_elapsed = current_elapsed
        number(sample['scan_seconds'])
        if 'invocation_elapsed_seconds' in sample:
            require(exact(sample['invocation_elapsed_seconds'], elapsed), 'final elapsed evidence changed')
        process = sample['processes']
        require(type(process) is dict, 'invalid process observation')
        rss = number(process['rss_bytes'], integer=True)
        members = process['members']
        require(type(members) is list and all(type(m) is dict for m in members),
                'process members must be a list of objects')
        sampled_ids = set()
        for member in members:
            require(set(member) == {'pid', 'state', 'ppid', 'pgrp', 'session', 'start_ticks', 'rss_bytes'},
                    'invalid process member fields')
            require(type(member['state']) is str and len(member['state']) == 1,
                    'invalid process state')
            for field in ('ppid', 'pgrp', 'session'):
                number(member[field], integer=True)
            key = (number(member['pid'], positive=True, integer=True),
                   number(member['start_ticks'], positive=True, integer=True))
            require(key not in sampled_ids, 'duplicate sampled process identity')
            sampled_ids.add(key)
            number(member['rss_bytes'], integer=True)
        require(rss == sum(member['rss_bytes'] for member in members), 'aggregate RSS sum differs')
        for field in ('logical_bytes', 'allocated_bytes', 'file_count', 'available_bytes'):
            number(sample['files'][field], integer=True)
        require(not violations(sample, limits), 'observed resource limit exceeded')
    require(all(m['state'] == 'Z' for m in resource['samples'][-1]['processes']['members']),
            'descendants survived helper exit')
    for field, group, key in [('observed_peak_aggregate_rss_bytes', 'processes', 'rss_bytes'),
                              ('observed_peak_logical_file_bytes', 'files', 'logical_bytes'),
                              ('observed_peak_allocated_file_bytes', 'files', 'allocated_bytes')]:
        number(resource[field], integer=True)
        require(resource[field] == max(s[group][key] for s in resource['samples']), 'resource peak differs from samples')
    require(type(raw['filesystem']['fstype']) is str and raw['filesystem']['fstype'] not in ('tmpfs', 'ramfs', ''),
            'recorded disk filesystem required')
    final = files(root)
    require(max(final['logical_bytes'], final['allocated_bytes']) <= limits.files_bytes
            and final['available_bytes'] >= limits.headroom_bytes, 'final artifact resource limit exceeded')
    # Catch ordinary accidental mutation during verification, not adversarial
    # TOCTOU. This tool is not a sandbox or signed telemetry attestation service.
    require(_inventory(root) == (directories, inventory), 'artifacts changed during verification')
    return {'validated_invocations': len(invocations), 'invocations': invocations,
            'signature_sha256': signature, 'batch_report_sha256': inventory['report.json'],
            'artifact_manifest_sha256': fingerprint({'directories': sorted(directories), 'files': inventory}),
            'elapsed_seconds': elapsed, 'resource_observations_checked': True,
            'formal_eligibility_established': False, 'final_workspace': final,
            'scope': 'every retained output/diagnostic/partial-file byte and exit matches the supplied frozen contract; raw events and observed bounds rechecked'}
