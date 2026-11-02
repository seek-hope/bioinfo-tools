#!/usr/bin/env python3
"""Large-input upstream/current measurements, with frozen commands and full validation."""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import threading
import time

import readme_benchmark as reference
from readme_inputs import digest
from scale_common import ARMS, AFFINITIES, NATIVE_NAMES, command, conditions, finite, native_signature, validate_check, validate_complete
from scale_validation import bam_summary
from scale_seal import source_hashes

ROOT = Path(__file__).resolve().parents[1]


def json_hash(value) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def atomic_json(path: Path, value) -> None:
    temporary = path.with_suffix('.partial')
    temporary.write_text(json.dumps(value, indent=2) + '\n')
    temporary.replace(path)


def frequency(cpus: str) -> dict:
    result = {}
    for cpu in cpus.split(','):
        path = Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq')
        result[cpu] = int(path.read_text()) if path.exists() else None
    return result


class TelemetrySampler:
    """One-second samples; the CLI timer excludes joining this observer."""
    def __init__(self, cpus):
        self.cpus = cpus
        self.samples = []
        self.error = None
        self.stop = threading.Event()
        self.worker = threading.Thread(target=self.collect, daemon=True)

    def collect(self):
        while not self.stop.wait(1):
            try:
                sample = reference.telemetry(int(self.cpus.split(',')[0]))
                sample['cpu_frequency_khz'] = frequency(self.cpus)
                self.samples.append(sample)
            except Exception as error:
                self.error = repr(error)
                return

    def __enter__(self):
        self.worker.start()
        return self

    def __exit__(self, *unused):
        self.stop.set()
        self.worker.join()


def flush_outputs(directory: Path) -> float:
    start = time.perf_counter()
    for path in sorted(directory.rglob('*')):
        if path.is_file() and not path.is_symlink():
            with path.open('rb') as handle:
                os.fsync(handle.fileno())
    return time.perf_counter() - start


def run_one(invocation: dict, env: dict, timeout: float) -> dict:
    directory = Path(invocation['directory'])
    temporary = directory / 'tmp'
    temporary.mkdir()
    reference.prepare_upstream_temp(temporary)
    start = time.perf_counter()
    reference.warm([Path(p) for p in invocation['inputs']])
    conditioning_s = time.perf_counter() - start
    time.sleep(2)
    cpus = invocation['cpu_affinity']
    before = reference.telemetry(int(cpus.split(',')[0]))
    before['cpu_frequency_khz'] = frequency(cpus)
    executed_command = list(invocation['command'])
    with reference.legacy.DiskSampler(temporary) as disk, TelemetrySampler(cpus) as telemetry, (directory/'stdout.txt').open('wb') as stdout, (directory/'stderr.txt').open('wb') as stderr:
        begin = time.perf_counter()
        process = subprocess.Popen(['/usr/bin/time', '-v', '-o', str(directory/'time.txt'), *executed_command],
                                   cwd=directory, env=dict(env, TMPDIR=str(temporary)),
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        timed_out = threading.Event()
        def stop():
            try:
                os.killpg(process.pid, signal.SIGKILL)
                timed_out.set()
            except ProcessLookupError:
                pass
        watchdog = threading.Timer(timeout, stop)
        watchdog.daemon = True
        watchdog.start()
        try:
            code = process.wait()
        except BaseException:
            stop()
            process.wait()
            raise
        finally:
            watchdog.cancel()
        elapsed = time.perf_counter() - begin
    after = reference.telemetry(int(cpus.split(',')[0]))
    after['cpu_frequency_khz'] = frequency(cpus)
    metrics = reference.legacy.parse_time(directory/'time.txt')
    labels = {'File system inputs': 'filesystem_inputs', 'File system outputs': 'filesystem_outputs',
              'Major (requiring I/O) page faults': 'major_faults',
              'Minor (reclaiming a frame) page faults': 'minor_faults',
              'Voluntary context switches': 'voluntary_context_switches',
              'Involuntary context switches': 'involuntary_context_switches'}
    for line in (directory/'time.txt').read_text().splitlines():
        key, separator, value = line.strip().rpartition(':')
        if separator and key in labels:
            metrics[labels[key]] = int(value.strip())
    metrics.update({key: invocation[key] for key in ('condition', 'block', 'arm', 'order_in_block', 'command', 'directory', 'output', 'cpu_affinity')})
    metrics['command'] = executed_command
    metrics['input_bytes'] = invocation['input_bytes']
    output = Path(invocation['output'])
    metrics.update(elapsed_s=elapsed, exit_code=124 if timed_out.is_set() else code,
                   input_conditioning_s=conditioning_s, before=before, after=after,
                   telemetry_samples=telemetry.samples, telemetry_interval_s=1, telemetry_error=telemetry.error,
                   temp_peak_bytes_sampled=disk.peak,
                   payload_bytes=output.stat().st_size if output.is_file() else None,
                   output_bytes=sum(p.stat().st_size for p in (directory/'out').rglob('*') if p.is_file()),
                   stderr=(directory/'stderr.txt').read_text(errors='replace')[:4000])
    metrics['fsync_s'] = flush_outputs(directory)
    if metrics['user_s'] is not None and metrics['sys_s'] is not None:
        metrics['cpu_seconds_per_wall_second'] = (metrics['user_s'] + metrics['sys_s']) / elapsed
    return metrics


def verify(case: str, dataset: dict, outputs: dict, upstream: dict, helper: Path, negative) -> dict:
    if case == 'bam_sort':
        summaries = {arm: bam_summary(path, upstream['samtools'], helper) | {'encoded_sha256': digest(path)} for arm, path in outputs.items()}
        passed = all(s['records'] == dataset['records'] for s in summaries.values())
        passed &= summaries['current']['normalized_sha256'] == summaries['upstream']['normalized_sha256']
        return {'passed': passed, 'expected_records': dataset['records'], 'observations': summaries,
                'rule': 'full SAM fields, @PG omitted, coordinate ties canonicalized'}
    return reference.verify(case, dataset, {'rust': outputs['current'], 'upstream': outputs['upstream']}, upstream, negative)


def reserve_for_pair(dataset: dict, case: str, directory: Path) -> int:
    if case in ('cutadapt_plain', 'cutadapt_gzip', 'fx_fq2fa'):
        estimate = (dataset['bytes'] * 31 + 9) // 10
    elif case == 'fq_lint':
        estimate = dataset['bytes'] + 1024 ** 2
    elif case in ('bam_sort', 'umi_dedup'):
        estimate = 8 * dataset['decoded_bytes_upper_bound']
    else:
        estimate = 64 * 1024 ** 2
    estimate += 10 * 1024 ** 3
    if shutil.disk_usage(directory).free < estimate:
        raise OSError(f'insufficient benchmark output/run space: need {estimate} bytes')
    return estimate


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--native-dir', type=Path, required=True)
    parser.add_argument('--outdir', type=Path, required=True)
    parser.add_argument('--canonicalizer', type=Path, required=True)
    parser.add_argument('--oracle-dir', type=Path, required=True)
    parser.add_argument('--toolchain-cache', type=Path, required=True)
    parser.add_argument('--cutadapt', type=Path, required=True)
    parser.add_argument('--calibration', action='store_true')
    parser.add_argument('--timeout', type=float, default=1800)
    args = parser.parse_args()
    if not finite(args.timeout, positive=True):
        parser.error('--timeout must be finite and positive')
    plan = json.loads(args.plan.read_text())
    planned = conditions(plan)
    if not args.calibration and plan.get('frozen_for_formal_run') is not True:
        raise ValueError('formal measurements require a frozen calibrated plan')
    manifest = json.loads(args.inputs.read_text())
    stable_keys = ('schema_version', 'seed', 'repeats', 'warmups', 'affinities', 'workloads',
                   'target_fastest_seconds', 'minimum_desired_seconds')
    if manifest.get('passed') is not True or any(manifest.get('plan', {}).get(k) != plan.get(k) for k in stable_keys):
        raise ValueError('prepared input population does not match the plan')
    mapping = {w['id']: w for w in manifest['workloads']}
    datasets = {d['id']: d for d in manifest['datasets']}
    if len(mapping) != len(manifest['workloads']) or len(datasets) != len(manifest['datasets']):
        raise ValueError('duplicate workload/dataset identity')
    if set(mapping) != {w['id'] for w in plan['workloads']}:
        raise ValueError('input workload population differs from plan')
    for workload in plan['workloads']:
        actual = mapping[workload['id']]
        if {k: actual[k] for k in workload} != workload:
            raise ValueError('input preparation workload changed')
        dataset = datasets[actual['dataset']]
        if dataset['source_population'] != workload['source_id'] or dataset['multiplicity'] != workload['multiplicity']:
            raise ValueError('input population/multiplicity changed')
        if type(dataset['records']) is not int or dataset['records'] <= 0 or dataset['records'] != dataset['source_metadata']['records'] * dataset['multiplicity']:
            raise ValueError('input record count violates repetition contract')
    cpus = {int(c) for affinity in AFFINITIES.values() for c in affinity.split(',')}
    if not cpus <= os.sched_getaffinity(0):
        raise ValueError('requested CPU affinity is unavailable')
    core_ids = [(Path(f'/sys/devices/system/cpu/cpu{c}/topology/physical_package_id').read_text().strip(),
                 Path(f'/sys/devices/system/cpu/cpu{c}/topology/core_id').read_text().strip()) for c in sorted(cpus)]
    if len(set(core_ids)) != len(core_ids):
        raise ValueError('CPU budgets include sibling threads of one physical core')
    if not (args.toolchain_cache / '.ready').is_file():
        raise ValueError('pre-extracted pinned toolchain required')
    reference.verify_inputs(manifest)
    root = args.outdir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    cache = args.toolchain_cache.resolve()
    os.environ['BIOINFO_TOOLCHAIN_CACHE'] = str(cache)
    controlled = {'LC_ALL': 'C', 'PYTHONHASHSEED': '0', 'PYTHONNOUSERSITE': '1',
                  'OMP_NUM_THREADS': '1', 'OPENBLAS_NUM_THREADS': '1', 'MKL_NUM_THREADS': '1',
                  'RUST_CUTADAPT_COMPRESSION_LEVEL': '6', 'GOGC': '100'}
    removed = ['RUST_CUTADAPT_CHUNK', 'PYTHONPATH', 'GOMEMLIMIT', 'GODEBUG',
               'JAVA_TOOL_OPTIONS', '_JAVA_OPTIONS', 'JDK_JAVA_OPTIONS']
    os.environ.update(controlled)
    for key in removed:
        os.environ.pop(key, None)
    os.environ['TMPDIR'] = str(root)
    env = dict(os.environ)
    (root/'bioinfo-ci-toolchain').symlink_to(cache, target_is_directory=True)
    native = {key: (args.native_dir/name).resolve() for key, name in NATIVE_NAMES.items()}
    upstream = {key: (args.cutadapt if key == 'cutadapt' else args.oracle_dir/key).resolve() for key in reference.PINNED}
    helper = args.canonicalizer.resolve()
    build_path = args.native_dir/'build-provenance.json'
    build = json.loads(build_path.read_text())
    if build.get('passed') is not True or build['source_sha256'] != source_hashes():
        raise ValueError('native source differs from sealed build')
    for key, name in NATIVE_NAMES.items():
        if digest(native[key]) != build['executables'][name]['sha256']:
            raise ValueError('native executable differs from sealed build')
    if digest(helper) != build['canonicalizer']['sha256'] or digest(ROOT/'bench/canonical_sam.rs') != build['canonicalizer']['source_sha256']:
        raise ValueError('canonicalizer differs from sealed build')
    versions = {}
    for name, executable in {**upstream, **{f'current_{k}': v for k, v in native.items()}}.items():
        cmd = [str(executable), 'version' if name == 'seqkit' else '--version']
        probe = reference.checked(cmd, text=True, env=env)
        banner = (probe.stdout or probe.stderr).strip().splitlines()[0]
        if name in reference.PINNED and not (banner == reference.PINNED[name] or name == 'fq' and banner.startswith(reference.PINNED[name] + ' ')):
            raise ValueError(f'wrong pinned version: {name}: {banner}')
        versions[name] = {'version': banner, 'command': cmd, 'sha256': digest(executable)}
    support_scripts = [cache/'umi/bin/umi_tools']
    support_scripts += sorted({p.parent/'_toolchain' for p in upstream.values() if (p.parent/'_toolchain').is_file()})
    versions['support_scripts'] = {str(p): digest(p) for p in support_scripts}
    timing_tools = {'benchmark_python': Path(sys.executable).resolve(), 'gnu_time': Path('/usr/bin/time'),
                    'taskset': Path(shutil.which('taskset') or '/missing/taskset').resolve()}
    for name, executable in timing_tools.items():
        probe = reference.checked([str(executable), '--version'], text=True, env=env)
        versions[name] = {'version': (probe.stdout or probe.stderr).strip(), 'sha256': digest(executable)}
    runtime_payloads = reference.runtime_provenance(cache, upstream['cutadapt'], env)
    source_paths = [ROOT/'Cargo.toml', ROOT/'Cargo.lock', *sorted(ROOT.glob('*/Cargo.toml')), *sorted(ROOT.glob('*/src/**/*.rs'))]
    script_paths = [Path(__file__).resolve(), ROOT/'bench/scale_common.py', ROOT/'bench/scale_validation.py', ROOT/'bench/canonical_sam.rs',
                    ROOT/'bench/scale_seal.py', ROOT/'bench/readme_benchmark.py', ROOT/'bench/readme_inputs.py', ROOT/'bench/run.py', ROOT/'bench/upstream.py']
    if not args.calibration:
        calibration_path = Path(plan['calibration_report'])
        if digest(calibration_path) != plan['calibration_report_sha256']:
            raise ValueError('calibration report changed')
        calibration = json.loads(calibration_path.read_text())
        validate_complete(calibration, formal=False)
        if any(calibration['plan'].get(k) != plan.get(k) for k in stable_keys) or calibration['inputs'] != manifest:
            raise ValueError('formal population differs from calibration')
        if calibration['versions'] != versions or calibration['runtime_payloads'] != runtime_payloads or any(calibration['identities'].get(str(p)) != digest(p) for p in script_paths):
            raise ValueError('executables or measurement scripts differ from calibration')
        if min(o['elapsed_s'] for r in calibration['results'] for o in r['observations']) < plan['minimum_desired_seconds']:
            raise ValueError('calibration duration is too short')
    identity_paths = [*source_paths, *script_paths, *support_scripts, *native.values(), *upstream.values(), *timing_tools.values(), helper, args.plan.resolve(), args.inputs.resolve(), build_path.resolve()]
    identities = {str(p): digest(p) for p in identity_paths}
    storage = json.loads(reference.checked(['findmnt', '-T', str(root), '-J', '-o', 'TARGET,SOURCE,FSTYPE,OPTIONS'], text=True).stdout)
    if storage['filesystems'][0]['fstype'] in ('tmpfs', 'ramfs'):
        raise ValueError('scale measurements require disk storage')
    for dataset in datasets.values():
        input_path = Path(dataset.get('path') or dataset['unsorted'])
        if input_path.stat().st_dev != root.stat().st_dev:
            raise ValueError('prepared inputs and outputs must use the same recorded disk filesystem')
    executed = root/'executed'
    executed.mkdir()
    for path in script_paths:
        shutil.copy2(path, executed/path.name)
    for index, path in enumerate(support_scripts):
        shutil.copy2(path, executed/f'runtime-{index}-{path.name}')
    blocks = [-1] if args.calibration else list(range(-1, 3))
    invocations = []
    for condition in planned:
        dataset = datasets[mapping[condition['workload']]['dataset']]
        for block in blocks:
            for arm in ARMS:
                directory = root/condition['id']/f'{block}-{arm}'
                cmd, output, compression, paths = command(condition['case'], dataset, arm, condition['cores'], directory, native, upstream)
                if any(p.stat().st_dev != root.stat().st_dev for p in paths):
                    raise ValueError('a measured input is outside the recorded disk filesystem')
                invocations.append({'condition': condition['id'], 'block': block, 'arm': arm,
                                    'order_in_block': condition['orders'][block + 1].index(arm),
                                    'cpu_affinity': condition['cpu_affinity'], 'directory': str(directory),
                                    'command': cmd, 'output': str(output), 'compression': compression,
                                    'inputs': list(map(str, paths)), 'input_bytes': sum(p.stat().st_size for p in paths)})
    report = {'schema_version': 1, 'kind': 'scale-calibration' if args.calibration else 'scale-benchmark',
              'started_utc': dt.datetime.now(dt.timezone.utc).isoformat(), 'plan': plan,
              'planned_conditions': planned, 'planned_invocations': invocations, 'invocations_sha256': json_hash(invocations),
              'inputs': manifest, 'versions': versions, 'identities': identities, 'build_provenance': build,
              'runtime_payloads': runtime_payloads,
              'source': reference.legacy.git_state(), 'hardware': reference.legacy.hardware(),
              'topology': reference.checked(['lscpu','-e=CPU,CORE,SOCKET,MAXMHZ,ONLINE'],text=True).stdout,
              'storage': storage,
              'parameters': {'timeout_s': args.timeout, 'cooldown_s': 2,
                             'controlled_environment': controlled, 'removed_environment_keys': removed,
                             'cache_policy': 'sequential read conditioning, no residency guarantee',
                             'fsync': 'all regular output/log files after CLI timing; duration separate'},
              'results': [], 'passed': False}
    output_report = root/'report.json'
    reserve = root/'.evidence-reserve'
    with reserve.open('xb') as space:
        os.posix_fallocate(space.fileno(), 0, 64 * 1024 ** 2)
        os.fsync(space.fileno())
    save = lambda: atomic_json(output_report, report)
    save()
    lookup = {(i['condition'], i['block'], i['arm']): i for i in invocations}
    signatures, anchors = {}, {}
    last_conditions = {c['workload']: c['id'] for c in planned}
    try:
        for condition in planned:
            dataset = datasets[mapping[condition['workload']]['dataset']]
            row = {'condition': condition, 'dataset': dataset['id'], 'records': dataset['records'],
                   'input_bytes': lookup[(condition['id'], -1, 'current')]['input_bytes'], 'observations': [], 'checks': []}
            report['results'].append(row)
            row['required_free_space_bytes'] = reserve_for_pair(dataset, condition['case'], root)
            negative = reference.lint_negative(dataset, upstream, native, root/condition['id'], condition['cpu_affinity'], env) if condition['case'] == 'fq_lint' else None
            for block in blocks:
                observed = {}
                outputs = {}
                for arm in condition['orders'][block + 1]:
                    invocation = lookup[(condition['id'], block, arm)]
                    observation = run_one(invocation, env, args.timeout)
                    observed[arm] = observation
                    outputs[arm] = Path(invocation['output'])
                    row['observations'].append(observation)
                    save()
                    if observation['telemetry_error']:
                        raise ValueError(f"telemetry observer failed: {observation['telemetry_error']}")
                row['observations'].sort(key=lambda o: (o['block'], ARMS.index(o['arm'])))
                if any(o['exit_code'] != 0 for o in observed.values()):
                    check = {'passed': False, 'reason': 'nonzero execution'}
                else:
                    check = verify(condition['case'], dataset, outputs, upstream, helper, negative)
                check['block'] = block
                row['checks'].append(check)
                print(condition['id'], f'block={block}', {a: round(observed[a]['elapsed_s'], 3) for a in ARMS}, f"passed={check['passed']}", flush=True)
                save()
                if not check['passed']:
                    raise ValueError(f"output validation failed: {condition['id']}/{block}")
                validate_check(condition['case'], check)
                workload = condition['workload']
                signature = native_signature(condition['case'], check)
                if workload in signatures and signatures[workload] != signature:
                    raise ValueError(f'native output differs across blocks or CPU budgets: {workload}')
                signatures[workload] = signature
                anchors.setdefault(workload, Path(observed['current']['directory'])/'out')
                for arm in ARMS:
                    payload = Path(observed[arm]['directory'])/'out'
                    # Keep the first current payload until every CPU budget for
                    # this workload matches it; failures retain both payloads.
                    if payload != anchors[workload]:
                        shutil.rmtree(payload)
            if condition['id'] == last_conditions[condition['workload']]:
                shutil.rmtree(anchors.pop(condition['workload']))
            save()
        reference.verify_inputs(manifest)
        report['inputs_unchanged'] = True
        report['runtime_payloads_unchanged'] = reference.runtime_provenance(cache, upstream['cutadapt'], env) == report['runtime_payloads']
        report['identities_unchanged'] = all(digest(Path(p)) == checksum for p, checksum in identities.items())
        report['completed_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
        report['passed'] = report['identities_unchanged'] and report['runtime_payloads_unchanged']
        validate_complete(report, formal=not args.calibration)
    except BaseException as error:
        report['passed'] = False
        report['error'] = repr(error)
        raise
    finally:
        reserve.unlink(missing_ok=True)
        report.setdefault('completed_utc', dt.datetime.now(dt.timezone.utc).isoformat())
        save()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
