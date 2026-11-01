#!/usr/bin/env python3
"""Shared, explicit workload/CPU contracts for the enlarged comparison."""
from __future__ import annotations
import hashlib
import itertools
import json
import math
from pathlib import Path
import random

import readme_benchmark as reference

ARMS = ('current', 'upstream')
AFFINITIES = {1: '4', 2: '4,6', 4: '0,2,4,6'}
SEED = 20261102
# CPU2 adds a scaling point where command-level parallelism is exposed, plus
# stats (the remaining slow FASTX calculation). Serial-only families retain1/4
# to measure actual CPU utilization under both budgets, without claiming threads.
WORKLOADS = [
    ('real-cutadapt', 'real-1000000', 'cutadapt_plain', [1, 2, 4]),
    ('synthetic-cutadapt', 'synthetic-1000000', 'cutadapt_plain', [1, 2, 4]),
    ('real-cutadapt-gzip', 'real-1000000', 'cutadapt_gzip', [1, 2, 4]),
    ('real-stats', 'real-1000000', 'fx_stats', [1, 2, 4]),
    ('synthetic-stats', 'synthetic-1000000', 'fx_stats', [1, 2, 4]),
    ('synthetic-fq2fa', 'synthetic-1000000', 'fx_fq2fa', [1, 4]),
    ('synthetic-lint', 'synthetic-1000000', 'fq_lint', [1, 4]),
    ('bam-flagstat', 'synthetic-bam-1000000', 'bam_flagstat', [1, 4]),
    ('bam-sort', 'synthetic-bam-1000000', 'bam_sort', [1, 2, 4]),
    ('umi-dedup', 'synthetic-bam-1000000', 'umi_dedup', [1, 4]),
    ('real-fastqc', 'real-1000000', 'fastqc', [1, 4]),
]
NATIVE_NAMES = {'cutadapt': 'rust-cutadapt', 'fx': 'rust-fx', 'bam': 'rust-bam',
                'umi': 'rust-umitools', 'fastqc': 'rust-fastqc'}


def initial_plan() -> dict:
    return {'schema_version': 1, 'seed': SEED, 'repeats': 3, 'warmups': 1,
            'target_fastest_seconds': 15, 'minimum_desired_seconds': 10,
            'affinities': {str(k): v for k, v in AFFINITIES.items()},
            'workloads': [{'id': name, 'source_id': source, 'case': case,
                           'cpu_budgets': cpus, 'multiplicity': 1}
                          for name, source, case, cpus in WORKLOADS]}


def validate_plan(plan: dict) -> None:
    if plan.get('schema_version') != 1 or plan.get('repeats') != 3 or plan.get('warmups') != 1:
        raise ValueError('expected schema1, three measured blocks and one warmup')
    if plan.get('affinities') != {str(k): v for k, v in AFFINITIES.items()}:
        raise ValueError('CPU affinity contract changed')
    if not isinstance(plan.get('seed'), int) or isinstance(plan['seed'], bool):
        raise ValueError('integer seed required')
    names = set()
    for item in plan['workloads']:
        if set(item) != {'id', 'source_id', 'case', 'cpu_budgets', 'multiplicity'}:
            raise ValueError('unknown workload fields')
        name = item['id']
        if not name or Path(name).name != name or name in names:
            raise ValueError('invalid or duplicate workload ID')
        names.add(name)
        if item['case'] not in reference.CASES or item['case'] == 'cutadapt_4cpu':
            raise ValueError('unknown case; CPU budget must be explicit')
        if not isinstance(item['multiplicity'], int) or isinstance(item['multiplicity'], bool) or item['multiplicity'] < 1:
            raise ValueError('positive integer multiplicity required')
        cpus = item['cpu_budgets']
        if not cpus or len(set(cpus)) != len(cpus) or any(type(c) is not int or c not in AFFINITIES for c in cpus):
            raise ValueError('invalid CPU budgets')
    if not names:
        raise ValueError('empty workload population')
    for key in ('target_fastest_seconds', 'minimum_desired_seconds'):
        if not finite(plan.get(key), positive=True):
            raise ValueError('positive calibration duration required')
    if plan['minimum_desired_seconds'] > plan['target_fastest_seconds']:
        raise ValueError('calibration minimum exceeds target')


def conditions(plan: dict) -> list[dict]:
    validate_plan(plan)
    rng = random.Random(plan['seed'])
    result = []
    for workload in plan['workloads']:
        for cores in workload['cpu_budgets']:
            orders = [list(ARMS), list(reversed(ARMS))] * 2
            rng.shuffle(orders)
            result.append({'id': f"{workload['id']}-c{cores}", 'workload': workload['id'],
                           'case': workload['case'], 'cores': cores,
                           'cpu_affinity': AFFINITIES[cores], 'orders': orders})
    return result


def command(case: str, dataset: dict, arm: str, cores: int, directory: Path,
            native: dict, upstream: dict) -> tuple:
    if arm not in ARMS or cores not in AFFINITIES:
        raise ValueError('explicit arm and CPU budget required')
    side = 'rust' if arm == 'current' else 'upstream'
    affinity = AFFINITIES[cores]
    cmd, output, compression, inputs = reference.commands(
        case, dataset, side, directory, native, upstream, affinity, affinity)
    if case.startswith('cutadapt_'):
        cmd[cmd.index('-j') + 1] = str(cores)
    elif case in ('fx_stats', 'fx_fq2fa'):
        cmd[cmd.index('-j') + 1] = str(cores)
    elif case == 'bam_sort':
        if arm == 'current':
            cmd[cmd.index('--max-memory') + 1] = '512MiB'
            cmd[cmd.index('--threads') + 1] = str(cores)
        else:
            cmd[cmd.index('-m') + 1] = f'{512 // cores}M'
            cmd[cmd.index('-@') + 1] = str(cores - 1)
    elif case == 'bam_flagstat' and arm == 'upstream':
        cmd[-1:-1] = ['-@', str(cores - 1)]
    elif case == 'umi_dedup' and arm == 'current':
        cmd[cmd.index('--threads') + 1] = str(cores)
    return cmd, output, compression, inputs


def finite(value, positive=False) -> bool:
    return type(value) in (int, float) and math.isfinite(value) and (value > 0 if positive else value >= 0)


def validate_check(case: str, check: dict) -> None:
    def equal_hashes(values):
        if len(values) != 2 or any(not isinstance(v, str) or len(v) != 64 or any(c not in '0123456789abcdef' for c in v) for v in values):
            raise ValueError('missing or invalid output hashes')
        if values[0] != values[1]:
            raise ValueError('output hashes differ despite passed marker')
    if case.startswith('cutadapt') or case in ('fx_stats', 'fx_fq2fa', 'bam_flagstat'):
        if set(check.get('sha256', {})) != {'rust', 'upstream'}:
            raise ValueError('missing output comparison')
        equal_hashes(list(check['sha256'].values()))
    elif case in ('bam_sort', 'umi_dedup'):
        observations = check.get('observations', {})
        arms = {'current', 'upstream'} if case == 'bam_sort' else {'rust', 'upstream'}
        if set(observations) != arms or any(o['records'] != check['expected_records'] for o in observations.values()):
            raise ValueError('BAM population mismatch')
        equal_hashes([o['normalized_sha256'] for o in observations.values()])
        if case == 'umi_dedup' and not all(o.get('independent_representatives') is True for o in observations.values()):
            raise ValueError('independent UMI representatives differ')
    elif case == 'fq_lint':
        negative = check.get('negative_control') or {}
        if negative.get('passed') is not True or set(negative.get('observations', {})) != {'rust', 'upstream'} or any(o.get('exit_code') != 1 for o in negative['observations'].values()):
            raise ValueError('missing or failed lint invalid-tail control')
    elif case == 'fastqc':
        if check.get('equal_work') is not False or check.get('speed_ratio_eligible') is not False:
            raise ValueError('QC must remain descriptive')
        if set(check.get('observations', {})) != {'rust', 'upstream'} or not all(o.get('independent_count_bases_and_length') is True for o in check['observations'].values()):
            raise ValueError('QC independent counts failed')
        unequal = {name for name, equal in check['module_equal'].items() if not equal}
        if set(check['differences']) != unequal or check['full_module_equality'] != (not unequal):
            raise ValueError('QC difference evidence incomplete')
    else:
        raise ValueError('unknown validation case')


def validate_complete(report: dict, formal: bool = True) -> None:
    if report.get('kind') != ('scale-benchmark' if formal else 'scale-calibration'):
        raise ValueError('wrong experiment kind')
    blocks = list(range(-1, 3)) if formal else [-1]
    expected = conditions(report['plan'])
    if report.get('planned_conditions') != expected:
        raise ValueError('planned population/orders changed')
    if [row['condition'] for row in report['results']] != expected:
        raise ValueError('incomplete or reordered population')
    invocations = report['planned_invocations']
    checksum = hashlib.sha256(json.dumps(invocations, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    if report.get('invocations_sha256') != checksum:
        raise ValueError('frozen commands changed')
    expected_invocations = [(c['id'], b, a) for c in expected for b in blocks for a in ARMS]
    if [(i['condition'], i['block'], i['arm']) for i in invocations] != expected_invocations:
        raise ValueError('incomplete planned commands')
    lookup = {(i['condition'], i['block'], i['arm']): i for i in invocations}
    for row in report['results']:
        expected_keys = list(itertools.product(blocks, ARMS))
        observations = row['observations']
        if [(o['block'], o['arm']) for o in observations] != expected_keys:
            raise ValueError('missing, duplicated, or reordered observations')
        if [check['block'] for check in row['checks']] != blocks or not all(check.get('passed') is True for check in row['checks']):
            raise ValueError('missing or failed output validation')
        for check in row['checks']:
            validate_check(row['condition']['case'], check)
        for sample in observations:
            order = row['condition']['orders'][sample['block'] + 1]
            planned = lookup[(row['condition']['id'], sample['block'], sample['arm'])]
            if any(sample.get(key) != planned[key] for key in ('command', 'output', 'directory', 'cpu_affinity', 'order_in_block')):
                raise ValueError('actual command differs from frozen invocation')
            if sample['command'][:3] != ['taskset', '-c', row['condition']['cpu_affinity']]:
                raise ValueError('actual CPU affinity differs from plan')
            if sample['order_in_block'] != order.index(sample['arm']) or type(sample['exit_code']) is not int or sample['exit_code'] != 0:
                raise ValueError('execution/order failure')
            for key in ('elapsed_s', 'user_s', 'sys_s', 'max_rss_kb', 'fsync_s'):
                if not finite(sample.get(key), positive=key in ('elapsed_s', 'max_rss_kb')):
                    raise ValueError(f'invalid metric {key}')
    if report.get('passed') is not True or not report.get('completed_utc') or any(report.get(key) is not True for key in ('identities_unchanged', 'inputs_unchanged', 'runtime_payloads_unchanged')):
        raise ValueError('run incomplete or artifact identities changed')
