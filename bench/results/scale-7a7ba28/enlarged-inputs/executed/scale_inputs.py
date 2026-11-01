#!/usr/bin/env python3
"""Expand frozen source populations for SCALE-PROTOCOL.md; never overwrite inputs."""
from __future__ import annotations
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

from readme_inputs import digest, scan_fastq
from scale_common import validate_plan

HEADROOM = 1024 ** 3


def require_space(directory: Path, size: int) -> None:
    if shutil.disk_usage(directory).free < size + HEADROOM:
        raise OSError(f'insufficient space: need {size + HEADROOM} bytes at {directory}')


def flush(path: Path) -> None:
    with path.open('rb') as handle:
        os.fsync(handle.fileno())


def repeat_fastq(source: dict, multiplicity: int, output: Path) -> dict:
    if type(multiplicity) is not int or multiplicity < 1:
        raise ValueError('multiplicity must be a positive integer')
    path = Path(source['path'])
    observed = scan_fastq(path)
    for key in ['records', 'bases', 'gc_bases', 'min_length', 'max_length', 'bytes', 'sha256']:
        if observed[key] != source[key]:
            raise ValueError(f'source changed: {key}: {path}')
    with path.open('rb') as handle:
        handle.seek(-1, os.SEEK_END)
        if handle.read(1) != b'\n':
            raise ValueError('repeated FASTQ source must end in a newline')
    require_space(output.parent, source['bytes'] * multiplicity)
    expected = hashlib.sha256()
    with output.open('xb') as out:
        for _ in range(multiplicity):
            with path.open('rb') as handle:
                while chunk := handle.read(8 * 1024 * 1024):
                    out.write(chunk)
                    expected.update(chunk)
        out.flush()
        os.fsync(out.fileno())
    checksum = digest(output)
    if checksum != expected.hexdigest() or digest(path) != source['sha256']:
        raise ValueError('FASTQ repetition failed integrity checks')
    result = dict(source, path=str(output), sha256=checksum)
    for key in ['records', 'bases', 'gc_bases', 'bytes']:
        result[key] = source[key] * multiplicity
    if output.stat().st_size != result['bytes']:
        raise ValueError('repeated FASTQ size mismatch')
    result['validation'] = 'independent full source FASTQ scan; exact repetition hash verified after fsync'
    result['independent_fastq_validation'] = True
    return result


def checked(command: list[str], log: list[dict]) -> str:
    proc = subprocess.run(command, capture_output=True, text=True)
    log.append({'command': command, 'exit': proc.returncode,
                'stdout': proc.stdout, 'stderr': proc.stderr})
    if proc.returncode:
        raise RuntimeError(log[-1])
    return proc.stdout


def decoded_bam_bytes(path: Path) -> int:
    with gzip.open(path, 'rb') as decoded:
        if decoded.read(4) != b'BAM\x01':
            raise ValueError('source is not BGZF BAM')
        return 4 + sum(len(chunk) for chunk in iter(lambda: decoded.read(8 * 1024 * 1024), b''))


def storage_estimate(groups: dict, sources: dict) -> dict:
    """Reserve the entire input population plus the largest generation/run peak."""
    input_bytes = 0
    generation_peak = 0
    execution_peak = 0
    decoded_sources = {}
    for (source_id, multiple), workloads in groups.items():
        source = sources[source_id]
        if source['kind'] == 'fastq':
            size = source['bytes'] * multiple
            input_bytes += size
            for work in workloads:
                if work['case'] in ('cutadapt_plain', 'cutadapt_gzip', 'fx_fq2fa'):
                    execution_peak = max(execution_peak, (size * 21 + 9) // 10)
                elif work['case'] == 'fq_lint':
                    execution_peak = max(execution_peak, size + 1024 ** 2)
        else:
            path = Path(source['unsorted'])
            if source_id not in decoded_sources:
                decoded_sources[source_id] = decoded_bam_bytes(path)
            decoded = decoded_sources[source_id] * multiple
            input_bytes += path.stat().st_size * multiple + 128 * 1024 * multiple + 1024 ** 2
            if any(w['case'] == 'umi_dedup' for w in workloads):
                input_bytes += 2 * decoded  # sorted BAM, index, and representative list
                generation_peak = max(generation_peak, 4 * decoded)
            if any(w['case'] in ('umi_dedup', 'bam_sort') for w in workloads):
                execution_peak = max(execution_peak, 8 * decoded)
    headroom = 10 * 1024 ** 3 + HEADROOM
    return {'input_upper_bound_bytes': input_bytes, 'generation_scratch_upper_bound_bytes': generation_peak,
            'execution_outputs_and_runs_upper_bound_bytes': execution_peak, 'headroom_bytes': headroom,
            'required_free_bytes': input_bytes + max(generation_peak, execution_peak) + headroom,
            'decoded_source_bytes': decoded_sources}


def repeat_bam(source: dict, multiplicity: int, directory: Path,
               samtools: Path, sorted_needed: bool) -> dict:
    if type(multiplicity) is not int or multiplicity < 1:
        raise ValueError('multiplicity must be a positive integer')
    if sorted_needed:
        profile = source.get('generator', {})
        if (profile.get('umi_counts_per_scale') != [6, 2, 1, 1]
                or profile.get('umi_distance_from_major') != [0, 1, 2, 10]
                or profile.get('independent_representative') != 'unique highest MAPQ'):
            raise ValueError('UMI repetition requires the frozen directional-graph population')
    path = Path(source['unsorted'])
    if digest(path) != source['sha256'][path.name]:
        raise ValueError(f'BAM source changed: {path}')
    # cat copies compressed records; allow two recompressed boundary blocks per
    # source copy. Sorting additionally reserves four full decoded populations
    # for temporary runs and outputs. Measure the source expansion instead of
    # assuming that every record is a fixed number of bytes.
    space_estimate = path.stat().st_size * multiplicity + 128 * 1024 * multiplicity + 1024 ** 2
    decoded_bytes = decoded_bam_bytes(path)
    if sorted_needed:
        space_estimate += 4 * decoded_bytes * multiplicity
    require_space(directory.parent, space_estimate)
    directory.mkdir()
    output = directory / 'unsorted.bam'
    log = []
    try:
        checked([str(samtools), 'cat', '--no-PG', '-o', str(output), *([str(path)] * multiplicity)], log)
        flush(output)
        checked([str(samtools), 'quickcheck', str(output)], log)
        count = int(checked([str(samtools), 'view', '-c', str(output)], log))
        if count != source['records'] * multiplicity:
            raise ValueError('repeated BAM count mismatch')
        result = dict(source, records=count, unsorted=str(output),
                      sha256={output.name: digest(output)}, bytes=output.stat().st_size,
                      source_decoded_bytes=decoded_bytes, decoded_bytes_upper_bound=decoded_bytes * multiplicity,
                      artifact_bytes={output.name: output.stat().st_size}, preparation_space_estimate=space_estimate)
        result.pop('sorted', None)
        result.pop('expected_qnames', None)
        if sorted_needed:
            sorted_path = directory / 'sorted.bam'
            temporary = directory / 'tmp'
            temporary.mkdir()
            checked([str(samtools), 'sort', '--no-PG', '-@', '3', '-m', '128M', '-l', '6',
                     '-T', str(temporary / 'sort'), '-o', str(sorted_path), str(output)], log)
            flush(sorted_path)
            checked([str(samtools), 'quickcheck', str(sorted_path)], log)
            checked([str(samtools), 'index', str(sorted_path)], log)
            if int(checked([str(samtools), 'view', '-c', str(sorted_path)], log)) != count:
                raise ValueError('sorted BAM count mismatch')
            expected = Path(source['expected_qnames'])
            if digest(expected) != source['sha256'][expected.name]:
                raise ValueError('independent representative list changed')
            shutil.copy2(expected, directory / expected.name)
            result.update(sorted=str(sorted_path), expected_qnames=str(directory / expected.name))
            for item in [sorted_path, Path(str(sorted_path) + '.bai'), directory / expected.name]:
                flush(item)
                result['sha256'][item.name] = digest(item)
                result['artifact_bytes'][item.name] = item.stat().st_size
            result['representative_proof'] = (
                'This frozen population has counts 6s/2s/s/s with s>=1 and pairwise Hamming '
                'distances 1,2,10 / 1,9 / 8. For any positive repetition m, the same two '
                'edges hold: 6ms>=2(2ms)-1 and 2ms>=2(ms)-1; both reverse inequalities '
                'remain false, and the distance-10 UMI stays isolated. This is specific '
                'to this population, not a general invariance of directional clustering. '
                'Copies of each unique highest-MAPQ representative are identical; the '
                'independent expected representative set therefore remains unchanged.')
        if digest(path) != source['sha256'][path.name]:
            raise ValueError('BAM source changed during generation')
        return result
    finally:
        (directory / 'generation.json').write_text(json.dumps(log, indent=2) + '\n')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--sources', type=Path, nargs='+', required=True)
    parser.add_argument('--outdir', type=Path, required=True)
    parser.add_argument('--samtools', type=Path, required=True)
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    validate_plan(plan)
    sources = {}
    for source in args.sources:
        for dataset in json.loads(source.read_text())['datasets']:
            identifier = dataset['id']
            if not isinstance(identifier, str) or Path(identifier).name != identifier or identifier in ('', '.', '..'):
                raise ValueError('invalid source ID')
            if identifier in sources:
                raise ValueError('duplicate source ID')
            if dataset['kind'] not in ('fastq', 'bam'):
                raise ValueError('unknown source kind')
            sources[identifier] = dataset
    groups = {}
    for workload in plan['workloads']:
        source_id, multiple = workload['source_id'], workload['multiplicity']
        if source_id not in sources or not isinstance(multiple, int) or multiple < 1:
            raise ValueError('invalid source or multiplicity')
        is_bam_case = workload['case'] in ('bam_flagstat', 'bam_sort', 'umi_dedup')
        if is_bam_case != (sources[source_id]['kind'] == 'bam'):
            raise ValueError('case does not apply to source kind')
        groups.setdefault((source_id, multiple), []).append(workload)
    output = args.outdir.resolve()
    if output.exists():
        raise FileExistsError(output)
    ancestor = output.parent
    while not ancestor.exists():
        ancestor = ancestor.parent
    estimate = storage_estimate(groups, sources)
    estimate['available_bytes_before_generation'] = shutil.disk_usage(ancestor).free
    require_space(ancestor, estimate['required_free_bytes'] - HEADROOM)
    output.mkdir(parents=True, exist_ok=False)
    executed = output/'executed'
    executed.mkdir()
    script_hashes = {}
    for name in ['scale_inputs.py', 'scale_common.py', 'readme_inputs.py', 'readme_benchmark.py', 'run.py', 'upstream.py']:
        source_script = Path(__file__).parent/name
        shutil.copy2(source_script, executed/name)
        script_hashes[name] = digest(executed/name)
    report = {'schema_version': 1, 'passed': False, 'plan_sha256': digest(args.plan), 'plan': plan,
              'source_manifest_sha256': {str(p.resolve()): digest(p) for p in args.sources},
              'generator_sha256': digest(Path(__file__)), 'executed_script_sha256': script_hashes,
              'storage_preflight': estimate, 'datasets': [], 'workloads': []}
    try:
        for (source_id, multiple), workloads in groups.items():
            source = sources[source_id]
            identifier = f'{source_id}-x{multiple}'
            if source['kind'] == 'fastq':
                dataset = repeat_fastq(source, multiple, output / f'{identifier}.fastq')
            else:
                dataset = repeat_bam(source, multiple, output / identifier, args.samtools.resolve(),
                                     any(w['case'] == 'umi_dedup' for w in workloads))
            dataset.update(id=identifier, source_population=source_id, multiplicity=multiple,
                           source_metadata=source,
                           generator={'method': 'exact frozen population repetition',
                                      'multiplicity': multiple, 'original_generator': source.get('generator')})
            report['datasets'].append(dataset)
            report['workloads'].extend(dict(w, dataset=identifier) for w in workloads)
            (output / 'inputs.json').write_text(json.dumps(report, indent=2) + '\n')
            print(identifier, dataset['records'], flush=True)
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        (output / 'inputs.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
