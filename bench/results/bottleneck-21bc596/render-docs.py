#!/usr/bin/env python3
"""Render completed, audited results; does not modify repository documents."""
import json
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT/'target/bottleneck-21bc596'
sys.path.insert(0, str(ROOT/'bench'))
from scale_common import validate_complete
report = json.loads((WORK/'formal/report.json').read_text())
validate_complete(report)
audit = json.loads((WORK/'formal-audit.json').read_text())
assert audit['passed']
summary = json.loads((WORK/'formal-summary/summary.json').read_text())
archive = 'results/bottleneck-21bc596'

def seconds(data):
    return f"{data['median_s']:.2f} [{data['min_s']:.2f}–{data['max_s']:.2f}]"

def readme_table(zh=False):
    lines = ['| 操作 | 输入 | CPU 核心预算 | 参考软件耗时（秒） | 当前实现耗时（秒） | 参考软件 / 当前 |' if zh else
             '| Operation | Input | CPU Budget | Reference Time (s) | Current Implementation Time (s) | Reference / Current |',
             '| --- | --- | ---: | ---: | ---: | ---: |']
    for row in summary:
        real = row['condition'].startswith('real-')
        bam = row['case'] == 'bam_sort'
        label = 'samtools sort' if bam else 'SeqKit stats -a -T'
        population = ('真实' if real else '合成') if zh else ('Real' if real else 'Synthetic')
        population += f" {'BAM' if bam else 'FASTQ'}, {row['records']//1000000}M"
        lines.append(f"| {label} | {population} | {row['cores']} | {seconds(row['arms']['upstream'])} | {seconds(row['arms']['current'])} | {row['paired_upstream_over_current']['median']:.2f}× |")
    return '\n'.join(lines)

for zh, filename, heading, historical in [
    (False, 'README.md', '## Performance Comparison with Reference Software', '### Earlier Comparison: 2026-11-01'),
    (True, 'README.zh-CN.md', '## 和参考软件的性能对比', '### 此前测量：2026-11-01')]:
    text = (ROOT/filename).read_text()
    start = text.index(heading)
    end = text.index('\n## ', start+len(heading))
    old = text[start:end]
    if zh:
        intro = '''### 统计与 BAM 排序：2026-11-02

本轮优化统计计数和归并选择，沿用本机 NVMe、1/2/4 个物理 P 核预算和逐轮配对比较。
参考版本为 SeqKit 2.14.0 和 samtools 1.24。9 个条件完成 54 次正式观测、18 次预热和
36 对输出校验；最短正式耗时为 19.58 秒。表中为三轮耗时的中位数 [最小值–最大值]，
倍数为逐轮参考软件/当前实现耗时比的中位数，大于 1 表示当前实现更快。M 表示百万条记录。

<!-- README_BOTTLENECK_START -->
'''+readme_table(True)+'''
<!-- README_BOTTLENECK_END -->

统计仍串行处理单个文件；BAM 排序仍只并行压缩最终 BAM。临时文件并行压缩候选
未通过性能筛选，已撤回。这组输入中统计快于 SeqKit，BAM 排序仍慢于 samtools。
输入重复固定的百万条数据，增加处理量，序列种类保持不变。
[完整报告](bench/BOTTLENECK.md)包含资源数据、正确性证据、指令剖析和候选取舍。
下方保留此前的二进制和输入规模；两个实验的绝对耗时不能直接换算成前后加速比。

'''
    else:
        intro = '''### Statistics and BAM Sort: 2026-11-02

This follow-up optimizes byte counting and merge selection, using the same host,
NVMe storage, 1/2/4 physical P-core budgets and paired comparisons. References are
SeqKit 2.14.0 and samtools 1.24. All 9 conditions completed: 54 measured observations,
18 warm-ups and 36 output checks. The shortest measured invocation was 19.58 s.
Times are median [minimum–maximum] across three blocks. Ratios are medians of
within-block reference/current times; values above 1 mean the current implementation
is faster. M denotes million records.

<!-- README_BOTTLENECK_START -->
'''+readme_table()+'''
<!-- README_BOTTLENECK_END -->

Statistics still process one file serially; BAM sort parallelizes final BAM
compression only. The temporary-compression parallel candidate failed its timing
screen and was withdrawn. Statistics were faster than SeqKit on these inputs;
BAM sorting remained slower than samtools. Inputs repeat fixed million-record
populations, adding work while retaining their sequence diversity.
The [full report](bench/BOTTLENECK.md) includes resources, correctness evidence,
instruction profiles and candidate decisions. The earlier binaries and input sizes
remain below; absolute times from the two experiments do not establish a before/after
speed ratio.

'''
    new = heading+'\n\n'+intro+historical+'\n\n'+old[len(heading):].lstrip('\n')
    new = new.replace('### FastQC Runtime\n', '### FastQC Runtime: 2026-11-01\n').replace('### FastQC 耗时\n', '### FastQC 耗时：2026-11-01\n')
    (WORK/(filename+'.section')).write_text(new)

screens = []
for number, title in [(1, 'Baseline versus serial candidate'), (2, 'Serial candidate versus rejected parallel candidate')]:
    data = json.loads((WORK/f'screen-{number}/report.json').read_text())
    assert data['passed'] and len(data['observations']) == 32
    lines = [f'### {title}', '', '| Case | First arm median s | Second arm median s |', '| --- | ---: | ---: |']
    for case in dict.fromkeys(o['case'] for o in data['observations']):
        medians = [statistics.median(o['elapsed_seconds'] for o in data['observations'] if o['case'] == case and o['arm'] == arm and o['block'] >= 0) for arm in ['before','current']]
        lines.append(f'| {case} | {medians[0]:.6f} | {medians[1]:.6f} |')
    screens.append('\n'.join(lines))

samples = [sample for row in report['results'] for observation in row['observations'] for sample in observation['telemetry_samples']]
temperatures = [v for sample in samples for v in sample['temperature_c'].values()]
loads = [sample['load_average'][0] for sample in samples]
text = f'''# Statistics and BAM merge selection after 21bc596

The accepted changes replace repeated byte classification with packed integer
contributions and replace a heap of merge records with an index-only selection
tree. On the calibrated inputs, statistics ran faster than SeqKit; BAM sorting
remained slower than samtools. No new dependency or CLI option was introduced.
The parallel temporary-compression prototype was tested and withdrawn.

## Formal upstream/current comparison

The run completed on 2026-11-02, {audit['started_utc']} through
{audit['completed_utc']}. All 9 conditions, 54 measured observations, 18 warm-ups
and 36 paired output checks passed. The shortest measured invocation was
{audit['minimum_formal_seconds']:.6f} s; none was below 10 s. Ratios below are the
median of three within-block upstream/current ratios, not ratios of the displayed
medians. There is no suite-wide average.

{(WORK/'formal-summary/tables.md').read_text()}

The source populations are one million real 150 bp reads from ENA SRR2584863,
one million synthetic 150 bp reads and one million synthetic 50 bp BAM records.
Statistics repeat each FASTQ 120 times. Sort reuses and fully rehashes the earlier
86-fold BAM. Repetition adds work, not sequence diversity or biological replication.
All budgets use the same workload input. The previous [scale experiment](SCALE.md)
is separate evidence with different FASTQ sizes and native executables.

## Method and resources

The [scale protocol](SCALE-PROTOCOL.md) was used without measurement-script changes.
Calibration ran both arms in every condition and qualified all 18 invocations;
its minimum was 19.256966 s. The plan was then frozen. Seed 20261103 balances both
arm orders twice across one warm-up and three measured blocks. Builds, generation,
correctness suites and other benchmarks were outside accepted timing.

The host was an i7-13700H, 20 logical CPUs, {report['hardware']['memory_kb']/1024/1024:.2f} GiB RAM,
Linux 7.3.0-4-cachyos, with inputs/output/temp on NVMe-backed ext4
(`/dev/nvme0n1p2`, `rw,noatime`). Affinities were CPU4, CPU4/6 and CPU0/2/4/6,
representing 1/2/4 physical P cores. CPU4/6 have 5.0 GHz maximum frequency;
CPU0/2 have 4.8 GHz maxima. SMT siblings remained available to background work.
Frequency was unlocked. Sampled temperatures spanned {min(temperatures):.0f}–{max(temperatures):.0f} °C
and one-minute load {min(loads):.2f}–{max(loads):.2f}; these are sampled conditions, not controlled constants.

References were SeqKit 2.14.0 and samtools 1.24; native builds used Rust 1.99.1
with Cargo --locked. Native sort used a 512 MiB estimated chunk budget and final
compression workers 1/2/4. Samtools used `-@ 0/1/3` and `-m 512M/256M/128M`.
These options are not equal total-RSS caps or a pure thread-count scaling test.
Final compression level was 6. Exact commands, executable/runtime/source hashes,
input sizes, wall/user/system time and I/O counters are in the raw report.

{(WORK/'formal-summary/resources.md').read_text()}

Effective CPU use is (user + system time)/wall time. Statistics remain serial;
-j is accepted but unused. Native sort parallelizes only final BAM compression.
Its four-core observation used a median 1.26 CPUs and remained slower than the
reference. GNU time RSS is not a simultaneous descendant aggregate; approximately
one-second sampling can miss temporary-space or temperature peaks. Sequential
read-conditioning does not guarantee cache residency. CLI timing includes startup
and ordinary I/O; post-exit fsync is untimed and reported separately. Low CPU use
alone does not identify a particular SSD or cache mechanism.

## Complexity, implementation, then concurrency

For statistics, let B include all FASTX input bytes, N be records and D distinct
lengths. The bound remains O(B + N log D), with O(D + largest record/input buffer)
state plus constant tables. These equal-length populations have D=1. The observed
roughly 5 MiB RSS does not establish constant memory for arbitrary input.

`SequenceCounter` precomputes GC, AT, N-or-X and arbitrary gap membership in four
16-bit lanes. `QualityDecoder` packs Q20/Q30 contributions in two lanes. Chunks of
at most 65535 bytes prevent inter-lane carry, including overlapping categories and
all-set lanes. Quality error probabilities still add in byte order into one
accumulator across chunks, then merge in original record order. Floating-point
addition was not replaced with multiplication or histogram reduction.

For external sorting, chunk comparisons remain O(N log C), for C records per
chunk. Each merge level reads/decodes/compresses records and performs O(N log F)
key comparisons for fan-in F; names can add comparison cost. The number of merge
levels is unchanged. The new selection tree builds in O(k), replacing k heap
pushes, and repairs one head in O(log k). It makes at most one key comparison per
level and stores fewer than 4k indices for nonempty k. Record/key/frame handles
stay in fixed slots. The tree height remains based on original slots as runs end.

Full BAM decoding, original global indices, run-index tie fallback and sidecar EOF
semantics remain. Final merging refills before emitting the selected record;
intermediate merging writes its record/frame and index before refilling. The
64 KiB frame-capture threshold limits the added copy only; retained capacities,
reader/decoder buffers, headers and run heads still contribute to memory.

Callgrind on matching 100k inputs, CPU4, showed:

| Workload | Baseline instructions | Current instructions | Reduction |
| --- | ---: | ---: | ---: |
| Real stats -a -T | 563,983,731 | 153,092,893 | 72.86% |
| Synthetic stats -a -T | 560,938,463 | 150,047,438 | 73.25% |
| Sort, 1 MiB chunk, 1 worker | 2,103,324,121 | 1,976,122,580 | 6.05% |
| Flagstat, unchanged path | 164,865,213 | 164,865,740 | approximately unchanged |

The original stats callback accounted for 93.35% of instructions. Heap selection
and movement accounted for about 6.8% of the sort diagnostic instructions;
compression, full decoding, conversion and allocation remain substantial.
Inclusive call-graph percentages overlap. These are instruction counts under
instrumentation, not wall-time speedups. All profiled outputs matched. Final sealed
binaries are byte-identical to the profiled serial candidate; later changes added
tests only. Both candidate source snapshots were verified against their seal hashes.

Only after the serial changes was temporary-run parallel BGZF compression tested.
The prototype used the existing ordered noodles writer at level 1. Its first
build lacked a trait import; that failure is retained. The corrected prototype
passed component tests, clippy and every output comparison, but its 2/4-core 10M
screens were slower. It was reverted. Per-writer pools and queues are real costs;
the experiment did not isolate their individual contribution or establish a
universal conclusion about parallel compression.

## Supplementary candidate screens

Each screen has 32 invocations: four cases, one warm-up and three measured blocks
per arm, balanced orders with seed 20261102, fixed affinities, output hashes and
untimed fsync. Every output matched. These short screening runs are separate from
the calibrated comparison above. Timeout polling affects subsecond resolution;
the 1M stats and 100k sort timings are not formal speedup evidence.

{chr(10).join(screens)}

In screen 1 the first arm is 21bc596 and the second is the serial candidate.
In screen 2 the first arm is that serial candidate and the second is the rejected
temporary-compression prototype. Neither table compares against upstream.

## Correctness and evidence

Fresh commands returned:

```text
bash tests/ci/check.sh
QUALITY PASS (base only; external oracles NOT RUN)
bash tests/ci/check.sh --with-fastx-qc-oracles
QUALITY PASS (base + FASTX/QC oracles)
Optimization behavior: 104/104
50/50 cases passed
20/20 cases passed
PASS independent audit: 9 timing medians/ranges, 9 paired ratios, 72 invocations, 36 checks
PASS final identities: 152 files rehashed
```

New tests cover lane boundaries, all-byte gap membership, quality thresholds and
bitwise sums across chunks/records, selection ties and exhaustion/reactivation,
and small/over-capture-limit/small records through actual spill merges. Existing
malformed-field, index EOF, refill-order, long-CIGAR, auxiliary-field and output
failure tests remain in the full gates. The sealed executables also passed the
FX baseline behavior check and pinned SeqKit/fq and samtools oracles.

Formal statistics checks compare the complete declared table. BAM checks include
quickcheck, canonical EOF, all normalized fields/counts and ordering, excluding
only @PG and permitted equal-coordinate ordering. Native byte signatures match
across blocks and CPU budgets. No fixture, comparison harness, compatibility row,
protected UMI source or approved exception was changed. No new upstream correction
or broader compatibility claim is introduced.

The [archive]({archive}/README.md) contains raw observations, per-invocation logs,
executed helpers, input manifests, source snapshots/patch, build provenance,
validation, both screens and the rejected prototype. Large reproducible payloads
and executables stay outside Git; their hashes and recipes remain. The audit
checks the complete population and recomputes medians/ranges and paired ratios;
input/runtime/source identities passed, and final route sampling matches the
mid-run sample. SHA-256 coverage binds the archived bytes, not a release signature.

For a new run use [the reproduction guide](SCALE-REPRODUCTION.md), selecting
real-stats, synthetic-stats and bam-sort before preparation. This run started at
multiplicities 120/120/86 and qualified in one calibration round. Review hardware
affinities before calibration, use fresh destinations and retain all reused input
dependencies. Do not reuse the archived frozen plan as qualification for changed
binaries, scripts or hosts.

A follow-up static review identifies full-level temporary merging as a candidate
for reducing intermediate work near the fan-in boundary. Actual run counts and
the I/O-versus-comparison tradeoff still require measurement; it is not an
implemented improvement or an executed probe in this delivery.
'''
(WORK/'report.md').write_text(text)
print('Rendered audited report and two README performance sections')
