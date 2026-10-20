# 性能基准

本文件只记录**同机、同输入、同参数**的测量。历史作业中 27~81 小时的 UMI-tools 数值来自其他硬件（Xeon E5-2690 v4）且包含统计与最终排序，**不可**与本文件直接比较，也不作为加速倍数来源。

## 方法

- 机器：i7-13700H（20 逻辑核 / 14 物理核），31 GiB 内存
- 输入：`small.bam`，4,428,466 reads，坐标排序，单端，read name 末段为 8 nt UMI
- 参数：`--method=directional`（默认）、`--edit-distance-threshold=1`、`--random-seed 1`
- 计时：`hyperfine --warmup 0 --min-runs 3`
- 内存：`/usr/bin/time -v` 的 Maximum resident set size（单独运行一次）
- 结果校验：三方输出分子数与记录流摘要

对照组：

- UMI-tools 1.1.6（Python 3.12.14，本机 conda 环境）
- umi-tools-rs 2.0.1（官方 v2.0.1 release 二进制，commit `010d3ee`）
  - 说明：其源码在本机无法编译（安装 Rust 1.99.0 后 `rust-htslib 1.0.1` 仍报 11 个编译错误，`E0609` unknown field `_address`），因此使用官方预编译 release 做同机对比。

## 全量结果（4,428,466 reads）

| 工具 | Mean ± σ | Min … Max | 相对 | 峰值 RSS |
|---|---:|---:|---:|---:|
| UMI-tools 1.1.6 | 143.083 ± 1.328 s | 142.018 … 144.571 s | 15.49× | 1.10 GiB |
| **rust-umitools** | **9.237 ± 0.549 s** | **8.869 … 9.868 s** | **1.00** | **0.78 GiB** |
| umi-tools-rs 2.0.1 | 73.287 ± 0.939 s | 72.625 … 74.361 s | 7.93× | 0.94 GiB |

- 相对 UMI-tools：**15.49 ± 0.93 倍**
- 相对 umi-tools-rs：**7.93 ± 0.48 倍**
- 三方输出分子数均为 **3,718,913**
- rust-umitools 的输出与 UMI-tools 在解压后的头部与记录流上逐字节一致

### 分阶段耗时（rust-umitools）

| 阶段 | 耗时 | 占比 |
|---|---:|---:|
| 解析 + 分组 + 聚类 | 7.1 s | 78% |
| 排序 + 提交（4 个 run + 归并） | 2.0 s | 22% |
| 合计 | 9.1 s | 100% |

排序内部：4 次 spill 各约 0.2–0.26 s，最终归并 1.75 s。

## 当前基准套件（bench.py，含 CPU）

`docs/bench/latest.json`（合成 200k reads，2 次重复）与 `docs/bench/big.json`（`big.bam`，58.9M reads，1 次）：

| case | wall (s) | CPU (s) | 峰值 RSS (kB) | 线程峰值 | 临时盘峰值 (B) | 输出 (B) |
|---|---:|---:|---:|---:|---:|---:|
| dedup | 0.065 | 0.135 | 42,572 | 35 | 0 | 106,143 |
| group | 0.784 | 0.855 | 141,192 | 26 | 0 | 12,642,692 |
| count | 0.032 | 0.015 | 7,412 | 17 | 0 | 40 |
| count_tab | 0.035 | 0.010 | 6,320 | 1 | 0 | 290,011 |
| dedup_low-memory | 0.066 | 0.135 | 42,592 | 35 | 0 | 106,143 |
| dedup_high-throughput | 0.064 | 0.130 | 42,528 | 35 | 0 | 106,143 |
| dedup_big（58.9M reads） | 58.820 | 283.950 | 1,806,220 | 35 | 784,234,693 | 692,843,975 |
| count_big（58.9M reads，--per-contig） | 17.755 | 18.980 | 354,520 | 17 | 0 | 8,234 |
| group_big（58.9M reads） | 466.907 | 833.460 | 3,120,164 | 26 | 3,851,413,687 | 7,643,164,831 |
| count_tab_big（5M 行表） | 4.300 | 4.250 | 5,984 | 1 | 0 | 34,450,011 |

- provenance：commit `1057e1a4d249`、二进制 sha256 `0b521bab1f86…`（完整值在 JSON 中）。
- 原始 hyperfine JSON：`docs/bench/hyperfine/{dedup,group,count,count_tab,dedup_low-memory,dedup_high-throughput}.json`；两次 bench 的完整报告为 `docs/bench/latest.json` 与 `docs/bench/big.json`（含工具版本、CPU 型号、逻辑核数、平台）。
- `dedup_big`/`group_big` 的临时盘峰值分别约 784 MB 与 3.85 GB，来自外部排序 spill；`group_big` 同时写出 7.64 GB BAM+TSV。`count_tab_big` 使用 5M 行读段/基因表（count_tab 的输入是表而不是 BAM）。

## 子集结果（399,540 reads）

| 工具 | 相对 |
|---|---:|
| rust-umitools | 1.00（约 0.74 s） |
| umi-tools-rs 2.0.1 | 4.03× |
| UMI-tools 1.1.6 | 9.41× |

三方分子数均为 335,399。

## 优化路径（同一输入）

| 版本 | wall | 峰值 RSS | 说明 |
|---|---:|---:|---|
| 迁移初期 | 88.9 s | 1.47 GiB | 中间 staged BAM + 外部归并；单线程 BGZF 排序 |
| 引入流式排序 + 线程化 BGZF + libdeflate | 16.4 s | 1.93 GiB | 去掉 staged 文件；稳定排序复制 Record |
| 索引排序 + 无分配 CIGAR + 收紧缓冲 | **9.2 s** | **0.78 GiB** | 索引向量排序、192 MiB chunk、批量上限 131k |

## group 与 paired 前后对比（同机）

基线二进制由 Git 提交构建，可复现脚本 `tests/bench/group_paired_before_after.py`（`/usr/bin/time -v`，3 次重复，结果见 `docs/bench/group_paired_before_after.json`）：

- group 基线：`322dd1f`（`9a29b22` 的父提交，`group_bundle` 克隆记录）
- paired 基线：`558da69`（`103e47e` 的父提交，mate 在聚类前登记）

### group：移动记录替代克隆（200 位置 × 20 UMI × 50 reads）

| 指标 | 基线 | 当前 | 变化 |
|---|---:|---:|---:|
| wall | 0.79 s | 0.76 s | −3.8% |
| CPU | 1.62 s | 0.81 s | −50.0% |
| 峰值 RSS | 159,336 kB | 117,416 kB | −26.3% |
| 输出 | 12,027,856 B / 200,000 行 | 同左 | 一致 |

### paired：mate 注册移到代表选择之后（20,000 位置 × 3 重复 pair）

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 输出记录 | 80,000 | 40,000 |
| 输出字节 | 449,942 | 303,005 |
| wall | 0.12 s | 0.10 s |
| CPU | 0.44 s | 0.39 s |
| 峰值 RSS | 88,504 kB | 77,308 kB |

重复 pair 只保留代表 read1 的 mate，输出减半；K5/K6 验证分子与记录正确性。

### paired：大 mate 集合的预算行为（150,000 pair，`--memory-limit 64MiB`）

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 结果 | exit 4（无法在预算内完成） | 完成 |
| wall | — | 0.73 s |
| CPU | — | 2.86 s |
| 峰值 RSS | — | 212,740 kB |
| 输出记录 | — | 300,000 |

当前实现把请求落盘并按块重扫，内存上界为块大小。

## 分配器与线程预算（mimalloc）

glibc 的 malloc 在 dedup 的多线程路径上产生大量 futex/mmap 争用：高 bundle fixture（500k 位置 × 4 reads）上 sys 8.2 s、CPU 12.7 s。改用 `mimalloc` 作为 Rust 全局分配器，并在启动时设置 `purge_delay=0` 后：

| 用例 | glibc 基线 | mimalloc + purge0 | 变化 |
|---|---:|---:|---:|
| dedup 高 bundle | 2.87 s / 12.7 CPU / 2636 MiB | 1.69 s / 4.3 CPU / 2415 MiB | wall −41%，CPU −66%，RSS −8% |
| dedup 低 bundle | 0.74 s / 2.2 CPU / 283 MiB | 0.43 s / 0.6 CPU / 256 MiB | wall −42%，CPU −73%，RSS −10% |
| count 高 bundle | 0.90 s / 2.1 CPU / 314 MiB | 0.97 s / 2.5 CPU / 444 MiB | wall +8%，RSS +41% |
| group 2M | 8.25 s / 9.4 CPU / 1106 MiB | 7.76 s / 9.8 CPU / 816 MiB | wall −6%，RSS −26% |

基线为 `32c8abf` 在独立 worktree 中的 release 构建；`/usr/bin/time -v`，2 次重复取中位数。count 的 RSS 上升来自 mimalloc 对小对象的段保留；`--threads 4` 可降到 411 MiB / 0.92 s。dedup/group 是主要工作流，整体净收益明显。

同批改动还缓存了热路径的 `RUST_UMITOOLS_*` 环境变量（`cluster_with_scratch` 曾对每个 bundle 调用一次 `getenv`），并让 count/group 复用 BGZF reader/writer 线程；I/O 线程数可用 `RUST_UMITOOLS_IO_THREADS` 覆盖，默认 `threads/2`。

### 文本输出缓冲

`group`/`count`/`count_tab` 曾对每一行调用一次 `write`；2M 行 TSV 即 2M 次系统调用。改为 1 MiB `BufWriter` 并在原子提交前显式 flush 后（改动前为 `c6335b6` 构建，2 次重复取中位数）：

| 用例 | 改动前 | 改动后 |
|---|---:|---:|
| group 2M reads（低 bundle） | 13.09 s / 14.9 CPU / 815 MiB | 2.50 s / 4.9 CPU / 815 MiB |
| group 2M reads（高 bundle） | 10.70 s / 17.2 CPU / 2415 MiB | 3.00 s / 10.1 CPU / 2417 MiB |
| count_tab 1M 行 | 0.68 s / 0.7 CPU / 12 MiB | 0.13 s / 0.1 CPU / 12 MiB |

输出字节完全一致：TSV 与解压后的 BAM 校验相同，K6 77/77 继续覆盖上游对照。count 的输出通常很小，收益有限。

### group 临时 BAM 压缩

`group` 先把结果写到临时 BAM，再由 `coordinate_sort` 读出并压缩成最终 BAM；临时 BAM 之前也用 level 6，属于重复压缩。改为 `Fastest` 后（基线 `510c352`，5 次交错中位数）：

| 用例 | 临时 BAM level 6 | Fastest | 变化 |
|---|---:|---:|---:|
| group 高 bundle | 3.14 s / 10.4 CPU | 2.91 s / 8.0 CPU | wall −7%，CPU −23% |
| group 低 bundle | 2.04 s / 3.9 CPU | 2.06 s / 3.6 CPU | 噪声内 |

最终 TSV 与解压后的 BAM 完全一致；只有临时 BAM 变大（进程结束即删除），最终输出体积不变。

### BAM 压缩级别

上游兼容的 `--compresslevel 0-9`，默认 **1**（0 为未压缩 BGZF；上游默认 6）。随机 500k-read fixture 上（3 次中位数）：

| level | dedup wall / CPU / out | group wall / CPU / out |
|---|---:|---:|
| 1 | 0.30 s / 0.7 / 14.1 MiB | 0.61 s / 1.2 / 17.2 MiB |
| 4 | 0.32 s / 0.8 / 13.6 MiB | — |
| 6 | 0.33 s / 1.0 / 13.5 MiB | 0.68 s / 1.7 / 17.1 MiB |

全 A fixture 上体积几乎不变，dedup 高 bundle 的 level 1 为 1.42 s / 3.5 CPU（level 6 为 1.58 s / 4.0 CPU）。默认 1 换取更低 CPU/wall；需要更小输出时可用 `--compresslevel 6`。解压内容不变（K5 77/77）。

### count 紧凑存储

`count` 之前复用通用 `Bundle`（`Entry{count,ties,reads}` + `order`），即使 `CountOnly` 也保留 read/tie 字段和重复的 order 向量。改为专用 `CountBundle`（`FxHashMap<Umi,u64>`，flush 时排序物化）后（基线 `4e00a52`，5 次交错中位数）：

| 用例 | 通用 Bundle | CountBundle | 变化 |
|---|---:|---:|---:|
| count 高 bundle | 0.99 s / 2.5 CPU / 444 MiB | 0.93 s / 3.0 CPU / 203 MiB | RSS −54%，wall −6%，CPU +20% |

进一步去掉 flush 时的排序：`CountBundle` 保留插入顺序 `order: Vec<Umi>`。与排序版同机交错对比：1.68 s / 3.8 CPU / 202 MiB → 1.53 s / 3.2 CPU / 255 MiB（wall −9%，CPU −16%，RSS +26%，仍低于 glibc 基线 314 MiB），TSV 完全一致。

另外，packed UMI 的 `substitute` 改为 u64 编码 + 掩码 XOR 查找：count 高 bundle 0.760→0.720 s、CPU 1.7→1.5（dedup/group 的 bundle 走 pairwise，不受影响）。

输出 TSV 完全一致（K5 77/77）。CPU 上升来自 flush 时对 2M UMI 的排序，RSS 收益远大于该代价。

### count-only 聚类（不物化成员）

`count` 只需要簇数量，不需要每个簇的成员顺序。directional 走 seeded traversal 计数，cluster 走 union-find 计数，adjacency 保留完整分组作为回退。fixture 为 `high.bam`（500k 位置 × 4 reads，2M reads），`--per-gene --gene-tag GX --threads 16 --quiet`，旧/新二进制交替 3 次取中位数：

| 方法 | 旧 wall / CPU / RSS | 新 wall / CPU / RSS | 输出 |
|---|---:|---:|---|
| directional | 0.67 s / 1.43 / 261,504 kB | 0.68 s / 1.28 / 250,632 kB | 记录流 md5 相同 |
| cluster | 0.71 s / 1.68 / 250,536 kB | 0.69 s / 1.30 / 248,872 kB | 记录流 md5 相同 |
| adjacency | 0.69 s / 1.55 / 253,492 kB | 0.69 s / 1.58 / 250,988 kB | 回退路径，噪声内 |

directional CPU 约 −10%，cluster 约 −23%；wall 基本持平，RSS 约 −4%。adjacency 仍需要分组，未获得结构性收益。

### dedup 峰值 RSS 与预算

`high.bam`（2M reads），`--threads 16 --quiet`，3 次中位数：

| `--memory-limit` | wall | CPU | 峰值 RSS |
|---|---:|---:|---:|
| 256 MiB | 2.36 s | 4.93 | 331,192 kB |
| 512 MiB | 1.81 s | 3.56 | 670,512 kB |
| 1 GiB | 1.62 s | 3.54 | 1,302,472 kB |
| 2 GiB | 1.39 s | 3.22 | 2,473,404 kB |
| 4 GiB | 1.38 s | 3.21 | 2,473,172 kB |
| auto（≈可用内存一半） | 1.38 s | 3.23 | 2,473,320 kB |

- `--sort-strategy low-memory` 不改变该峰值（仍约 2.47 GiB），说明峰值由 pending/batch 预算与排序缓冲共同决定，单纯缩小 sorter chunk 不够。
- 显式 `--memory-limit 1GiB` 可把 RSS 降到约 1.30 GiB，但 wall 增加约 17%；默认继续使用可用内存，避免为低重复大输入引入普遍减速。需要低 RSS 的流水线应显式设置预算。
- 旧/新二进制以及各预算下的解压记录流完全一致；`samtools view -h` 的差异仅来自 `@PG` 命令行路径。

T2/T4 的原始前后数据、fixture 定义与复现命令见 `docs/bench/tuning.json`。

## 复现

```bash
export PATH=<umi_tools 环境>:$PATH
hyperfine --warmup 0 --min-runs 3 \
  --command-name umi_tools      "umi_tools dedup -I small.bam -S /tmp/u.bam --random-seed 1" \
  --command-name rust-umitools  "rust-umitools dedup -I small.bam -S /tmp/r.bam --random-seed 1" \
  --command-name umi-tools-rs   "umi-tools-rs dedup -I small.bam -S /tmp/c.bam --random-seed 1"

/usr/bin/time -v rust-umitools dedup -I small.bam -S /tmp/r.bam --random-seed 1
```

## 说明

- 未测量 `--output-stats` 与 `--per-gene` 的大数据集开销；`big.bam` 上测了 dedup、`count --per-contig` 与 group，count_tab 用 5M 行表单独测量；paired 的前后对比如上（含 15 万对的大 mate 预算用例）。
- `--memory-limit` 先预留 8–32 MiB 给 HTSlib、分配器与线程栈。64/128/256 MiB 预算在 200k reads 合成 fixture 上实测峰值 RSS 57.1/102.7/187.0 MiB；预算是缓冲预算，不是硬性进程上限。
- 两次运行的输出要比较记录，必须固定 `--random-seed`；不设置时上游不可复现。
- 默认不写自有 `@PG`，因此头部可直接比较；比较忽略压缩后的字节差异（压缩级别/块布局），逐字段比较解压后的内容。

## 邻居策略校准（合成 bundle）

强制策略、3 次平均；`RUST_UMITOOLS_NEIGHBOR_STRATEGY` 与 `RUST_UMITOOLS_TIMING` 可复现。

| UMI 数 | k=1 pairwise | k=1 substitute | k=2 pairwise | k=2 partition |
|---:|---:|---:|---:|---:|
| 100 | 23.3 µs | 30.3 µs | — | — |
| 150 | 66.0 µs | 35.3 µs | — | — |
| 250 | 166.3 µs | 56.0 µs | — | — |
| 800 | 1170.3 µs | 205.7 µs | 1583 µs | 3218 µs |
| 1600 | — | — | 4826 µs | 9332 µs |
| 3200 | — | — | 16879 µs | 35717 µs |

据此把统一切断值 25 改为分档：k≤1 用 100、k=2 用 3000、k≥3 用 500（未测档位保守取值）。三种策略的边集合与输出相同（BAM md5 一致）。

## 邻居枚举占比

U=400、k=1：枚举 0.095–0.109 ms，整次运行约 10 ms（约 1%）。因此未启用 2-bit/SIMD/arena，保留可移植标量路径。

## 排序策略（`--memory-limit 64MiB`）

| 策略 | chunk | wall |
|---|---:|---:|
| auto | 2 MiB | 0.012 s |
| low-memory | 1 MiB | 0.012 s |
| high-throughput | 8 MiB | 0.013 s |

三个输出 BAM 的 md5 去重后为 1 个（策略不改变输出）；该输入未触发 spill，策略主要影响溢写行为。

## 有界内存路径

- bundle 窗口在位置边界提前结束；mate 请求超限落盘并按块重扫；gene 聚合按 gene 哈希分区；单个超大 bundle 使用分块精确邻居 + 磁盘 pair 索引/CSR。
- K9 强制这些路径并与默认路径逐字节比较；15.49× 的测量使用默认路径，未叠加上述选项，默认行为不变。

## 线程扫描（合成 200k reads）

`--threads` 在完整机器与 `taskset -c 0-1`（2 CPU）下的 hyperfine 均值（3 次）：

| threads | 20 CPUs | 2 CPUs（taskset） |
|---:|---:|---:|
| 1 | 0.016 s | 0.016 s |
| 2 | 0.016 s | 0.016 s |
| 4 | 0.017 s | 0.017 s |
| 8 | 0.017 s | 0.017 s |

该输入约 16 ms，启动与固定开销占主导，线程数不改变结果；受限亲和性下也无退化。大输入的扩展性以 `small.bam` 的 9.2 s 与分阶段数据为准。

## 退化调查阈值

在固定 runner、固定输入与参数的条件下，若某命令的中位 wall time 或峰值 RSS 相对上一记录版本**持续退化超过 10%**，作为调查项处理并复测；跨机器或跨文件系统不设绝对阈值。
