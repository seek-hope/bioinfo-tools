#!/usr/bin/env python3
"""Prepare only the performance sections after complete formal acceptance.

Preserves the Argon High README structure and prose outside that section.
Writes review candidates under target; never edits the published READMEs.
"""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'bench'))
from scale_common import validate_complete
from summarize_scale import summarize, time_cell, LABELS

WORK = ROOT/'target/scale-benchmark'
report = json.loads((WORK/'formal/report.json').read_text())
validate_complete(report)
rows = summarize(report)
labels_zh = {'cutadapt_plain':'接头修剪，FASTQ', 'cutadapt_gzip':'接头修剪，gzip 级别 6',
             'fx_stats':'SeqKit stats -a -T', 'fx_fq2fa':'SeqKit fq2fa', 'fq_lint':'fq lint',
             'bam_flagstat':'samtools flagstat', 'bam_sort':'samtools sort',
             'umi_dedup':'UMI-tools directional dedup', 'fastqc':'质控报告'}


def table(selected, zh=False, qc=False):
    columns = (['操作','输入','CPU 核心预算','参考软件耗时（秒）','当前实现耗时（秒）'] if zh else
               ['Operation','Input','CPU Budget','Reference Time (s)','Current Implementation Time (s)'])
    if not qc:
        columns.append('参考软件 / 当前' if zh else 'Reference / Current')
    lines = ['| '+' | '.join(columns)+' |', '| '+' | '.join(['---','---']+['---:']*(len(columns)-2))+' |']
    for row in selected:
        real = row['dataset'].startswith('real-')
        bam = 'bam' in row['dataset']
        population = ('真实 FASTQ' if real else '合成 BAM' if bam else '合成 FASTQ') if zh else ('Real FASTQ' if real else 'Synthetic BAM' if bam else 'Synthetic FASTQ')
        size = f"{row['records']/1e6:g}M"
        cells = [(labels_zh if zh else LABELS)[row['case']], f'{population}, {size}',
                 str(row['cores']), time_cell(row['arms']['upstream']), time_cell(row['arms']['current'])]
        if not qc:
            cells.append(f"{row['paired_upstream_over_current']['median']:.2f}×")
        lines.append('| '+' | '.join(cells)+' |')
    return '\n'.join(lines)


short = sum(a['observations_below_10s'] for r in rows for a in r['arms'].values())
dates = report['started_utc'][:10] + ('–'+report['completed_utc'][:10] if report['completed_utc'][:10]!=report['started_utc'][:10] else '')
comparable = [r for r in rows if r['case']!='fastqc']
qc = [r for r in rows if r['case']=='fastqc']
short_en = f' The results include {short} measured observations shorter than 10 seconds.' if short else ''
short_zh = f'其中 {short} 次正式观测短于 10 秒，已计入结果。' if short else ''
english = f'''## Performance Comparison with Reference Software

Measured on {dates} (UTC) on an i7-13700H laptop with 31.1 GiB RAM and NVMe storage.
The table shows the median [minimum–maximum] of three runs after a warm-up.
Ratios are the median of the paired reference/current times; values above 1
mean the current implementation is faster.{short_en}

Inputs repeat fixed one-million-record populations: real 150 bp reads from
SRR2584863, synthetic 150 bp FASTQ, and synthetic 50 bp BAM. M denotes million
records. Repetition increases processing work while retaining the source
sequence diversity. The same input is used for each core budget.

<!-- README_SCALE_START -->
{table(comparable)}
<!-- README_SCALE_END -->

Core budgets allocate 1, 2 or 4 physical P cores. Current FX and flagstat
processing are serial; BAM sort workers compress the final BAM output.
The [full results](bench/SCALE.md) include input bytes, commands, CPU usage,
peak RSS, temporary storage and output checks.

### FastQC Runtime

FastQC timings are listed separately while GC and duplication statistics are
compared and plots are added. The current implementation processes one input
serially.

{table(qc,qc=True)}

See [bench/README.md](bench/README.md) for the measurement methods.
Earlier measurements remain available in [bench/RESULTS-README.md](bench/RESULTS-README.md)
and [bench/OPTIMIZATION.md](bench/OPTIMIZATION.md).
'''
chinese = f'''## 和参考软件的性能对比

测量于 {dates}（UTC），使用 i7-13700H 笔记本、31.1 GiB 内存和 NVMe 存储。
每个条件预热一次，再测量三轮；表中耗时为中位数 [最小值–最大值]。
倍数取逐轮“参考软件耗时 / 当前实现耗时”的中位数，大于 1 表示当前实现更快。
{short_zh}

输入由固定的百万条数据重复生成：SRR2584863 的真实 150 bp reads、合成 150 bp FASTQ
和合成 50 bp BAM。M 表示百万条记录。重复增加处理量，序列种类保持不变。
同一工作负载在各核心预算下使用相同输入。

<!-- README_SCALE_START -->
{table(comparable,zh=True)}
<!-- README_SCALE_END -->

核心预算分配 1、2 或 4 个物理 P 核。当前 FX 和 flagstat 串行处理数据，BAM sort
通过工作线程压缩最终 BAM 输出。[完整结果](bench/SCALE.md)还列出输入字节数、命令、
CPU 使用量、峰值 RSS、临时空间和输出校验结果。

### FastQC 耗时

FastQC 耗时单独列出；GC、重复度统计的核对和图形报告开发继续进行。
当前实现串行处理单个输入。

{table(qc,zh=True,qc=True)}

测试方法见 [bench/README.md](bench/README.md)。历史测量见
[bench/RESULTS-README.md](bench/RESULTS-README.md) 和
[bench/OPTIMIZATION.md](bench/OPTIMIZATION.md)。
'''
for name, language, section, begin, end in (
    ('README.md','en',english,'## Performance Comparison with Reference Software','## Development Roadmap'),
    ('README.zh-CN.md','zh',chinese,'## 和参考软件的性能对比','## 后续开发路线图'),
):
    current = (ROOT/name).read_text()
    assert current.count(begin)==1 and current.count(end)==1
    start, stop = current.index(begin), current.index(end)
    assert start<stop
    candidate = current[:start]+section+'\n'+current[stop:]
    (WORK/f'readme-section.{language}.md').write_text(section)
    (WORK/name).write_text(candidate)
print('Prepared performance sections and README candidates from the complete accepted report')
