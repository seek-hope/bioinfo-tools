#!/usr/bin/env python3
"""Paired, correctness-gated README measurements. See README-PROTOCOL.md."""
from __future__ import annotations

import argparse
from collections import OrderedDict
import datetime as dt
import gzip
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time

import run as legacy
from readme_inputs import digest
from upstream import prepare_upstream_temp

ROOT = Path(__file__).resolve().parents[1]
PINNED = {"cutadapt": "4.6", "umi_tools": "UMI-tools version: 1.1.6", "samtools": "samtools 1.24",
          "fastqc": "FastQC v0.13.0", "seqkit": "seqkit v2.14.0", "fq": "fq v0.12.0"}


CASES = {'cutadapt_plain', 'cutadapt_gzip', 'cutadapt_4cpu', 'fx_stats', 'fx_fq2fa',
         'fq_lint', 'bam_flagstat', 'bam_sort', 'umi_dedup', 'fastqc'}


def dataset_cases(dataset: dict) -> list[str]:
    if dataset['kind'] == 'bam':
        return ['bam_flagstat', 'bam_sort', 'umi_dedup']
    if dataset['kind'] != 'fastq':
        raise ValueError(f"unknown dataset kind: {dataset['kind']}")
    cases = ['cutadapt_plain', 'fx_stats', 'fx_fq2fa', 'fq_lint', 'fastqc']
    if dataset['records'] >= 1000000:
        cases += ['cutadapt_gzip', 'cutadapt_4cpu']
    return cases


def planned_conditions(manifest: dict, datasets: set[str], cases: set[str]) -> list[dict]:
    if cases - CASES:
        raise ValueError(f'unknown cases: {sorted(cases - CASES)}')
    ids = [d['id'] for d in manifest['datasets']]
    if len(ids) != len(set(ids)):
        raise ValueError('duplicate dataset IDs')
    if datasets - set(ids):
        raise ValueError('requested dataset is absent')
    plan = [{'dataset':d['id'], 'case':case} for d in manifest['datasets']
            if not datasets or d['id'] in datasets
            for case in dataset_cases(d) if not cases or case in cases]
    if not plan or cases - {item['case'] for item in plan}:
        raise ValueError('selected cases have no applicable dataset')
    return plan


def checked(command: list[str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(command, check=True, capture_output=True, **kwargs)


def tree_digest(path: Path) -> str:
    """Hash runtime payloads, excluding generated Python bytecode."""
    if path.is_file():
        return digest(path)
    h = hashlib.sha256()
    for item in sorted(path.rglob('*')):
        if item.is_file() and '__pycache__' not in item.parts and item.suffix != '.pyc':
            h.update(str(item.relative_to(path)).encode() + b'\0' + digest(item).encode() + b'\n')
    return h.hexdigest()


def runtime_provenance(cache: Path, cutadapt: Path, env: dict) -> dict:
    script = '''import importlib.util,importlib.metadata,json,sys,zlib
items={}
for name in sys.argv[1:]:
 spec=importlib.util.find_spec(name)
 if spec is None: continue
 try: version=importlib.metadata.version(name.replace('_','-'))
 except importlib.metadata.PackageNotFoundError: version=None
 items[name]={'version':version,'origin':spec.origin,'package':bool(spec.submodule_search_locations)}
print(json.dumps({'python':sys.version,'executable':sys.executable,'zlib_runtime':zlib.ZLIB_RUNTIME_VERSION,'modules':items}))'''
    result = {}
    for name, interpreter, modules in [('cutadapt', cutadapt.parent / 'python', ['cutadapt','dnaio','xopen','isal','zlib_ng']),
                                       ('umi_tools', cache / 'umi/bin/python3', ['umi_tools','pysam','numpy','scipy','pandas'])]:
        data = json.loads(checked([str(interpreter), '-c', script, *modules], env=env, text=True, timeout=30).stdout)
        data['interpreter_sha256'] = digest(interpreter.resolve())
        for module in data['modules'].values():
            path = Path(module['origin'])
            module['payload_sha256'] = tree_digest(path.parent if module['package'] else path)
        result[name] = data
    result['fastqc_payload_sha256'] = tree_digest(cache / 'fastqc')
    result['samtools_binary_sha256'] = digest(cache / 'umi/bin/samtools')
    java = cache / 'umi/lib/jvm/bin/java'
    version = checked([str(java), '-version'], env=env, text=True, timeout=30)
    result['java'] = {"version": version.stderr.strip(), "executable_sha256": digest(java.resolve())}
    return result


def verify_inputs(manifest: dict) -> None:
    for dataset in manifest['datasets']:
        if dataset['kind'] == 'fastq':
            path = Path(dataset['path'])
            if digest(path) != dataset['sha256'] or path.stat().st_size != dataset['bytes']:
                raise ValueError(f'input changed: {path}')
        else:
            for name, checksum in dataset['sha256'].items():
                path = Path(dataset['unsorted']).parent / name
                if digest(path) != checksum:
                    raise ValueError(f'input changed: {path}')


def telemetry(cpu: int) -> dict:
    temperatures = {}
    for sensor in Path('/sys/class/hwmon').glob('hwmon*'):
        try:
            if (sensor / 'name').read_text().strip() != 'coretemp':
                continue
            for label in sensor.glob('temp*_label'):
                value = label.with_name(label.name.replace('_label', '_input'))
                temperatures[label.read_text().strip()] = int(value.read_text()) / 1000
        except (OSError, ValueError):
            continue
    frequency = Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq')
    return {"load_average": os.getloadavg(), "temperature_c": temperatures,
            "cpu_frequency_khz": int(frequency.read_text()) if frequency.exists() else None}


def warm(paths: list[Path]) -> None:
    for p in paths:
        with p.open('rb') as f:
            while f.read(8 * 1024 * 1024):
                pass


def hash_decoded(path: Path) -> str:
    h = hashlib.sha256()
    opener = gzip.open if path.suffix == '.gz' else open
    with opener(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def bam_summary(path: Path, samtools: Path, expected_qnames: Path | None) -> dict:
    h = hashlib.sha256()
    count = 0
    names = []
    refs = {}
    previous = None
    tied = []
    checked([str(samtools), 'quickcheck', str(path)], timeout=30)
    with tempfile.TemporaryFile() as errors:
        proc = subprocess.Popen([str(samtools), 'view', '-h', str(path)], stdout=subprocess.PIPE, stderr=errors)
        watchdog = threading.Timer(120, proc.kill)
        watchdog.daemon = True
        watchdog.start()
        try:
            assert proc.stdout is not None
            with proc.stdout:
                for line in proc.stdout:
                    if line.startswith(b'@'):
                        if line.startswith(b'@PG\t'):
                            continue
                        if line.startswith(b'@SQ\t'):
                            name = next(x[3:] for x in line.rstrip().split(b'\t') if x.startswith(b'SN:'))
                            refs[name] = len(refs)
                        h.update(line)
                        continue
                    fields = line.rstrip(b'\n').split(b'\t')
                    key = (refs.get(fields[2], len(refs)), int(fields[3]))
                    if previous is not None and key < previous:
                        raise ValueError('BAM output is not coordinate sorted')
                    if key != previous:
                        for item in sorted(tied):
                            h.update(item)
                        tied = []
                    tied.append(line)
                    previous = key
                    count += 1
                    if expected_qnames is not None:
                        names.append(fields[0].decode())
            for item in sorted(tied):
                h.update(item)
            code = proc.wait()
        finally:
            watchdog.cancel()
            if proc.poll() is None:
                proc.kill()
            proc.wait()
        errors.seek(0)
        stderr = errors.read()
    if code != 0:
        raise ValueError(f'cannot decode BAM: {stderr.decode(errors="replace")}')
    expected_ok = None if expected_qnames is None else sorted(names) == expected_qnames.read_text().splitlines()
    return {"normalized_sha256": h.hexdigest(), "records": count, "coordinate_sorted": True,
            "independent_representatives": expected_ok, "decoder_stderr": stderr.decode(errors='replace')}


def parse_qc(path: Path) -> OrderedDict:
    modules = OrderedDict()
    name = None
    for line in path.read_text().splitlines():
        if line == '>>END_MODULE':
            name = None
        elif line.startswith('>>'):
            name = line[2:].split('\t')[0]
            if name in modules:
                raise ValueError('duplicate QC module')
            modules[name] = [line]
        elif name is not None:
            modules[name].append(line)
    return modules


def verify(case: str, dataset: dict, outputs: dict, tools: dict, negative: dict | None) -> dict:
    if case.startswith('cutadapt') or case in ('fx_stats', 'fx_fq2fa', 'bam_flagstat'):
        hashes = {side: hash_decoded(path) for side, path in outputs.items()}
        return {"passed": hashes['rust'] == hashes['upstream'], "rule": "decoded output bytes",
                "sha256": hashes}
    if case in ('bam_sort', 'umi_dedup'):
        expected = Path(dataset['expected_qnames']) if case == 'umi_dedup' else None
        summaries = {side: bam_summary(path, tools['samtools'], expected) for side, path in outputs.items()}
        n = dataset['expected_dedup_records'] if expected else dataset['records']
        ok = all(x['records'] == n and (expected is None or x['independent_representatives']) for x in summaries.values())
        return {"passed": ok and summaries['rust']['normalized_sha256'] == summaries['upstream']['normalized_sha256'],
                "rule": "BAM without @PG; coordinate order required; equal-coordinate ties canonicalized",
                "expected_records": n, "observations": summaries}
    if case == 'fq_lint':
        return {"passed": dataset['independent_fastq_validation'] and bool(negative and negative['passed']),
                "rule": "valid-input exit verdict plus independent FASTQ scan and invalid-tail controls",
                "negative_control": negative}
    if case == 'fastqc':
        modules = {side: parse_qc(path) for side, path in outputs.items()}
        observations = {}
        basic_ok = True
        for side, data in modules.items():
            basic = dict(line.split('\t', 1) for line in data['Basic Statistics'][1:] if not line.startswith('#'))
            length = str(dataset['min_length']) if dataset['min_length'] == dataset['max_length'] else f"{dataset['min_length']}-{dataset['max_length']}"
            unit = next((divisor, name) for divisor, name in [(10**9,'Gbp'),(10**6,'Mbp'),(10**3,'kbp'),(1,'bp')] if dataset['bases'] >= divisor)
            scaled = dataset['bases'] * 10 // unit[0]
            displayed_bases = f"{scaled // 10}" + (f".{scaled % 10}" if scaled % 10 else '') + ' ' + unit[1]
            valid = basic['Total Sequences'] == str(dataset['records']) and basic['Sequence length'] == length and basic['Total Bases'] == displayed_bases
            basic_ok &= valid
            observations[side] = {"basic": basic, "independent_count_bases_and_length": valid, "sha256": digest(outputs[side])}
        all_names = list(dict.fromkeys([*modules['rust'], *modules['upstream']]))
        equality = {name: modules['rust'].get(name) == modules['upstream'].get(name) for name in all_names}
        differences = {name: {side: modules[side].get(name) for side in modules} for name, equal in equality.items() if not equal}
        return {"passed": basic_ok, "rule": "independent read count, displayed total bases and length; full raw module comparisons retained",
                "full_module_equality": all(equality.values()), "module_equal": equality,
                "differences": differences, "observations": observations,
                "equal_work": False, "speed_ratio_eligible": False}
    raise ValueError(case)


def commands(case: str, d: dict, side: str, directory: Path, rust: dict, tools: dict, cpu: str, multi_cpu: str) -> tuple[list[str], Path, str, list[Path]]:
    out = directory / 'out'
    out.mkdir(parents=True)
    path = Path(d.get('path', d.get('unsorted', '')))
    affinity = multi_cpu if case == 'cutadapt_4cpu' else cpu
    compression = 'none'
    if case.startswith('cutadapt'):
        output = out / ('trimmed.fastq.gz' if case == 'cutadapt_gzip' else 'trimmed.fastq')
        executable = rust['cutadapt'] if side == 'rust' else tools['cutadapt']
        cmd = [str(executable), '--quiet', '-a', 'AGATCGGAAGAGC', '-m', '30', '-j', '4' if case == 'cutadapt_4cpu' else '1', '-o', str(output), str(path)]
        if side == 'upstream':
            cmd += ['--compression-level=6']
        compression = 'gzip level 6' if case == 'cutadapt_gzip' else 'none'
    elif case in ('fx_stats', 'fx_fq2fa'):
        executable = rust['fx'] if side == 'rust' else tools['seqkit']
        output = out / ('stats.tsv' if case == 'fx_stats' else 'reads.fasta')
        cmd = [str(executable), 'stats', '-a', '-T'] if case == 'fx_stats' else [str(executable), 'fq2fa']
        cmd += ['-j', '1', '-o', str(output), str(path)]
    elif case == 'fq_lint':
        executable = rust['fx'] if side == 'rust' else tools['fq']
        cmd = [str(executable), 'lint', str(path)]
        output = directory / 'stdout.txt'
    elif case == 'fastqc':
        executable = rust['fastqc'] if side == 'rust' else tools['fastqc']
        cmd = [str(executable), '--quiet', '--extract', '--outdir', str(out), str(path)]
        output = out / (path.stem + '_fastqc') / 'fastqc_data.txt'
        compression = 'report ZIP (different plot resources)'
    elif case == 'bam_flagstat':
        executable = rust['bam'] if side == 'rust' else tools['samtools']
        cmd = [str(executable), 'flagstat', str(path)]
        output = directory / 'stdout.txt'
    elif case == 'bam_sort':
        executable = rust['bam'] if side == 'rust' else tools['samtools']
        output = out / 'sorted.bam'
        opts = ['--max-memory', '32MiB', '--threads', '1'] if side == 'rust' else ['-m', '32M', '-@', '0', '-l', '6', '-T', str(directory / 'tmp' / 'sort')]
        cmd = [str(executable), 'sort', *opts, '-o', str(output), str(path)]
        compression = 'final BGZF level 6; temporary codec is implementation-specific'
    elif case == 'umi_dedup':
        path = Path(d['sorted'])
        executable = rust['umi'] if side == 'rust' else tools['umi_tools']
        output = out / 'dedup.bam'
        cmd = [str(executable), 'dedup', '-I', str(path), '-S', str(output), '--extract-umi-method=tag', '--umi-tag=UB', '--method=directional', '--random-seed=1', '--compresslevel=6', '-L', str(out / 'dedup.log')]
        if side == 'rust':
            cmd += ['--threads', '1', '--tmpdir', str(directory / 'tmp')]
        compression = 'BGZF level 6'
    else:
        raise ValueError(case)
    return ['taskset', '-c', affinity, *cmd], output, compression, [path]


def run_one(cmd: list[str], directory: Path, paths: list[Path], env: dict, cooldown: float, timeout: float, cpu: int) -> dict:
    directory.mkdir(parents=True, exist_ok=True)
    temporary = directory / 'tmp'
    temporary.mkdir(exist_ok=True)
    prepare_upstream_temp(temporary)
    warm(paths)
    time.sleep(cooldown)
    before = telemetry(cpu)
    with legacy.DiskSampler(temporary) as disk, (directory / 'stdout.txt').open('wb') as out, (directory / 'stderr.txt').open('wb') as err:
        begin = time.perf_counter()
        proc = subprocess.Popen(['/usr/bin/time', '-v', '-o', str(directory / 'time.txt'), *cmd],
                                cwd=directory, env=dict(env, TMPDIR=str(temporary)), stdout=out, stderr=err, start_new_session=True)
        timed_out = threading.Event()
        def terminate():
            try:
                os.killpg(proc.pid, signal.SIGKILL)
                timed_out.set()
            except ProcessLookupError:
                pass
        watchdog = threading.Timer(timeout, terminate)
        watchdog.daemon = True
        watchdog.start()
        try:
            code = proc.wait()
        finally:
            watchdog.cancel()
        elapsed = time.perf_counter() - begin
        if timed_out.is_set():
            code = 124
    measurements = legacy.parse_time(directory / 'time.txt')
    measurements.update({"elapsed_s": elapsed, "exit_code": code, "temp_peak_bytes_sampled": disk.peak,
                         "before": before, "after": telemetry(cpu), "command": cmd,
                         "output_bytes": sum(p.stat().st_size for p in (directory / 'out').rglob('*') if p.is_file()),
                         "stderr": (directory / 'stderr.txt').read_text(errors='replace')[:4000]})
    return measurements


def lint_negative(d: dict, tools: dict, rust: dict, root: Path, cpu: str, env: dict) -> dict:
    path = root / 'invalid-tail.fastq'
    with path.open('wb') as out, Path(d['path']).open('rb') as source:
        shutil.copyfileobj(source, out)
        out.write(b'@invalid_tail\nACGT\n+\nI\n')
    codes = {}
    for side, exe in [('rust', rust['fx']), ('upstream', tools['fq'])]:
        r = subprocess.run(['taskset', '-c', cpu, str(exe), 'lint', str(path)], env=env, capture_output=True, timeout=300)
        codes[side] = {"exit_code": r.returncode, "stderr": r.stderr.decode(errors='replace')[-1800:]}
    checksum = digest(path)
    path.unlink()
    return {"passed": all(x['exit_code'] == 1 for x in codes.values()), "sha256": checksum, "observations": codes}


def summarize(samples: list[dict], n: int) -> dict:
    values = [s['elapsed_s'] for s in samples]
    median = statistics.median(values)
    return {"median_s": median, "min_s": min(values), "max_s": max(values),
            "records_per_s": n / median, "max_rss_kib": max(s['max_rss_kb'] or 0 for s in samples),
            "cpu_median_s": statistics.median((s['user_s'] or 0) + (s['sys_s'] or 0) for s in samples),
            "temp_peak_bytes_sampled": max(s['temp_peak_bytes_sampled'] for s in samples),
            "output_bytes": [s['output_bytes'] for s in samples],
            "payload_bytes": [s['payload_bytes'] for s in samples]}


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--inputs', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--cutadapt', type=Path, required=True)
    p.add_argument('--oracle-dir', type=Path, default=ROOT / 'tests/ci/env/bin')
    p.add_argument('--cpu', default='4')
    p.add_argument('--multi-cpu', default='0,2,4,6')
    p.add_argument('--repeats', type=int, default=5)
    p.add_argument('--warmups', type=int, default=1)
    p.add_argument('--cooldown', type=float, default=2)
    p.add_argument('--timeout', type=float, default=300)
    p.add_argument('--datasets', default='')
    p.add_argument('--cases', default='')
    p.add_argument('--pilot', action='store_true')
    p.add_argument('--run-name', default='run')
    p.add_argument('--toolchain-cache', type=Path, default=Path('/tmp/bioinfo-ci-toolchain'))
    args = p.parse_args()
    chosen = set(args.datasets.split(',')) - {''}
    chosen_cases = set(args.cases.split(',')) - {''}
    if chosen_cases - CASES:
        p.error(f'unknown cases: {sorted(chosen_cases - CASES)}')
    if args.repeats < 1 or args.warmups < 0 or args.cooldown < 0:
        p.error('invalid repetitions/warmup/cooldown')
    if not args.run_name or Path(args.run_name).name != args.run_name:
        p.error('run-name must be a single directory name')
    root = args.inputs.resolve().parent / args.run_name
    if args.out.exists() or root.exists():
        p.error('refusing to overwrite an existing output or run directory')
    args.cpu = str(int(args.cpu))
    cpus = {int(x) for x in args.multi_cpu.split(',')} | {int(args.cpu)}
    if not cpus <= os.sched_getaffinity(0):
        p.error('requested CPU affinity is unavailable')
    cache = args.toolchain_cache.resolve()
    if not (cache / '.ready').is_file():
        p.error('pre-extracted oracle toolchain with .ready is required')
    root.mkdir(parents=True)
    probes = root / 'probes'
    probes.mkdir()
    (probes / 'bioinfo-ci-toolchain').symlink_to(cache, target_is_directory=True)
    # Both probes and timed wrapper calls use this exact pre-extracted cache.
    os.environ['BIOINFO_TOOLCHAIN_CACHE'] = str(cache)
    env = dict(os.environ, TMPDIR=str(probes), LC_ALL='C', PYTHONHASHSEED='0', OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1', MKL_NUM_THREADS='1', RUST_CUTADAPT_COMPRESSION_LEVEL='6')
    env.pop('RUST_CUTADAPT_CHUNK', None)
    rust = {key: ROOT / f'target/release/{name}' for key, name in [('cutadapt','rust-cutadapt'),('umi','rust-umitools'),('bam','rust-bam'),('fastqc','rust-fastqc'),('fx','rust-fx')]}
    tools = {name: (args.cutadapt if name == 'cutadapt' else args.oracle_dir / name).resolve() for name in PINNED}
    versions = {}
    for name, exe in {**{f'rust_{k}': v for k,v in rust.items()}, **tools}.items():
        cmd = [str(exe), 'version' if name == 'seqkit' else '--version']
        result = checked(cmd, env=env, text=True)
        banner = (result.stdout or result.stderr).strip().splitlines()[0]
        if name in PINNED and not (banner == PINNED[name] or (name == 'fq' and banner.startswith(PINNED[name] + ' '))):
            raise ValueError(f'wrong pinned version: {name}: {banner}')
        versions[name] = {"version": banner, "command": cmd, "executable_sha256": digest(exe)}
    manifest = json.loads(args.inputs.read_text())
    verify_inputs(manifest)
    try:
        plan = planned_conditions(manifest, chosen, chosen_cases)
    except ValueError as error:
        p.error(str(error))
    report = {"schema_version": 1, "planned_conditions": plan, "kind": "readme-pilot" if args.pilot else "readme-benchmark",
              "started_utc": dt.datetime.now(dt.timezone.utc).isoformat(), "source": legacy.git_state(),
              "hardware": legacy.hardware(), "topology": checked(['lscpu','-e=CPU,CORE,SOCKET,MAXMHZ,ONLINE'],text=True).stdout,
              "versions": versions, "runtime_payloads": runtime_provenance(cache, tools['cutadapt'], env), "native_source_sha256": {str(f.relative_to(ROOT)): digest(f) for f in [ROOT/'Cargo.lock',ROOT/'Cargo.toml',*sorted(ROOT.glob('rust-*/src/**/*.rs')),*sorted(ROOT.glob('bioinfo-*/src/**/*.rs'))]},
              "script_sha256": {f.name: digest(f) for f in [Path(__file__),ROOT/'bench/readme_inputs.py',ROOT/'bench/run.py',ROOT/'bench/upstream.py']},
              "inputs": manifest, "input_manifest_sha256": digest(args.inputs), "parameters": vars(args) | {"seed": 20261031}, "results": []}
    def save():
        args.out.parent.mkdir(parents=True, exist_ok=True)
        text = json.dumps(report, indent=2, default=str) + '\n'
        partial = args.out.with_suffix('.partial')
        partial.write_text(text)
        partial.replace(args.out)
    save()
    rng = random.Random(20261031)
    for d in manifest['datasets']:
        if chosen and d['id'] not in chosen:
            continue
        for case in dataset_cases(d):
            if chosen_cases and case not in chosen_cases:
                continue
            condition = root / d['id'] / case
            if condition.exists():
                raise ValueError(f'refusing to overwrite existing condition {condition}')
            condition.mkdir(parents=True)
            row = {"dataset": d['id'], "records": d['records'], "case": case, "samples": {"rust": [], "upstream": []}, "warmups": [], "checks": [], "cpu_affinity": args.multi_cpu if case == 'cutadapt_4cpu' else args.cpu, "passed": False}
            report['results'].append(row)
            save()
            try:
                negative = lint_negative(d, tools, rust, condition, args.cpu, env) if case == 'fq_lint' else None
            except Exception as error:
                row['checks'].append({"passed": False, "reason": f'negative-control failure: {error}'})
                save()
                continue
            extra = rng.choice(['rust', 'upstream'])
            first = ['rust', 'upstream'] * (args.repeats//2) + ([extra] if args.repeats % 2 else [])
            rng.shuffle(first)
            for index in range(-args.warmups, args.repeats):
                lead = rng.choice(['rust','upstream']) if index < 0 else first[index]
                order = ['rust','upstream'] if lead == 'rust' else ['upstream','rust']
                outputs = {}; observations = {}; directories = {}
                for side in order:
                    directory = condition / f'{index}-{side}'
                    directory.mkdir()
                    cmd, output, compression, paths = commands(case,d,side,directory,rust,tools,args.cpu,args.multi_cpu)
                    row['compression'] = compression
                    try:
                        observation = run_one(cmd,directory,paths,env,args.cooldown,args.timeout,int(args.cpu))
                    except Exception as error:
                        row['checks'].append({"passed": False, "reason": f'execution failure: {error}', "side": side, "block": index})
                        save()
                        raise
                    observation.update({"block": index, "order_in_block": order.index(side),
                                        "payload_bytes": output.stat().st_size if output.is_file() else None})
                    outputs[side] = output; observations[side] = observation; directories[side] = directory
                    if index >= 0:
                        row['samples'][side].append(observation)
                    save()
                try:
                    if any(x['exit_code'] != 0 for x in observations.values()):
                        check = {"passed": False, "reason": "nonzero execution", "exit_codes": {k:v['exit_code'] for k,v in observations.items()}}
                    else:
                        check = verify(case,d,outputs,tools,negative)
                except Exception as error:
                    check = {"passed": False, "reason": str(error)}
                check['block'] = index
                row['checks'].append(check)
                if index < 0:
                    row['warmups'].append(observations)
                print(f"{d['id']} {case} block={index} order={order} ok={check['passed']} wall=" + str({k:round(v['elapsed_s'],4) for k,v in observations.items()}),flush=True)
                save()
                if check['passed']:
                    for directory in directories.values():
                        shutil.rmtree(directory / 'out')
                else:
                    break
            row['passed'] = len(row['samples']['rust']) == args.repeats and all(x['passed'] for x in row['checks'])
            if row['samples']['rust']:
                row['summary'] = {side: summarize(samples,d['records']) for side,samples in row['samples'].items()}
                if row['passed'] and case != 'fastqc':
                    ratios = [u['elapsed_s']/r['elapsed_s'] for r,u in zip(row['samples']['rust'],row['samples']['upstream'])]
                    row['paired_speed_ratio'] = {"samples": ratios, "median": statistics.median(ratios), "min": min(ratios), "max": max(ratios)}
            save()
    report['completed_utc'] = dt.datetime.now(dt.timezone.utc).isoformat()
    report['complete_matrix'] = plan == [{'dataset':r['dataset'], 'case':r['case']} for r in report['results']]
    report['passed'] = report['complete_matrix'] and all(r['passed'] for r in report['results'])
    save()
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
