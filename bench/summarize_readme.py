#!/usr/bin/env python3
"""Render all recorded conditions without selecting only favorable results."""
import argparse
import json
from pathlib import Path

LABELS = {'cutadapt_plain':'cutadapt: plain output (1 CPU)', 'cutadapt_gzip':'cutadapt: gzip level 6 (1 CPU)',
          'cutadapt_4cpu':'cutadapt: plain output (4 CPUs)', 'fx_stats':'SeqKit stats -a -T',
          'fx_fq2fa':'SeqKit fq2fa', 'fq_lint':'fq lint', 'bam_flagstat':'samtools flagstat',
          'bam_sort':'samtools sort (32 MiB chunks)', 'umi_dedup':'UMI-tools dedup', 'fastqc':'FastQC'}


def table(rows):
    result = ['| Dataset | Command | Native s (min–max) | Upstream s (min–max) | Upstream/native ratio | Peak RSS MiB native/upstream |',
              '| --- | --- | ---: | ---: | ---: | ---: |']
    for row in rows:
        if not row.get('passed') or 'summary' not in row:
            result.append(f"| {row['dataset']} | {LABELS[row['case']]} | incomplete | incomplete | — | — |")
            continue
        a,b = row['summary']['rust'],row['summary']['upstream']
        ratio = f"{row['paired_speed_ratio']['median']:.2f}×" if 'paired_speed_ratio' in row else 'not comparable'
        result.append(f"| {row['dataset']} | {LABELS[row['case']]} | {a['median_s']:.3f} ({a['min_s']:.3f}–{a['max_s']:.3f}) | {b['median_s']:.3f} ({b['min_s']:.3f}–{b['max_s']:.3f}) | {ratio} | {a['max_rss_kib']/1024:.1f} / {b['max_rss_kib']/1024:.1f} |")
    return '\n'.join(result)


def validate_report(report):
    if not report.get('completed_utc'):
        raise ValueError('refusing to summarize an unfinished run')
    parameters = report.get('parameters', {})
    if report.get('kind') != 'readme-benchmark' or parameters.get('repeats') != 5 or parameters.get('warmups') != 1:
        raise ValueError('formal tables require five repeats and one warm-up; pilot results are separate')
    for row in report['results']:
        if row.get('passed'):
            if any(len(row['samples'][side]) != 5 for side in ('rust', 'upstream')) or len(row['warmups']) != 1:
                raise ValueError('passing condition has incomplete samples')
            if sorted(check.get('block', -99) for check in row['checks']) != [-1, 0, 1, 2, 3, 4] or not all(check['passed'] for check in row['checks']):
                raise ValueError('passing condition has incomplete or failing checks')


def qc_table(rows):
    lines = ['| Dataset | Modules raw-equal in every measured pair | Differing modules / missing comparison |',
             '| --- | ---: | --- |']
    for row in rows:
        if row['case'] != 'fastqc':
            continue
        checks = [c for c in row['checks'] if c.get('block', -1) >= 0 and 'module_equal' in c]
        if not checks:
            lines.append(f"| {row['dataset']} | — | no measured module comparison; inspect failure in JSON |")
            continue
        names = dict.fromkeys(name for check in checks for name in check['module_equal'])
        equal = {name:all(check['module_equal'].get(name, False) for check in checks) for name in names}
        lines.append(f"| {row['dataset']} | {sum(equal.values())}/{len(equal)} | {', '.join(name for name,value in equal.items() if not value)} |")
    return '\n'.join(lines) + '\n'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('results',nargs='+',type=Path)
    p.add_argument('--out',type=Path,required=True)
    args=p.parse_args()
    reports=[json.loads(path.read_text()) for path in args.results]
    try:
        for report in reports:
            validate_report(report)
    except ValueError as error:
        p.error(str(error))
    rows=[row for r in reports for row in r['results']]
    text='# Complete README benchmark tables\n\n'
    text+='Completed conditions use medians of five fresh-process executions after one warm-up; failed/incomplete conditions remain visible. Parentheses show observed min–max, not confidence intervals. Ratios are medians of paired upstream/native times; values below 1 mean native is slower. RSS is the maximum GNU time RSS across the five executions, not aggregate process-tree memory.\n\n'
    text+='## Same-output or validation-verdict workloads\n\n'+table([r for r in rows if r['case']!='fastqc'])+'\n\n'
    text+='## FastQC: descriptive timings, unequal report work\n\n'+table([r for r in rows if r['case']=='fastqc'])+'\n\n'
    text+='Independent checks here cover total reads, displayed total bases and length. Complete raw module differences are retained in JSON. These rows do not establish full QC equivalence.\n\n'
    text+=qc_table(rows)
    text+='\n## Throughput, CPU, temporary storage and payload size\n\n'
    text+='Each pair of values is native / upstream. Throughput is input records divided by median elapsed time. CPU is median user+system seconds. Temporary storage is the maximum sampled value; short-lived files can be missed. Payload is the measured output file size range, so QC means the data table, not its whole report bundle; raw JSON also records bundle output bytes.\n\n'
    text+='| Dataset | Command | Million records/s | CPU s | Sampled temp MiB | Payload bytes (min–max) |\n| --- | --- | ---: | ---: | ---: | ---: |\n'
    for row in rows:
        if not row.get('passed'):
            continue
        a,b=row['summary']['rust'],row['summary']['upstream']
        def size_span(side):
            sizes=[n for n in side['payload_bytes'] if n is not None]
            return f'{min(sizes)}–{max(sizes)}' if sizes else 'not recorded'
        text+=f"| {row['dataset']} | {LABELS[row['case']]} | {a['records_per_s']/1e6:.3f} / {b['records_per_s']/1e6:.3f} | {a['cpu_median_s']:.3f} / {b['cpu_median_s']:.3f} | {a['temp_peak_bytes_sampled']/2**20:.3f} / {b['temp_peak_bytes_sampled']/2**20:.3f} | {size_span(a)} / {size_span(b)} |\n"
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(text)
    # Keep a simple numeric table for independent plotting or downstream analysis.
    lines=['dataset\tcase\tnative_median_s\tupstream_median_s\tpaired_ratio_median\tnative_max_rss_kib\tupstream_max_rss_kib\tpassed']
    for row in rows:
        summary=row.get('summary',{}); a=summary.get('rust',{});b=summary.get('upstream',{})
        lines.append('\t'.join(str(x) for x in [row['dataset'],row['case'],a.get('median_s',''),b.get('median_s',''),row.get('paired_speed_ratio',{}).get('median',''),a.get('max_rss_kib',''),b.get('max_rss_kib',''),row.get('passed',False)]))
    args.out.with_suffix('.tsv').write_text('\n'.join(lines)+'\n')
    print(f'wrote {len(rows)} conditions to {args.out}')


if __name__=='__main__':
    main()
