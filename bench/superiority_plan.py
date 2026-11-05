#!/usr/bin/env python3
"""Pure population/calibration/schedule checks for expanded paired experiments.

This module consumes normalized evidence from a future family-specific driver.
It does not run CLIs, inspect output files, or turn caller-supplied validation
flags into independent correctness evidence. Keep raw reports and their hashes.
"""
from __future__ import annotations
import copy
import hashlib
import json
import math
import random
import re
from statistics import median

ARMS = ('current', 'upstream')
BUDGETS = (1, 2, 4)
MINIMUM_SECONDS = 10.0
TARGET_SECONDS = 15.0
MAX_RATIO = 0.90


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                     allow_nan=False).encode()).hexdigest()


def exact_keys(value, keys, label):
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ValueError(f'invalid {label} fields')


def sha256(value):
    if not isinstance(value, str) or re.fullmatch('[0-9a-f]{64}', value) is None:
        raise ValueError('expected a lowercase SHA-256 identity')


def positive(value, label):
    if type(value) not in (int, float) or not math.isfinite(value) or value <= 0:
        raise ValueError(f'{label} must be finite and positive')


def validate_manifest(manifest):
    exact_keys(manifest, ('schema_version', 'seed', 'identities', 'cases'), 'experiment manifest')
    if type(manifest['schema_version']) is not int or manifest['schema_version'] != 1:
        raise ValueError('expected experiment schema1')
    if type(manifest['seed']) is not int or manifest['seed'] < 0:
        raise ValueError('explicit nonnegative integer order seed required')
    if not isinstance(manifest['identities'], dict) or not manifest['identities']:
        raise ValueError('frozen source/runtime/scripts/environment/resource identities required')
    for key, value in manifest['identities'].items():
        if not isinstance(key, str) or not key:
            raise ValueError('identity names must be nonempty strings')
        sha256(value)
    if not isinstance(manifest['cases'], list) or not manifest['cases']:
        raise ValueError('nonempty declared case population required')
    seen = set()
    for case in manifest['cases']:
        exact_keys(case, ('id', 'budgets', 'count', 'input_sha256', 'commands_sha256',
                          'comparison', 'equal_work', 'scope'), 'case')
        if not isinstance(case['id'], str) or not re.fullmatch('[A-Za-z0-9][A-Za-z0-9_.-]*', case['id']) or case['id'] in seen:
            raise ValueError('unique safe case IDs required')
        seen.add(case['id'])
        if type(case['count']) is not int or case['count'] < 1:
            raise ValueError('fixed positive invocation count required')
        budgets = case['budgets']
        if not isinstance(budgets, list) or any(type(c) is not int for c in budgets) or len(budgets) != len(BUDGETS) or set(budgets) != set(BUDGETS):
            raise ValueError('all physical-core budgets 1, 2 and 4 are required exactly once')
        sha256(case['input_sha256'])
        commands = case['commands_sha256']
        exact_keys(commands, (f'{arm}:{cores}' for cores in budgets for arm in ARMS), 'command identities')
        for value in commands.values():
            sha256(value)
        if type(case['equal_work']) is not bool or (case['comparison'] == 'descriptive' and case['equal_work']):
            raise ValueError('explicit equal-work assessment required; descriptive is not comparable')
        if case['comparison'] not in ('exact', 'proved-correction', 'descriptive') or not isinstance(case['scope'], str) or not case['scope'].strip():
            raise ValueError('explicit comparison scope required')
    return manifest


def key(row):
    return row['case'], row['cores'], row['block'], row['arm']


def expected_keys(manifest, blocks):
    return {(c['id'], cores, block, arm) for c in manifest['cases']
            for cores in c['budgets'] for block in blocks for arm in ARMS}


def validate_population(manifest, rows, blocks):
    """Reject missing/duplicate/extra evidence before considering any time."""
    validate_manifest(manifest)
    if not isinstance(rows, list):
        raise ValueError('observation list required')
    by_case = {c['id']: c for c in manifest['cases']}
    expected = expected_keys(manifest, blocks)
    observed = set()
    for row in rows:
        exact_keys(row, ('case', 'cores', 'block', 'arm', 'count', 'elapsed_seconds',
                         'manifest_sha256', 'batch_report_sha256', 'validation_report_sha256',
                         'input_sha256', 'command_sha256', 'signature_sha256',
                         'execution_passed', 'correctness_passed', 'resources_passed',
                         'validated_invocations'), 'observation')
        if not isinstance(row['case'], str) or not isinstance(row['arm'], str) or type(row['cores']) is not int or type(row['block']) is not int:
            raise ValueError('invalid observation identity')
        identity = key(row)
        if identity not in expected or identity in observed:
            raise ValueError('unexpected or duplicate observation')
        observed.add(identity)
        case = by_case[row['case']]
        if type(row['count']) is not int or row['count'] != case['count']:
            raise ValueError('invocation count differs from the frozen condition')
        if row['manifest_sha256'] != fingerprint(manifest) or row['input_sha256'] != case['input_sha256'] or row['command_sha256'] != case['commands_sha256'][f"{row['arm']}:{row['cores']}"]:
            raise ValueError('source/input/command identities differ from the plan')
        for name in ('batch_report_sha256', 'validation_report_sha256'):
            sha256(row[name])
        # Failed/incomplete observations stay representable, but cannot yield rates.
        for name in ('execution_passed', 'correctness_passed', 'resources_passed'):
            if type(row[name]) is not bool:
                raise ValueError('validation outcomes must be booleans')
        if type(row['validated_invocations']) is not int or not 0 <= row['validated_invocations'] <= row['count']:
            raise ValueError('invalid validated invocation population')
        if row['elapsed_seconds'] is not None:
            positive(row['elapsed_seconds'], 'elapsed time')
        if row['signature_sha256'] is not None:
            sha256(row['signature_sha256'])
    if observed != expected:
        raise ValueError('incomplete planned observation population')


def failures(row):
    problems = [name for name in ('execution_passed', 'correctness_passed', 'resources_passed')
                if row[name] is not True]
    if row['validated_invocations'] != row['count']:
        problems.append('not every invocation was validated')
    if row['elapsed_seconds'] is None:
        problems.append('elapsed time unavailable')
    if row['signature_sha256'] is None:
        problems.append('complete output signature unavailable')
    return problems


def calibration(manifest, rows):
    """One complete paired round; never resize inputs or omit a faster budget."""
    validate_population(manifest, rows, [0])
    proposed = copy.deepcopy(manifest)
    summaries = []
    for case, next_case in zip(manifest['cases'], proposed['cases'], strict=True):
        selected = [row for row in rows if row['case'] == case['id']]
        problems = [f'{key(r)}: {reason}' for r in selected for reason in failures(r)]
        native_signatures = {r['signature_sha256'] for r in selected if r['arm'] == 'current'}
        if len(native_signatures) != 1 or None in native_signatures:
            problems.append('native calibration signatures differ across CPU budgets')
        if case['comparison'] == 'exact':
            for cores in case['budgets']:
                signatures = {r['signature_sha256'] for r in selected if r['cores'] == cores}
                if len(signatures) != 1 or None in signatures:
                    problems.append(f'calibration output signatures differ at {cores} cores')
        if problems:
            summaries.append({'case': case['id'], 'qualified': False, 'failures': problems,
                              'minimum_seconds': None, 'proposed_count': None})
            continue
        minimum = min(r['elapsed_seconds'] for r in selected)
        count = max(case['count'], math.ceil(case['count'] * TARGET_SECONDS / minimum))
        next_case['count'] = count
        summaries.append({'case': case['id'], 'qualified': minimum >= MINIMUM_SECONDS,
                          'failures': [], 'minimum_seconds': minimum, 'proposed_count': count})
    return {'qualified': all(r['qualified'] for r in summaries), 'conditions': summaries,
            'proposed_manifest': proposed if not any(r['failures'] for r in summaries) else None}


def schedule(manifest):
    """Freeze one warmup plus three measured pairs; AB/BA balanced over four pairs."""
    validate_manifest(manifest)
    rng = random.Random(manifest['seed'])
    conditions = [(c['id'], cores) for c in manifest['cases'] for cores in c['budgets']]
    rng.shuffle(conditions)
    result = []
    for case, cores in conditions:
        order = [ARMS, ARMS, ARMS[::-1], ARMS[::-1]]
        rng.shuffle(order)
        for block, arms in zip((-1, 0, 1, 2), order, strict=True):
            for arm in arms:
                result.append({'case': case, 'cores': cores, 'block': block, 'arm': arm})
    return result


def freeze(manifest, rows):
    result = calibration(manifest, rows)
    if not result['qualified']:
        raise ValueError('all calibration arms/budgets must pass and reach ten seconds')
    # Freeze the counts actually measured, not a newly suggested unmeasured count.
    return {'schema_version': 1, 'manifest': copy.deepcopy(manifest),
            'manifest_sha256': fingerprint(manifest), 'calibration_sha256': fingerprint(rows),
            'schedule': schedule(manifest), 'threshold': MAX_RATIO}


def summarize(frozen, calibration_rows, rows):
    """No rates for ineligible populations; no averaging away losing conditions."""
    exact_keys(frozen, ('schema_version', 'manifest', 'manifest_sha256',
                        'calibration_sha256', 'schedule', 'threshold'), 'frozen experiment')
    if type(frozen['schema_version']) is not int or frozen['schema_version'] != 1 or type(frozen['threshold']) is not float or frozen['threshold'] != MAX_RATIO:
        raise ValueError('invalid frozen protocol version or margin')
    expected_frozen = freeze(frozen['manifest'], calibration_rows)
    if fingerprint(frozen) != fingerprint(expected_frozen):
        raise ValueError('frozen experiment/calibration/schedule changed')
    manifest = frozen['manifest']
    validate_population(manifest, rows, [-1, 0, 1, 2])
    actual_order = [{name: row[name] for name in ('case', 'cores', 'block', 'arm')} for row in rows]
    if actual_order != frozen['schedule']:
        raise ValueError('actual execution order differs from the frozen paired schedule')
    summaries = []
    for case in manifest['cases']:
        all_native = [r for r in [*calibration_rows, *rows]
                      if r['case'] == case['id'] and r['arm'] == 'current']
        native_signatures = {r['signature_sha256'] for r in all_native}
        for cores in case['budgets']:
            selected = [r for r in rows if r['case'] == case['id'] and r['cores'] == cores]
            problems = [f'{key(r)}: {reason}' for r in selected for reason in failures(r)]
            if len(native_signatures) != 1 or None in native_signatures:
                problems.append('native signatures differ across calibration/blocks/CPU budgets')
            if any(r['elapsed_seconds'] is not None and r['elapsed_seconds'] < MINIMUM_SECONDS for r in selected):
                problems.append('formal observation shorter than ten seconds; retain, do not resize/drop')
            if case['comparison'] == 'exact':
                for block in [-1, 0, 1, 2]:
                    pair = [r for r in selected if r['block'] == block]
                    if pair[0]['signature_sha256'] != pair[1]['signature_sha256']:
                        problems.append(f'complete output signatures differ in block {block}')
            if not case['equal_work']:
                problems.append('equal-work comparison not established')
            eligible = not problems and case['comparison'] != 'descriptive'
            ratios = None
            if eligible:
                ratios = [next(r['elapsed_seconds'] for r in selected if r['block'] == b and r['arm'] == 'current') /
                          next(r['elapsed_seconds'] for r in selected if r['block'] == b and r['arm'] == 'upstream')
                          for b in [0, 1, 2]]
                if any(not math.isfinite(r) or r <= 0 for r in ratios):
                    raise ValueError('invalid paired ratio')
            value = median(ratios) if ratios is not None else None
            summaries.append({'case': case['id'], 'cores': cores, 'comparison': case['comparison'],
                              'scope': case['scope'], 'eligible': eligible, 'failures': problems,
                              'paired_current_upstream': ratios, 'median_current_upstream': value,
                              'margin_passed': value <= MAX_RATIO if value is not None else False})
    return {'passed': all(c['eligible'] and c['margin_passed'] for c in summaries),
            'conditions': summaries, 'manifest_sha256': fingerprint(manifest),
            'formal_population_sha256': fingerprint(rows),
            'scope': 'normalized-evidence protocol checks only; caller must retain/verify raw execution, resource and every-invocation correctness reports'}
