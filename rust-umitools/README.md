# rust-umitools

**简体中文** | [English](README.en.md)

**UMI-tools 的 Rust 高性能实现。** 在已验证的输入与参数组合下，解压后的 BAM、TSV 与 FASTQ 输出与 UMI-tools 1.1.6 **逐字节一致**；在 4,428,466 reads 的真实单端 BAM 上快 **15.5 倍**，峰值内存更低。大规模数据可走有界内存路径，结果与内存路径一致。

## 为什么可信

- **逐字节对照**：`run_exact.sh` 有 **77 个用例**（5 种归并方法 × 单端/双端 × read_id/tag × 排序开关、双端重复 pair、MAPQ、NH/X0/XT、spliced、read-length、ignore-tlen、UMI 长度 1–40、阈值 0/1/2、长 soft clip/intron、大 UG、已有 UG/BX、per-gene/per-cell、subset、`--chrom`、`count --wide`），解压后的头部与记录流完全一致。
- **语义对照**：`run_oracle.sh` 39 项；另有随机差分（生成器 + 暴力参考 + 上游）、确定性套件（线程 × spill × 多轮归并）、格式/管道套件与基准 smoke。
- **随机性对齐**：复现 CPython MT19937（含多重比对替换后的那次 RNG 消耗），固定 `--random-seed` 时代表 read 与上游一致。
- **单一核心**：dedup/group/count 共用同一套 UMI 编码、邻居查询、聚类与磁盘后备实现。

## 安装

### 预编译二进制（Linux x86_64，公开下载）

公开分发仓库：[seek-hope/bioinfo-tools-dist](https://github.com/seek-hope/bioinfo-tools-dist)。每个 release 同时包含 `rust-umitools` 与 `rust-cutadapt`：

```bash
curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash -s rust-umitools
```

脚本下载最新 release、校验 SHA256，并安装到 `~/.local/bin`。也可以指定版本/目录：

```bash
INSTALL_DIR=/opt/bin BIOINFO_VERSION=v0.2.0 bash install.sh rust-umitools
```

手动下载：

```bash
base=https://github.com/seek-hope/bioinfo-tools-dist/releases/latest/download
curl -fLO "$base/rust-umitools-x86_64-unknown-linux-gnu.tar.gz"
curl -fLO "$base/rust-umitools-x86_64-unknown-linux-gnu.tar.gz.sha256"
sha256sum -c rust-umitools-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf rust-umitools-x86_64-unknown-linux-gnu.tar.gz
install -m 755 rust-umitools-x86_64-unknown-linux-gnu/rust-umitools ~/.local/bin/
```

预编译包面向 Linux x86_64（glibc）。当前不提供 aarch64 预编译包。

### 从源码安装

源码仓库是私有的：

```bash
cargo install --git https://github.com/seek-hope/bioinfo-tools --package rust-umitools --locked
```

需要 Rust 1.88+；本地 checkout 开发仍推荐：

```bash
cargo build --release      # 产物 target/release/rust-umitools
cargo install --path .     # 或安装到 ~/.cargo/bin
```

## 快速上手（5 分钟）

```bash
# 去重（默认坐标排序）
rust-umitools dedup -I sorted.bam -S deduped.bam
# 双端
rust-umitools dedup -I sorted.bam -S deduped.bam --paired
# 分组 / 计数
rust-umitools group -I sorted.bam   -S grouped.bam --output-bam --group-out groups.tsv
rust-umitools count -I assigned.bam -S counts.tsv  --per-gene --gene-tag GX
# 单细胞
rust-umitools dedup -I in.bam -S out.bam --per-gene --gene-tag GX --per-cell --cell-tag CB
rust-umitools count -I in.bam -S c.tsv --per-gene --gene-tag GX --per-cell --cell-tag CB --wide
# FASTQ
rust-umitools extract   -I reads.fastq.gz -S out.fastq.gz -p NNNN
rust-umitools whitelist -I reads.fastq.gz -S whitelist.tsv -p NNNNCCCC
# CRAM 与管道
rust-umitools dedup -I in.cram -S out.bam --reference genome.fa
cat in.bam | rust-umitools dedup -I - -S - | samtools view -c -
```

每个子命令的 `--help` 都带可复制的示例与默认值说明。

## 从 umi_tools 迁移

| umi_tools | rust-umitools |
|---|---|
| `umi_tools dedup -I in.bam -S out.bam` | 同名参数 |
| `--paired`、`--extract-umi-method=tag --umi-tag=RX` | 同名参数 |
| `--per-gene --gene-tag GX`、`--per-cell --cell-tag CB` | 同名参数（per-cell 需 `--per-gene`） |
| `umi_tools group --group-out g.tsv` | 同名参数；写 BAM 需 `--output-bam -S out.bam` |
| `umi_tools count --per-gene --gene-tag GX [--per-cell] [--wide]` | 同名参数 |
| `umi_tools count_tab -I r.tsv -S c.tsv [--per-cell]` | 同名参数 |
| `umi_tools extract -I r.fastq -S o.fastq -p NNNN` | 同名参数（string 与 regex 子集） |
| `umi_tools whitelist -I r.fastq -S wl.tsv -p NNNNCCCC` | 同名参数（distance/density knee、纠错映射） |

默认值与上游一致：`--method=directional`、`--edit-distance-threshold=1`、`--umi-separator=_`、`--soft-clip-threshold=4`、unpaired/chimeric 策略 `use`、unmapped `discard`。

## 性能（同机、同输入、同参数）

> 下表是 4,428,466 reads 的 `small.bam` 历史基线；后续 mimalloc、默认压缩级别等优化没有重测该输入（原始文件不在仓库），因此不要把下表当成当前构建的最新数字。当前合成/真实基准与命令见 [docs/BENCHMARK.md](docs/BENCHMARK.md) 和 `docs/bench/*.json`。

| 工具 | wall time | 峰值 RSS |
|---|---:|---:|
| UMI-tools 1.1.6 | 143.08 s | 1.10 GiB |
| **rust-umitools** | **9.24 s** | **0.78 GiB** |
| umi-tools-rs 2.0.1 | 73.29 s | 0.94 GiB |

- 历史基线相对 UMI-tools：**15.49×**；相对 umi-tools-rs：**7.93×**；三者分子数一致（3,718,913）。
- 邻居策略阈值、排序策略与有界内存路径的测量见 [docs/BENCHMARK.md](docs/BENCHMARK.md)。

## 有界内存

- bundle 窗口在位置边界提前结束；mate 请求超限时落盘并按块重扫；gene 聚合超预算时按 gene 哈希分区；单个超大 bundle 用分块精确邻居查询 + 磁盘 pair 索引/CSR。
- 上述路径均与内存路径**逐字节一致**，由 `run_spill.sh`（K9）强制触发并对比。
- `--memory-limit` 最低 64 MiB；`--sort-strategy auto|low-memory|high-throughput` 调整缓冲策略，输出不变。

## 诊断与自动化

| 选项 | 用途 |
|---|---|
| `--quiet` | 只保留错误与警告，适合 CI |
| `--progress auto\|on\|off` | 进度输出；默认仅在终端显示 |
| `--summary-json FILE` | 原子写入机器可读摘要（计数、资源、阶段耗时） |
| `--version --verbose` | 参考版本、压缩后端、命令与退出码 |
| `--sort-strategy …` | 排序/批处理预算策略；解压后输出不变 |
| `--subset P` | 以概率 P 抽样（与平局共享 RNG，和上游一致） |
| `--chrom NAME` | 只处理指定 contig（写前校验名称） |
| `--reference FASTA` | CRAM 读写参考 |
| `--partition-genes N` | count 的 gene 分区后备（0=自动） |

仓库提供 `.github/workflows/ci.yml`（x86_64 fmt/clippy/test、MSRV 1.88）与 `Containerfile`。

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 2 | 用法错误，或该能力未实现（写前拒绝） |
| 3 | 输入不满足前提（UMI 长度不一致、未排序等） |
| 4 | I/O 或资源错误 |

## 已知限制

- 默认使用 `mimalloc` 全局分配器（启动时 `purge_delay=0`）；count 使用紧凑 `CountBundle`，高 UMI 输入的峰值 RSS 从约 444 MiB 降到约 255 MiB（早期排序版曾到 203 MiB，最终采用保持插入顺序的版本）。
- 默认 `--compresslevel 1`（更快，体积略大）；需要更小输出时用 `--compresslevel 6`（接近上游默认）。
- `--output-stats`：null model 未实现，参数验证阶段拒绝（exit 2，不产生输出）。
- `--gene-transcript-map` 仅在 `count --per-contig` 下支持；`dedup` 没有 `--per-contig`，该组合仍在写前拒绝。
- `--memory-limit` 先预留 8–32 MiB 给 HTSlib、分配器与线程栈，剩余部分才是 bundle/排序预算；实测 64/128/256 MiB 预算对应峰值 RSS 57/103/187 MiB，仍不是硬性进程上限。
- dedup 默认使用可用内存的一半；高重复 2M-read 输入实测 auto 约 2.47 GiB。需要更低 RSS 时用 `--memory-limit 1GiB`（实测约 1.30 GiB，wall 约 +17%）；`--sort-strategy low-memory` 不改变该峰值。
- 单个超大 position 或 gene 若超过扣除预留后的预算，会在写输出前 exit 4，并提示提高 `--memory-limit` 或拆分输入。
- CRAM 解码可能由 noodles/htslib 设置补出派生 `MD/NM` 标签，故 BAM↔CRAM 按核心字段比较。
- 压缩后的 BAM 字节可能与上游不同（压缩级别/BGZF 布局）；解压后的内容逐字节一致。
- 上游未固定 `--random-seed` 时使用 `os.urandom`，不可复现；本实现默认 seed=0。

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --release
bash tests/oracle/run_oracle.sh      # 39 项语义对照
bash tests/oracle/run_exact.sh       # 77 项逐字节对照
bash tests/ci/check_manifest.sh      # 兼容矩阵同步
bash tests/oracle/run_determinism.sh # 线程 × spill × 多轮归并
bash tests/oracle/run_spill.sh       # 有界内存路径等价
bash tests/oracle/run_extract.sh     # extract + whitelist
bash tests/oracle/run_formats.sh     # stdin/stdout、CRAM、摘要
bash tests/bench/run_bench_smoke.sh  # 基准 smoke
```

兼容性细节见 [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md)；迭代计划与完成状态见 [docs/ROADMAP.md](docs/ROADMAP.md)。
