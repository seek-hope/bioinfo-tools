#!/usr/bin/env python3
"""Summarize a complete formal scale run; upstream/current are the primary arms."""
import argparse
import csv
import io
import json
from pathlib import Path
import statistics

from scale_common import ARMS, validate_complete

LABELS = {'cutadapt_plain': 'cutadapt plain', 'cutadapt_gzip': 'cutadapt gzip6 output',
          'fx_stats': 'SeqKit stats -a -T', 'fx_fq2fa': 'SeqKit fq2fa', 'fq_lint': 'fq lint',
          'bam_flagstat': 'samtools flagstat', 'bam_sort': 'samtools sort',
          'umi_dedup': 'UMI-tools directional dedup', 'fastqc': 'FastQC (descriptive)'}


def summarize(report):
    validate_complete(report)
    result = []
    for row in report['results']:
        samples = {(o['block'], o['arm']): o for o in row['observations'] if o['block'] >= 0}
        entry = {'condition': row['condition']['id'], 'case': row['condition']['case'],
                 'dataset': row['dataset'], 'records': row['records'], 'input_bytes': row['input_bytes'], 'cores': row['condition']['cores'],
                 'cpu_affinity': row['condition']['cpu_affinity'], 'arms': {}}
        for arm in ARMS:
            observations = [samples[(b, arm)] for b in range(3)]
            wall = [o['elapsed_s'] for o in observations]
            entry['arms'][arm] = {'median_s': statistics.median(wall), 'min_s': min(wall), 'max_s': max(wall),
                'max_rss_mib': max(o['max_rss_kb'] for o in observations) / 1024,
                'median_effective_cpus': statistics.median((o['user_s'] + o['sys_s']) / o['elapsed_s'] for o in observations),
                'max_temp_bytes_sampled': max(o['temp_peak_bytes_sampled'] for o in observations),
                'median_fsync_s': statistics.median(o['fsync_s'] for o in observations),
                'observations_below_10s': sum(t < 10 for t in wall)}
        if entry['case'] != 'fastqc':
            ratios = [samples[(b, 'upstream')]['elapsed_s'] / samples[(b, 'current')]['elapsed_s'] for b in range(3)]
            entry['paired_upstream_over_current'] = {'samples': ratios, 'median': statistics.median(ratios),
                                                     'min': min(ratios), 'max': max(ratios)}
        result.append(entry)
    return result


def time_cell(data):
    return f"{data['median_s']:.2f} [{data['min_s']:.2f}–{data['max_s']:.2f}]"


def tables(rows):
    lines = ['| Workload | Repeated population | Input records | Input GiB | CPU budget | Upstream s, median [min–max] | Current s, median [min–max] | Paired upstream/current |',
             '|---|---|---:|---:|---:|---:|---:|---:|']
    resources = ['| Condition | Peak RSS MiB upstream/current | Effective CPUs upstream/current | Sampled temp GiB upstream/current | Untimed fsync s upstream/current |',
                 '|---|---:|---:|---:|---:|']
    for row in rows:
        u, c = row['arms']['upstream'], row['arms']['current']
        population = 'real FASTQ subset' if row['dataset'].startswith('real-') else ('synthetic BAM' if 'bam' in row['dataset'] else 'synthetic FASTQ')
        ratio = row.get('paired_upstream_over_current')
        ratio_cell = f"{ratio['median']:.2f}×" if ratio else '— unequal report work'
        lines.append(f"| {LABELS[row['case']]} | {population} | {row['records']/1e6:g}M | {row['input_bytes']/2**30:.2f} | {row['cores']} | {time_cell(u)} | {time_cell(c)} | {ratio_cell} |")
        resources.append(f"| {row['condition']} | {u['max_rss_mib']:.1f}/{c['max_rss_mib']:.1f} | {u['median_effective_cpus']:.2f}/{c['median_effective_cpus']:.2f} | {u['max_temp_bytes_sampled']/2**30:.3f}/{c['max_temp_bytes_sampled']/2**30:.3f} | {u['median_fsync_s']:.3f}/{c['median_fsync_s']:.3f} |")
    return '\n'.join(lines) + '\n', '\n'.join(resources) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--outdir', type=Path, required=True)
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    rows = summarize(report)
    args.outdir.mkdir(parents=True, exist_ok=False)
    (args.outdir/'summary.json').write_text(json.dumps(rows, indent=2) + '\n')
    performance, resources = tables(rows)
    (args.outdir/'tables.md').write_text(performance)
    (args.outdir/'resources.md').write_text(resources)
    buffer = io.StringIO()
    writer = csv.writer(buffer, delimiter='\t', lineterminator='\n')
    writer.writerow(['condition', 'records', 'cores', 'upstream_median_s', 'current_median_s', 'paired_upstream_over_current'])
    for row in rows:
        writer.writerow([row['condition'], row['records'], row['cores'], row['arms']['upstream']['median_s'],
                         row['arms']['current']['median_s'], row.get('paired_upstream_over_current', {}).get('median', '')])
    (args.outdir/'tables.tsv').write_text(buffer.getvalue())
    print(f"SCALE SUMMARY PASS: {len(rows)} conditions, {len(rows)*6} measured observations, {len(rows)*2} warmups")


if __name__ == '__main__':
    main()
