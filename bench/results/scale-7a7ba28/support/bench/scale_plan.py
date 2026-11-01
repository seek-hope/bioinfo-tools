#!/usr/bin/env python3
"""Create, enlarge, or freeze a scale plan from complete calibration evidence."""
import argparse
import copy
import json
import math
from pathlib import Path

from readme_inputs import digest
from scale_common import initial_plan, validate_complete


def propose(report: dict, freeze: bool) -> dict:
    validate_complete(report, formal=False)
    plan = copy.deepcopy(report['plan'])
    minima = {workload['id']: min(sample['elapsed_s']
              for row in report['results'] if row['condition']['workload'] == workload['id']
              for sample in row['observations']) for workload in plan['workloads']}
    if freeze and any(value < plan['minimum_desired_seconds'] for value in minima.values()):
        raise ValueError('calibration still contains an arm/CPU budget shorter than the minimum')
    for workload in plan['workloads']:
        if not freeze:
            workload['multiplicity'] = max(workload['multiplicity'],
                math.ceil(workload['multiplicity'] * plan['target_fastest_seconds'] / minima[workload['id']]))
    plan['calibration_minima_seconds'] = minima
    plan['frozen_for_formal_run'] = freeze
    return plan


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--calibration', type=Path)
    parser.add_argument('--freeze', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.freeze and not args.calibration:
        parser.error('--freeze requires a complete calibration')
    plan = initial_plan() if args.calibration is None else propose(json.loads(args.calibration.read_text()), args.freeze)
    if args.calibration:
        plan['calibration_report'] = str(args.calibration.resolve())
        plan['calibration_report_sha256'] = digest(args.calibration)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open('x') as output:
        output.write(json.dumps(plan, indent=2) + '\n')


if __name__ == '__main__':
    main()
