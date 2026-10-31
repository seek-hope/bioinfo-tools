# bioinfo-tools

[English](README.md) | [简体中文](README.zh-CN.md)

用于测序数据预处理、序列文件操作和质量控制的原生 Rust 命令行工具集。
仓库包含五个 CLI 和两个共享库，并用固定版本的上游工具验证行为。
不同命令的性能表现各有差异；[本机基准](bench/RESULTS-README.md)同时展示更快和更慢的结果。

## 工具及其在分析流程中的作用

FASTQ 保存读段及其质量分数，FASTA 保存不含质量分数的序列，SAM/BAM 保存比对结果。
UMI 是用于识别原始分子的序列标签，可帮助区分原始分子与扩增产生的重复 reads。

常见流程是检查 reads、按文库设计提取 UMI、修剪接头，再用**外部比对器**产生比对结果，
随后排序、建立索引、统计或按 UMI 去重。本仓库提供其中的数据处理步骤，不包含比对器。

| CLI | 主要用途 | 兼容目标 |
| --- | --- | --- |
| `rust-cutadapt` | 接头修剪、质量与长度过滤、双端处理、拆样 | [cutadapt](https://cutadapt.readthedocs.io/) 4.6 |
| `rust-umitools` | UMI 提取、白名单、去重、分组与计数 | [UMI-tools](https://umi-tools.readthedocs.io/) 1.1.6 |
| `rust-bam` | SAM/BAM 转换、坐标排序、索引、flag 统计及部分深度/覆盖操作 | [samtools](https://www.htslib.org/) 1.24 |
| `rust-fastqc` | 十二个分析模块，可配置阈值、接头和污染物，生成数据与摘要报告 | [FastQC](https://www.bioinformatics.babraham.ac.uk/projects/fastqc/) 0.13.0 |
| `rust-fx` | FASTA/FASTQ 统计、转换、筛选、序列变换与校验 | [SeqKit](https://bioinf.shenwei.me/seqkit/) 2.14.0 / [fq](https://github.com/stjude-rust-labs/fq) 0.12.0 |

`bioinfo-fastx` 提供共享序列读写与压缩支持，`bioinfo-common` 提供诊断、退出码辅助逻辑
及来源信息。原生命令运行时不调用上游 Python 或 Java 程序；开发与对照测试会使用这些参考程序。

## 构建与安装

从源码构建全部五个 CLI：

```bash
git clone https://github.com/seek-hope/bioinfo-tools.git
cd bioinfo-tools
cargo build --release --workspace --bins --locked
export PATH="$PWD/target/release:$PATH"
```

工作区声明的最低 Rust 版本为 **1.91**，`rust-toolchain.toml` 固定开发版本为 **1.99.1**。
编译压缩依赖还需要 C 构建工具链。依赖由 `Cargo.lock` 固定；正常构建可以下载缺失的包。

[二进制分发仓库](https://github.com/seek-hope/bioinfo-tools-dist)和[安装说明](dist/README.md)
提供 `rust-cutadapt` 与 `rust-umitools` 的预编译版本。当前发布流程只打包这两个工具，
其余三个请从源码构建。已发布二进制可能早于本文描述的修改和测量版本。

## 快速上手

以下示例使用你自己的输入文件。替换现有流程步骤前，请查看对应命令的 `--help`
及[兼容矩阵](compat/README.md)。

### 检查、校验与转换 reads

```bash
rust-fx stats -a -T reads.fastq.gz
rust-fx lint reads.fastq.gz
rust-fx fq2fa -w 0 reads.fastq.gz -o reads.fasta
```

### 修剪接头并生成 QC 报告

请按文库选择接头序列；这里的序列只是示例。

```bash
rust-cutadapt -a AGATCGGAAGAGC -m 30 -j 4 \
  -o trimmed.fastq.gz reads.fastq.gz
mkdir -p qc
rust-fastqc --extract -o qc trimmed.fastq.gz
```

FastQC 输出包含 `fastqc_data.txt`、`summary.txt`、HTML 和 ZIP 工件。
当前原生 HTML 展示数据表，完整图形资源仍有兼容缺口；可选模块是否出现取决于输入与配置。

### 提取 UMI，再处理比对结果

下面从 read 起始处提取八个碱基作为 UMI，并将其附加到 read 名。
请根据实际文库设计修改 barcode pattern。

```bash
rust-umitools extract -p NNNNNNNN -I reads.fastq.gz -S umi.fastq.gz
rust-cutadapt -a AGATCGGAAGAGC -m 30 -j 4 \
  -o umi.trimmed.fastq.gz umi.fastq.gz
```

使用能够保留这些 read 名的外部比对器处理 `umi.trimmed.fastq.gz`，生成 `aligned.bam`，然后执行：

```bash
rust-bam sort -o sorted.bam aligned.bam
rust-bam index sorted.bam
rust-bam flagstat sorted.bam
rust-umitools dedup --random-seed 1 --compresslevel 6 \
  -I sorted.bam -S dedup.bam
```

这里的去重命令从 read 名读取 UMI。对于 UMI 已保存在 `UB` 标签中的 BAM，
应改用 `--extract-umi-method=tag --umi-tag=UB`。
仓库另提供[可执行的小型 UMI 工作流](examples/umi-workflow/README.md)。

## 兼容性与当前边界

支持范围按版本和命令登记。[矩阵](compat/README.md)目前有 266 行：
**189 verified、9 experimental、57 unsupported、11 exception**。
各行代表的行为范围不同，不把这些数量换算为完成百分比。

- `verified` 表示所列输入和比较规则已有可执行证据。
- `experimental` 表示已有实现但仍存在缺口；`unsupported` 表示能力缺失或明确拒绝。
- `exception` 表示有规则和证明的已审查行为差异。独立的 `unachievable` 列目前仅为
  两个因原生运行策略而排除的 JVM 启动选项填写原因。

已知边界包括 samtools/SeqKit 命令覆盖不完整、`rust-bam` 不支持 CRAM、
`rust-bam sort` 为单线程、`rust-fx -j` 目前接受参数但不执行并行处理。
FastQC 的 HTML/ZIP 资源也尚未完整；本次 150 bp 基准还暴露了未解决的 GC 分布和
重复度估计差异，因此**不能认定其完整报告与上游等价**。

部分上游错误已由独立证据证明并在原生实现中修正，详见[证明与登记例外](docs/EVIDENCE-UPSTREAM-DEFECTS.md)。
新发现的差异需要先调查，不能直接套用该策略。流式读取也不等于总内存固定：
QC 会保存按 read 位置、tile 和不同序列累积的状态，排序与分组另有内存和临时空间需求。

## 实测性能

### e125519 之后的优化

后续测试使用**原始被测二进制**与优化版，在相同的 SRR2584863/合成输入（FASTQ 为 150 bp）上比较，
绑定一个 P 核的逻辑 CPU，每个条件测量五轮。
通过验收的结果覆盖 13 个条件、195 次计时执行。下表耗时为中位数，倍数为逐轮
**原生旧版耗时/新版耗时**的中位数。

<!-- README_OPTIMIZATION_START -->
| 操作与输入 | 优化前秒 | 优化后秒 | 上游秒 | 配对旧版/新版 |
| --- | ---: | ---: | ---: | ---: |
| stats -a -T，100 万真实 reads | 1.972 | 0.621 | 0.501 | 3.17× |
| fq2fa，1000 万合成 reads | 1.843 | 1.470 | 1.197 | 1.25× |
| lint，1000 万合成 reads | 5.614 | 2.140 | 1.789 | 2.62× |
| flagstat，1000 万合成 BAM 记录 | 0.815 | 0.703 | 0.319 | 1.16× |
| sort（32 MiB 分块），100 万合成 BAM 记录 | 1.406 | 1.246 | 0.533 | 1.13× |
| FastQC，100 万真实 reads* | 8.025 | 4.479 | 6.171 | 1.79× |
<!-- README_OPTIMIZATION_END -->

1000 万条单文件 lint 的峰值 RSS 从 **829.0 MiB 降至 4.9 MiB**，原因是移除了未被使用的名称集合。
配对 lint 仍需保存 read-one 的名称。若干优化后的命令仍慢于上游。
*FastQC 的原生报告、HTML、ZIP 全部保持逐字节一致；由于报告工作量及未解决的模块差异，
上游耗时仍只作描述。全部条件、观测范围、正确性检查、资源不足后的重测与复现方法见
[优化报告](bench/OPTIMIZATION.md)。输入、输出和临时文件仍位于 tmpfs。

### 优化前的上游比较

以下测量对应**本轮优化之前**的构建，原始观测保持不变。

<!-- README_BENCHMARK_START -->
测量日期 **2026-10-31**：Linux、i7-13700H、31.1 GiB 可见内存，输入、输出和临时文件
均放在 **tmpfs**。命令及其子进程绑定一个 P 核的逻辑 CPU；只有注明的四 CPU 案例使用
四个不同 P 核。每个条件进行一次预热、五次独立进程配对重复。本机为共享笔记本，
未锁定频率，记录到的 CPU package 温度最高为 100°C。
被测 `rust-fx` 在工作区构建后进行了单 crate 的 release 重建，
[构建记录](bench/RESULTS-README.md#source-failed-pilot-and-review)标识了准确的被测工件。

前六行使用 SRR2584863 的 **100 万条真实 150 bp reads**，后三行使用 **100 万条合成 BAM 记录**。
耗时为中位数；比值为每次配对的“上游耗时 / 原生耗时”的中位数，
**大于 1 表示原生更快，小于 1 表示原生更慢**。
RSS 取五次 GNU time 观测的最大值，不是整个进程树的总内存。

| 操作 | 原生秒 | 上游秒 | 配对比值 | 峰值 RSS MiB 原生/上游 |
| --- | ---: | ---: | ---: | ---: |
| cutadapt，未压缩，1 CPU | 2.872 | 1.840 | 0.64× | 9.7 / 18.7 |
| cutadapt，未压缩，4 CPU | 0.779 | 0.772 | 0.99× | 13.2 / 36.3 |
| cutadapt，gzip 级别 6 | 9.438 | 8.573 | 0.91× | 13.0 / 19.0 |
| SeqKit stats -a -T | 2.225 | 0.566 | 0.25× | 5.6 / 36.6 |
| SeqKit fq2fa | 0.212 | 0.270 | 1.29× | 5.4 / 34.3 |
| fq lint | 0.581 | 0.201 | 0.34× | 150.2 / 4.4 |
| samtools flagstat | 0.093 | 0.038 | 0.41× | 4.2 / 4.2 |
| samtools sort（32 MiB 分块） | 1.701 | 0.614 | 0.36× | 43.5 / 54.8 |
| UMI-tools directional dedup | 0.453 | 5.116 | 11.20× | 256.3 / 326.8 |

按方案追加的 **1000 万条合成记录**用于检查耗时较短的流式案例：

| 操作 | 原生秒 | 上游秒 | 配对比值 | 峰值 RSS MiB 原生/上游 |
| --- | ---: | ---: | ---: | ---: |
| SeqKit fq2fa | 1.980 | 1.289 | 0.65× | 5.4 / 34.7 |
| fq lint | 6.467 | 1.959 | 0.30× | 829.4 / 4.4 |
| samtools flagstat | 0.960 | 0.374 | 0.39× | 4.1 / 4.1 |

UMI 的优势限于这个每个坐标 bundle 含四类 UMI 的合成场景。多项原生命令较慢，
此处 lint 的内存占用也明显更高。耗时包含启动、压缩和文件读写，不是单独的算法加速测量。
全部 10 万/100 万合成案例及观测 min–max 范围均保存在完整报告中。

**FastQC 的报告生成工作量不同，且 GC/重复度差异尚未解决**，因此下表只描述耗时，不计算加速比。
基准中的独立检查覆盖 read 数、显示的总 bases 和长度，不代表完整 QC 等价。

| FastQC 输入 | 原生秒 | 上游秒 | 峰值 RSS MiB 原生/上游 |
| --- | ---: | ---: | ---: |
| 100 万真实 reads | 9.306 | 6.197 | 92.1 / 322.9 |
| 100 万合成 reads | 9.290 | 6.188 | 110.0 / 298.4 |
<!-- README_BENCHMARK_END -->

完整命令、逐次观测、输入哈希、资源测量和复现步骤见[基准报告](bench/RESULTS-README.md)
与[基准说明](bench/README.md)。FastQC 因报告生成工作量及部分模块数值不同而单独列出。
结果仅适用于被测输入、二进制和机器，不能据此推导整套工具的通用加速倍数。

## 路线图

近期优先补全并验证现有工具：剩余命令、参数和格式覆盖，FastQC 完整图形与资源，
以及本次基准暴露的 GC/重复度差异。性能优化以 profiling 和同输入正确性检查为依据。
更多 CLI 的分发需要先完成验证与发布审查。

未来候选方向包括基因组区间操作（BED/GFF、区间交集等）、feature counting 与表达数据处理，
以及 VCF/BCF 查询、过滤和统计。这些方向目前是候选，没有承诺实现日期。
范围和优先级见[开发计划](docs/DEVELOPMENT-PLAN.md)。

## 测试与贡献

```bash
bash tests/ci/check.sh
# 安装固定版本的外部参考工具后：
bash tests/ci/check.sh --with-fastx-qc-oracles
```

基础门禁检查文档、兼容策略、格式、lint、release 测试/构建和 cutadapt 冒烟用例。
可选门禁增加固定版本的 FASTX/QC 对照，但不等于重新执行全部历史组件 oracle。
详见[CI 配置与覆盖](tests/ci/README.md)。提交贡献时请提供可复现用例并遵守[工作区规则](AGENTS.md)。

本 monorepo 是统一源码目录，不需要旁边的其他仓库 checkout。
[HISTORY.md](HISTORY.md)记录了原始仓库与归档关系。
历史 `docs/EVIDENCE-*.md` 描述特定构建，不能替代当前功能或性能证据。
