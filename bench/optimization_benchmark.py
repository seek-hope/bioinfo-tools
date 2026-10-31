#!/usr/bin/env python3
"""Blocked before/after/upstream performance check; preserves README oracles."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import itertools
import json
import os
from pathlib import Path
import random
import re
import shutil
import statistics
import sys

import readme_benchmark as reference
from readme_inputs import digest

ROOT = Path(__file__).resolve().parents[1]
ARMS = ('before', 'after', 'upstream')
NATIVE_NAMES = {'fx': 'rust-fx', 'bam': 'rust-bam', 'fastqc': 'rust-fastqc'}
FASTQ_CASES = ('fx_stats', 'fx_fq2fa', 'fq_lint', 'fastqc')


def plan(manifests: list[dict], pilot: bool) -> list[tuple[dict, str]]:
    datasets = [dataset for manifest in manifests for dataset in manifest['datasets']]
    by_id = {dataset['id']: dataset for dataset in datasets}
    if len(by_id) != len(datasets):
        raise ValueError('duplicate dataset ID')
    selected = [('synthetic-100000', FASTQ_CASES),
                ('synthetic-bam-100000', ('bam_flagstat', 'bam_sort'))] if pilot else [
        ('real-1000000', FASTQ_CASES), ('synthetic-1000000', FASTQ_CASES),
        ('synthetic-bam-1000000', ('bam_flagstat', 'bam_sort')),
        ('synthetic-10000000', ('fx_fq2fa', 'fq_lint')),
        ('synthetic-bam-10000000', ('bam_flagstat',))]
    result = []
    for name, cases in selected:
        dataset = by_id[name]
        for case in cases:
            if case not in reference.dataset_cases(dataset):
                raise ValueError(f'inapplicable case {name}/{case}')
            result.append((dataset, case))
    return result


def native_equal(case: str, outputs: dict, directories: dict) -> dict:
    """Additional before/after requirement, independent of upstream differences."""
    if case == 'fastqc':
        hashes = {arm: {str(path.relative_to(directories[arm] / 'out')): digest(path)
                        for path in sorted((directories[arm] / 'out').rglob('*')) if path.is_file()}
                  for arm in ('before', 'after')}
        rule = 'all native report, summary, HTML and ZIP bytes'
    elif case == 'fq_lint':
        # Ignore only the wall-clock timestamp; retain all levels and messages.
        timestamp = rb'\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?'
        hashes = {arm: hashlib.sha256(
            re.sub(timestamp, b'<timestamp>', outputs[arm].read_bytes())).hexdigest()
                  for arm in ('before', 'after')}
        rule = 'native lint messages, with wall-clock timestamps removed'
    else:
        hashes = {arm: reference.hash_decoded(outputs[arm]) for arm in ('before', 'after')}
        rule = 'native output bytes (including BAM encoding for sort)'
    return {'passed': hashes['before'] == hashes['after'], 'rule': rule, 'observations': hashes}


def complete(report: dict) -> bool:
    expected = report['planned_conditions']
    observed = [{'dataset': row['dataset'], 'case': row['case']} for row in report['results']]
    repeats = report['parameters']['repeats']
    blocks = list(range(-1, repeats))
    return expected == observed and all(
        row['passed'] and [check['block'] for check in row['checks']] == blocks
        and all(check['passed'] for check in row['checks'])
        and len(row['warmups']) == 1 and set(row['warmups'][0]) == set(ARMS)
        and all(sample['exit_code'] == 0 for sample in row['warmups'][0].values())
        and all(len(row['samples'][arm]) == repeats
                and [sample['block'] for sample in row['samples'][arm]] == list(range(repeats))
                and all(sample['exit_code'] == 0 for sample in row['samples'][arm]) for arm in ARMS)
        for row in report['results'])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, nargs='+', required=True)
    parser.add_argument('--baseline-dir', type=Path, required=True)
    parser.add_argument('--native-dir', type=Path, required=True)
    parser.add_argument('--outdir', type=Path, required=True)
    parser.add_argument('--oracle-dir', type=Path, default=ROOT / 'tests/ci/env/bin')
    parser.add_argument('--toolchain-cache', type=Path, default=Path('/tmp/bioinfo-ci-toolchain'))
    parser.add_argument('--cpu', type=int, default=4)
    parser.add_argument('--pilot', action='store_true')
    parser.add_argument('--streaming-only', action='store_true',
                        help='retry the three 10M conditions with their original block orders')
    args = parser.parse_args()
    if args.pilot and args.streaming_only:
        parser.error('pilot and streaming-only are separate populations')
    if args.outdir.exists():
        parser.error('refusing to overwrite output directory')
    if args.cpu not in os.sched_getaffinity(0):
        parser.error('CPU is unavailable')
    cache = args.toolchain_cache.resolve()
    if not (cache / '.ready').is_file():
        parser.error('pre-extracted pinned toolchain required')
    manifests = [json.loads(path.read_text()) for path in args.inputs]
    all_conditions = plan(manifests, args.pilot)
    conditions = all_conditions[-3:] if args.streaming_only else all_conditions
    for manifest in manifests:
        reference.verify_inputs(manifest)
    root = args.outdir.resolve()
    root.mkdir(parents=True)
    probes = root / 'probes'
    probes.mkdir()
    (probes / 'bioinfo-ci-toolchain').symlink_to(cache, target_is_directory=True)
    os.environ['BIOINFO_TOOLCHAIN_CACHE'] = str(cache)
    env = dict(os.environ, TMPDIR=str(probes), LC_ALL='C', PYTHONHASHSEED='0',
               OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1')
    native = {arm: {key: (directory / name).resolve() for key, name in NATIVE_NAMES.items()}
              for arm, directory in [('before', args.baseline_dir), ('after', args.native_dir)]}
    tools = {name: (args.oracle_dir / name).resolve() for name in ('seqkit', 'fq', 'samtools', 'fastqc')}
    versions = {}
    for name, exe in {**tools, **{f'{arm}_{key}': value for arm, entries in native.items() for key, value in entries.items()}}.items():
        command = [str(exe), 'version' if name == 'seqkit' else '--version']
        result = reference.checked(command, env=env, text=True)
        banner = (result.stdout or result.stderr).strip().splitlines()[0]
        if name in tools and banner != reference.PINNED[name] and not (name == 'fq' and banner.startswith(reference.PINNED[name] + ' ')):
            raise ValueError(f'wrong oracle version: {name}: {banner}')
        versions[name] = {'banner': banner, 'executable': str(exe), 'sha256': digest(exe)}
    repeats = 1 if args.pilot else 5
    report = {'schema': 1, 'kind': 'optimization-pilot' if args.pilot else 'optimization-benchmark',
              'started_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
              'source': reference.legacy.git_state(), 'hardware': reference.legacy.hardware(),
              'topology': reference.checked(['lscpu', '-e=CPU,CORE,SOCKET,MAXMHZ,ONLINE'], text=True).stdout,
              'versions': versions, 'inputs': manifests,
              'input_manifest_sha256': {str(path): digest(path) for path in args.inputs},
              'source_sha256': {str(path.relative_to(ROOT)): digest(path) for path in [ROOT/'Cargo.lock', ROOT/'Cargo.toml', *sorted(ROOT.glob('rust-*/src/**/*.rs')), *sorted(ROOT.glob('bioinfo-*/src/**/*.rs'))]},
              'script_sha256': {name: digest(ROOT/'bench'/name) for name in ['optimization_benchmark.py', 'readme_benchmark.py', 'readme_inputs.py', 'run.py', 'upstream.py']},
              'parameters': vars(args) | {'repeats': repeats, 'warmups': 1, 'cooldown': 2, 'seed': 20261101},
              'planned_conditions': [{'dataset': dataset['id'], 'case': case} for dataset, case in conditions],
              'results': [], 'passed': False}
    report_path = root / 'report.json'
    def save():
        temporary = root / 'report.partial'
        temporary.write_text(json.dumps(report, indent=2, default=str) + '\n')
        temporary.replace(report_path)
    save()
    rng = random.Random(20261101)
    block_orders = {}
    for dataset, case in all_conditions:
        orders = list(itertools.permutations(ARMS))
        rng.shuffle(orders)
        block_orders[(dataset['id'], case)] = orders
    for dataset, case in conditions:
        # Three complete FASTA outputs coexist until all comparisons finish.
        # The extra FASTQ punctuation makes bytes-bases an upper bound for
        # these verified, uncompressed four-line inputs. Lint creates one
        # invalid-tail copy at a time before timing.
        required = 512 * 1024 * 1024
        if case == 'fx_fq2fa':
            required += 3 * (dataset['bytes'] - dataset['bases'])
        elif case == 'fq_lint':
            required += dataset['bytes']
        free = shutil.disk_usage(root).free
        if free < required:
            raise OSError(f'insufficient output space: need {required} bytes, available {free}')
        condition = root / dataset['id'] / case
        condition.mkdir(parents=True)
        row = {'dataset': dataset['id'], 'records': dataset['records'], 'case': case,
               'samples': {arm: [] for arm in ARMS}, 'warmups': [], 'checks': [], 'passed': False}
        report['results'].append(row)
        save()
        try:
            negatives = {}
            if case == 'fq_lint':
                for arm in ('before', 'after'):
                    work = condition / f'{arm}-negative'
                    work.mkdir()
                    negatives[arm] = reference.lint_negative(dataset, tools, native[arm], work, str(args.cpu), env)
            orders = block_orders[(dataset['id'], case)]
            # One warm-up + five measurements use all six permutations. For
            # five measured blocks each arm occupies each slot one or two times.
            for block in range(-1, repeats):
                order = orders[block + 1]
                outputs, observations, directories = {}, {}, {}
                for arm in order:
                    directory = condition / f'{block}-{arm}'
                    directory.mkdir()
                    side = 'upstream' if arm == 'upstream' else 'rust'
                    command, output, compression, paths = reference.commands(
                        case, dataset, side, directory, native.get(arm, native['after']), tools, str(args.cpu), str(args.cpu))
                    observation = reference.run_one(command, directory, paths, env, 2, 300, args.cpu)
                    observation.update(block=block, order_in_block=order.index(arm),
                                       payload_bytes=output.stat().st_size if output.is_file() else None)
                    outputs[arm], observations[arm], directories[arm] = output, observation, directory
                    row['compression'] = compression
                    if block >= 0:
                        row['samples'][arm].append(observation)
                    else:
                        # Persist even an interrupted or failed warm-up.
                        row['warmups'] = [observations]
                    save()
                if any(value['exit_code'] != 0 for value in observations.values()):
                    check = {'passed': False, 'reason': 'nonzero exit', 'observations': observations}
                else:
                    checks = {arm: reference.verify(case, dataset, {'rust': outputs[arm], 'upstream': outputs['upstream']}, tools, negatives.get(arm))
                              for arm in ('before', 'after')}
                    checks['native_preservation'] = native_equal(case, outputs, directories)
                    check = {'passed': all(value['passed'] for value in checks.values()), 'checks': checks}
                check['block'] = block
                row['checks'].append(check)
                save()
                print(dataset['id'], case, 'block', block, 'order', order, 'passed', check['passed'],
                      {arm: round(value['elapsed_s'], 4) for arm, value in observations.items()}, flush=True)
                if not check['passed']:
                    break
                for directory in directories.values():
                    shutil.rmtree(directory / 'out')
            row['passed'] = len(row['checks']) == repeats + 1 and all(check['passed'] for check in row['checks'])
            if row['passed']:
                row['summary'] = {arm: reference.summarize(row['samples'][arm], dataset['records']) for arm in ARMS}
                ratios = [before['elapsed_s'] / after['elapsed_s'] for before, after in zip(row['samples']['before'], row['samples']['after'])]
                row['paired_before_after_ratio'] = {'samples': ratios, 'median': statistics.median(ratios), 'min': min(ratios), 'max': max(ratios)}
        except Exception as error:
            row['checks'].append({'passed': False, 'reason': repr(error)})
            save()
            raise
        save()
    report['passed'] = complete(report)
    report['completed_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
    save()
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
