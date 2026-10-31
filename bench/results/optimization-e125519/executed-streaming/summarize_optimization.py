#!/usr/bin/env python3
"""Render all completed optimization conditions; never summarize a pilot."""
import argparse
import csv
import io
import json
import math
from pathlib import Path
import statistics

import readme_benchmark as reference
from optimization_benchmark import ARMS, complete


LABELS = {'fx_stats': 'stats -a -T', 'fx_fq2fa': 'fq2fa', 'fq_lint': 'lint',
          'bam_flagstat': 'flagstat', 'bam_sort': 'sort (32 MiB)', 'fastqc': 'FastQC*'}


def render(report):
    if report['kind'] != 'optimization-benchmark' or report['parameters']['repeats'] != 5 or not report['passed'] or not complete(report):
        raise ValueError('only a complete, passing five-repeat formal experiment can be summarized')
    lines = ['| Dataset | Command | Before s [min–max] | After s [min–max] | Upstream s [min–max] | Paired before/after | Max RSS MiB before/after/upstream |',
             '| --- | --- | ---: | ---: | ---: | ---: | ---: |']
    stream = io.StringIO()
    writer = csv.writer(stream, delimiter='\t', lineterminator='\n')
    writer.writerow(['dataset', 'case', 'arm', 'median_s', 'min_s', 'max_s', 'records_per_s',
                     'max_rss_mib', 'cpu_median_s', 'temp_peak_bytes_sampled', 'paired_before_after_median'])
    for row in report['results']:
        samples = row['samples']
        if any(not math.isfinite(sample['elapsed_s']) or sample['elapsed_s'] <= 0
               for arm in ARMS for sample in samples[arm]):
            raise ValueError('invalid elapsed time')
        ratio = statistics.median(before['elapsed_s'] / after['elapsed_s']
                                  for before, after in zip(samples['before'], samples['after']))
        times, memory = [], []
        for arm in ARMS:
            value = reference.summarize(samples[arm], row['records'])
            times.append(f"{value['median_s']:.3f} [{value['min_s']:.3f}–{value['max_s']:.3f}]")
            memory.append(f"{value['max_rss_kib']/1024:.1f}")
            writer.writerow([row['dataset'], row['case'], arm, value['median_s'], value['min_s'], value['max_s'],
                             value['records_per_s'], value['max_rss_kib']/1024, value['cpu_median_s'],
                             value['temp_peak_bytes_sampled'], ratio])
        lines.append(f"| {row['dataset']} | {LABELS[row['case']]} | " + ' | '.join(times) +
                     f" | {ratio:.2f}× | " + ' / '.join(memory) + ' |')
    lines += ['', '*FastQC before/after compares identical native report bundles. Its upstream values are descriptive: missing native plots and unresolved statistical differences prevent an upstream speed ratio or whole-report equivalence claim.',
              '', 'Ratios above one favor the optimized native executable. Ratios are medians within matched blocks, not ratios of separate medians. Min–max values are observed ranges, not confidence intervals. RSS is the maximum GNU time observation, not simultaneous process-tree total memory.', '']
    return '\n'.join(lines), stream.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--outdir', type=Path, required=True)
    args = parser.parse_args()
    markdown, tsv = render(json.loads(args.report.read_text()))
    args.outdir.mkdir(parents=True, exist_ok=True)
    for filename in ('tables.md', 'tables.tsv'):
        if (args.outdir / filename).exists():
            parser.error('refusing to overwrite a generated table')
    (args.outdir / 'tables.md').write_text(markdown)
    (args.outdir / 'tables.tsv').write_text(tsv)


if __name__ == '__main__':
    main()
