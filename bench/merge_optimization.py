#!/usr/bin/env python3
"""Join ten complete core conditions with the complete 10M retry, retaining failures."""
import argparse
import copy
import json
from pathlib import Path

from optimization_benchmark import complete, plan as planned_conditions
from readme_inputs import digest


def merge(core, streaming):
    plan = core['planned_conditions']
    expected = [{'dataset': dataset['id'], 'case': case}
                for dataset, case in planned_conditions(core['inputs'], False)]
    if plan != expected or len(plan) != 13 or len(core['results']) != 11:
        raise ValueError('expected the full plan, ten complete rows and one interrupted warm-up')
    interrupted = core['results'][10]
    if (interrupted['passed'] or interrupted['checks']
            or any(interrupted['samples'][arm] for arm in ('before', 'after', 'upstream'))
            or {'dataset': interrupted['dataset'], 'case': interrupted['case']} != plan[10]):
        raise ValueError('cannot discard measured observations or a completed/failed check')
    first = copy.deepcopy(core)
    first['planned_conditions'] = plan[:10]
    first['results'] = core['results'][:10]
    if not complete(first):
        raise ValueError('core conditions are incomplete or failed')
    if streaming['planned_conditions'] != plan[10:] or not streaming['passed'] or not complete(streaming):
        raise ValueError('all three streaming conditions must pass with five repeats')
    for key in ('versions', 'source_sha256', 'input_manifest_sha256', 'inputs', 'topology'):
        if core[key] != streaming[key]:
            raise ValueError(f'changed experiment identity: {key}')
    for key in ('cpu', 'repeats', 'warmups', 'cooldown', 'seed', 'baseline_dir', 'native_dir',
                'oracle_dir', 'toolchain_cache', 'pilot'):
        if core['parameters'][key] != streaming['parameters'][key]:
            raise ValueError(f'changed parameter: {key}')
    for name, checksum in core['script_sha256'].items():
        if name != 'optimization_benchmark.py' and streaming['script_sha256'].get(name) != checksum:
            raise ValueError(f'changed command, timing or comparison implementation: {name}')
    result = copy.deepcopy(core)
    result['results'] = core['results'][:10] + streaming['results']
    result['completed_utc'] = streaming['completed_utc']
    result['parameters']['outdir'] = 'see stage_reports for original execution directories'
    result['stage_reports'] = [{key: report[key] for key in ('kind', 'parameters', 'script_sha256', 'source', 'started_utc')}
                               for report in (core, streaming)]
    result['retry_reason'] = 'tmpfs ENOSPC during the first 10M warm-up; retained the ten fully checked core conditions and reran all three 10M conditions'
    result['passed'] = complete(result)
    if not result['passed']:
        raise ValueError('combined population is incomplete')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core', type=Path, required=True)
    parser.add_argument('--streaming', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists():
        parser.error('refusing to overwrite a report')
    result = merge(json.loads(args.core.read_text()), json.loads(args.streaming.read_text()))
    for stage, path in zip(result['stage_reports'], (args.core, args.streaming)):
        stage.update(path=str(path), sha256=digest(path))
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(f"Optimization acceptance: {len(result['results'])}/13 conditions; {result['passed']}")


if __name__ == '__main__':
    main()
