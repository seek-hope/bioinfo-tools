# rust-umitools 迭代 roadmap

> 历史迭代记录，分析基线为 `558da69`。文中的 R0–R5 和“当前”描述该快照；
> 后续功能以 [COMPATIBILITY](COMPATIBILITY.md) 为准，当前批次顺序见
> [根开发计划](../../docs/DEVELOPMENT-PLAN.md)。

分析基线：`558da69`，版本 `0.2.0`，对照 UMI-tools `1.1.6`。本文是后续迭代建议，不代表功能已经实现，也不承诺发布日期。分析依据为当前源码、兼容矩阵、已有基准，以及本轮新增的双端最小对照实验；未重新运行大型性能基准。

建议顺序：**先修结果正确性和资源约束，再扩大验证与性能覆盖，随后完善单细胞和 FASTQ 工作流。** 当前 15.49× 加速是特定单端输入上的结果，不宜直接推广到双端、统计输出、超高深度或单细胞数据。

## 0. 实施状态

R0–R5 已按本文范围实施（2026-10），不含文末“暂缓方向”。证据入口：

| 阶段 | 状态 | 关键证据 |
|---|---|---|
| R0 正确性与资源 | 完成 | exact 77 项、oracle 39 项；`--output-stats` 写前拒绝；输出路径预检与原子提交 |
| R1 验证与发布 | 完成 | `compatibility.tsv`（50 行）+ K7；`versions.py` 固定 oracle；随机差分 12/12；确定性 8/8；bench（wall/CPU/RSS/线程/临时盘/阶段/provenance，含 big.bam 与 hyperfine 原始 JSON）；CI 与容器；`--quiet/--progress/--summary-json/--version --verbose` |
| R2 有界与性能 | 完成 | 位置边界窗口、mate 落盘 join、gene 分区、超大 bundle 磁盘 CSR（K9）；group 移动记录（CPU 1.62→0.81 s、RSS 159,336→117,416 kB）；paired mate 注册（重复 pair 输出 80,000→40,000 条）与大 mate 预算（基线 exit 4、当前完成）；整 contig 缓冲（`--buffer-whole-contig`）与 8–32 MiB 预留；无向 union-find；策略阈值实测分档；排序策略；cgroup 感知 |
| R3 单细胞 | 完成 | per-gene/per-cell dedup、group per-gene、count per-cell/wide、`--chrom`、`--subset`、`--filter-umi`（单端/配对白名单）、`count --gene-transcript-map`；`dedup` 的 map 组合仍写前拒绝 |
| R4 FASTQ | 完成 | extract string/regex、whitelist distance/density/纠错；K10 20/20 |
| R5 格式 | 完成 | stdin/stdout（spool）、CRAM（`--reference`）、region、SIGPIPE 与背压；K11 |

R5 之后的性能优化：mimalloc、文本输出缓冲、group 临时 BAM Fastest、默认 `--compresslevel 1`、count 紧凑 `CountBundle`；同机前后测量见 `docs/BENCHMARK.md`。

说明：T11 的机制与原文不同——bundle 只接收同一位置的 read，因此在位置边界结束窗口即可精确保持有界内存，无需持久化状态/RNG 回放；输出等价性由 K9 证明。

## 1. 当前证据与需要修正的认识

分析时的 37 项语义对照、38 项字节对照和 small.bam 基准是有效的已记录证据，但有限 fixtures 不能证明所有参数组合正确。本轮发现的双端反例说明当时的“38/38”不能当作无条件的全功能兼容承诺；补充回归后字节对照为 44 项。

| ID | 发现及证据 | 对迭代的影响 |
|---|---|---|
| F01 | **已修复：双端去重多保留 mate。** 两对同位置、同 UMI 的 reads，修复前上游输出 2 条、Rust 输出 3 条；mate 请求原在聚类选择前登记。 | 已修复：请求只在代表 read 选定后登记，并有 6 个字节级回归用例；见第 8 节。 |
| F02 | **已修复：统计 null 列为占位值。** 旧实现把观测值原样写入 `unique_null` / `directional_null`。 | 已改为在参数验证阶段拒绝 `--output-stats`，不再输出可能被误读的统计。 |
| F03 | **已修复（有界路径）：内存预算不等于 RSS 硬上限，但所有增长源都有有界路径。** 窗口/mate/gene/超大 bundle 均可落盘，cgroup 限额参与预算。 | 全进程 RSS 硬上限仍受分配器与 HTSlib 缓冲影响，已在文档声明。 |
| F04 | **已修复：增长源均有磁盘后备。** count 分区、mate 落盘 join、bundle 窗口提前结束、超大 bundle 磁盘 CSR。 | 强制路径与内存路径逐字节一致（K9）；单个超大 gene 仍可能 exit 4。 |
| F05 | **已修复：归并 fan-in 与线程数有上限。** `merge_runs` 改为多轮归并（每轮最多 32 个 run），每个 run 的 reader 单线程。 | 线程峰值不再随 run 数线性增长；仍非进程级统一线程池。 |
| F06 | **已修复：临时盘检查改为按需。** 原实现在 Budget::resolve 中无条件要求 256 MiB 可用空间，且 cache×8、free/8 各有 64 MiB 下限。 | 现在下限为 1 MiB，仅在 spill 前通过 `ensure_tmp_space` 检查实际所需空间；无 spill 的运行不再受无关磁盘满影响。缩小 chunk 仍不减少总 spill 数据量，属后续调优。 |
| F07 | **部分修复：输出安全。** 全部输出写前做规范路径与硬链接冲突检查；`group --group-out` 改为临时文件提交。 | 多产物跨 rename 的事务性仍有限，已在文档声明。 |
| F08 | **源码确认：验证覆盖有限。** exact 测试只打印 oracle 版本；主要是小 fixture，不强制走多 run 归并；paired fixture 没有重复 read pairs 的合并场景。 | 必须建立按行为覆盖的矩阵，不能只增加 PASS 数量。 |

源码定位：[dedup](../src/commands/dedup.rs)、[mate 回收](../src/io/paired.rs)、[预算](../src/runtime/budget.rs)、[排序](../src/io/sort.rs)、[group](../src/commands/group.rs)、[count](../src/commands/count.rs)、[count_tab](../src/commands/count_tab.rs)、[exact harness](../tests/oracle/exact.py)。上述资源风险来自静态检查，本轮没有做 OOM、磁盘耗尽或线程压力实验。

## 2. 迭代顺序与依赖

版本号为建议；每阶段按验收条件完成，不按日期强行发布。工作量 S/M/L 分别表示局部修改、跨模块修改、涉及新存储或新语义的较大工作；不是工期估计。

| 阶段 | 优先级与建议版本 | 核心交付 | 依赖 / 工作量 |
|---|---|---|---|
| R0 | P0，0.2.x 修复线 | 双端正确性、统计诚实性、资源约束、输出安全 | 立即启动；总体 L，可拆分补丁 |
| R1 | P1，0.3 | 更完整的兼容测试、可移植 CI、可复现基准及发布包 | 可与 R0 部分并行；M–L |
| R2 | P1，0.4 | 大规模有界处理、group/paired 优化、精确邻居算法优化 | R0 资源模型 + R1 验证；L |
| R3 | P1，0.5 | gene/cell 工作流、稀疏计数与相关兼容参数 | R1 语义矩阵 + R2 存储；L |
| R4 | P2，0.6–0.7 | `extract`，随后 `whitelist` | R1 发布基础；白名单依赖条码提取验证；L |
| R5 | P2，按能力独立发布 | stdin/stdout、CRAM、region/index 集成 | R0 I/O 契约 + R2 spool；M–L |

依赖重点：大 bundle 磁盘处理和单细胞聚合共用存储基础；双端 stdin 必须具备可重读 spool；性能优化必须先有能够覆盖新执行路径的差分测试。发行与帮助文档可持续并行改进，不必等全部新命令完成。

## 3. R0：先保证结果与资源承诺可靠

### R0-1 双端 mate 回收正确性（P0，M）

把 mate 请求的产生位置移到最终代表 read 确定之后，保留上游 mate key 和消费规则。请求集合应进入统一预算；大集合的磁盘后备可在 R2 完成，期间必须提前给出资源错误。

验收：文末重复 pair 反例与上游一致；覆盖同 UMI 重复、不同 UMI 归并、MAPQ 平局、多 seed、重复 QNAME、跨 contig、缺 mate、secondary/supplementary、unpaired/chimeric/unmapped 策略；同时检查 read1 和 read2 的身份、数量、顺序与标签，不能只看分子数。排序和不排序路径均验证。

### R0-2 统计输出的科学语义（P0，S→L）

先停止把占位 null 值作为可分析的统计输出：在完整实现前明确拒绝该能力，或提供清晰命名、与上游兼容统计分开的实验输出。兼容实现需逐一核对上游产物清单、统计对象、分箱、pre/post 合并方式、null 模型、随机生成器及浮点格式。

当前 `pre` 和 `post` 分别输出，同一 count 可出现两行；距离统计仅枚举阈值内候选。这些也需要对照，不能只替换 RNG 就宣称完成。

验收：所有声称兼容的统计文件都与固定 oracle 对照；包含无 reads、单 UMI、多 UMI、缺失 bins、pre/post 共享 count 和固定 seed。随机统计如不能逐字节对齐，必须单独说明算法与误差标准，不能纳入“exact”覆盖。

### R0-3 统一内存、线程和临时盘预算（P0，L）

建立按实际数据量预留和释放的预算，覆盖 active bundles、待处理批次、输出重排、排序索引、aux tags、图边、mates、结果集合、压缩缓冲和安全余量。区分“单 UMI 多 reads”和“多 UMI 高图密度”：两者不能使用同一个 UMI 数上限估算。EOF 和 contig 切换也要分批提交，不能把所有剩余 bundles 一次投入 worker。

检测有效资源：结合宿主可用内存与 cgroup v1/v2 限额/当前占用，CPU 结合 affinity/quota；读不到信息时明确保守回退。启动时拒绝无法容纳运行时与最小工作集的预算，不能接受 16 MiB 却预留 64 MiB chunk。

线程采用共享 HTSlib pool 或明确的阶段分配，避免每个 run 单独开一组线程。多轮有限 fan-in 归并同时考虑内存、线程和文件描述符上限；维持原始序号作为稳定平局依据。

临时盘应按是否需要 spill 决定检查，计入目标目录与 tmpdir 所在文件系统，运行中跟踪占用并处理 ENOSPC。保留 cache/free 启发式为可评估策略，不把“参与公式”当作最佳调优证据。

验收：在声明支持的最低预算及 128 MiB / 512 MiB / 1 GiB 等档位，对四个命令、长 aux tags、深度极高和密集图输入进行受控测试；成功时峰值 RSS 不超过声明上限，不可满足时在 OOM 前 exit 4。记录系统/分配器开销，不能通过重定义上限隐藏超额。低 fd 上限、多 runs、容器限额均受测；工作线程峰值符合公布契约，主线程及必要控制线程单独说明。

### R0-4 输出安全与输入前提（P0，M）

统一验证 input、BAM、TSV、stats、log 之间的同路径、符号链接和硬链接别名；在创建可见输出前完成所有参数验证。`group-out` 也走 TempOutput；多文件输出先全部生成并验证，再提交，最后写完成标记。普通文件系统多个 rename 不是原子事务，需明确失败恢复策略。

核对 HTSlib close/flush 错误是否能经包装层传播；验证 SIGINT 和磁盘写满清理，SIGKILL 后用可识别清单恢复清理。`count_tab` 对 gene 非连续输入（G1/G2/G1）应明确要求、校验或提供有界聚合模式，避免静默输出重复 gene 行被误解为完整总计。

验收：损坏输入、写满、无权限、路径别名、错误参数、提交失败的情况下，不破坏输入和既有输出，不留下被标记为成功的半成品；stdout 无法回滚时返回非零并明确流式失败约定。

## 4. R1：让兼容性和性能证据可维护

### R1-1 行为矩阵、差分测试与确定性（P1，M–L）

建立机器可读的兼容清单：命令、参数组合、oracle 版本、fixture、比较级别、已知差异。自动生成文档，区分“实现了”“测试通过的组合”和“已知失败”。`experimental` 参数优先验证 MAPQ、NH/X0/XT、splicing、read length、ignore TLEN 和代表 read 平局。

扩展 UMI 长度 1/8/9/21/22/32/更长、N/非标准字节、距离 0/1/2/大于等于长度、不同字母表、相同坐标平局、长 soft clip/intron、缺 tags、超大 UG 编号及已有 UG/BX 标签。比较成员、代表、计数、顺序、标签、头部和 TSV，保留首差异定位。

随机生成小合法输入与暴力算法、上游交叉验证；失败自动缩减为 fixture。跨 threads、batch/chunk、单 run/多 run/多轮归并验证确定性。特别审计 adjacency 的 set 顺序是否只影响行顺序，不能未经大组反例验证就断言永不影响下游 RNG 或 group IDs。

验收：锁定并检查 UMI-tools 1.1.6 及 Python/NumPy/pysam/samtools 环境；oracle 版本错误时失败；所有缺陷有回归；新增优化路径至少有一个必经用例。大 BAM 使用分块字节比较，避免 `gzip.read()` 同时装入两份解压文件。

### R1-2 多维基准与归因（P1，M）

保留目前 small.bam 的同机证据，新增真实大型输入与可公开生成的合成负载，至少覆盖：单/双端，dedup/group/count，低/高重复率，高 UMI 多样性，长 reads/aux tags，阈值 1/2，多 runs 和低内存。

测量 wall time、CPU 时间、峰值 RSS、线程峰值、临时盘峰值/读写量、输出大小；拆开解码、语义分组、邻居枚举、聚类、mates、排序与压缩。现有“解析+分组+聚类 78%”不足以判断 Hamming 是主要瓶颈。

保存输入 checksum、命令、工具与动态库版本、提交号、二进制 checksum、硬件/容器配置、原始 hyperfine JSON 和内存日志。分别报告双方默认资源和同资源预算条件，记录 warm/cold cache 与运行顺序；竞品版本更新另立基线。

验收：每项加速都有正确性证据和原始测量；自动报告中位数、分布、内存及 I/O。建议将稳定专用 runner 上超过 10% 的持续退化设为调查阈值，而非通用机器上的绝对性能保证。收益小于运行噪声时不默认启用新策略。

### R1-3 发布、安装与诊断体验（P1，M）

将当前依赖本机缓存、默认个人 oracle 路径的验证脚本，与通用 CI 环境安装拆开。测试声明的 MSRV（Cargo.toml 为 1.88，锁定依赖实测最低要求）与 CI 是否一致；增加 Linux x86_64/aarch64，macOS 按需求验证。提供带校验和的二进制、容器，随后评估 Bioconda。

增加 `--version --verbose`、统一 `--quiet`/`--progress`、结构化 `--summary-json`、配置预检和可复制错误建议。摘要分别列出 records、read pairs、molecules、过滤原因和资源高水位；`samtools view -c` 在双端情况下统计的是 records，不能直接当分子数。

验收：干净环境安装与最小工作流成功；构建、来源、许可证和支持平台清晰；stdout 数据不混日志；JSON schema 稳定；容器收到中断能正确退出。版本发布附兼容矩阵和基准变更。

## 5. R2：大规模处理与下一轮性能优化

### R2-1 真正的磁盘后备处理（P1，L）

把“活跃窗口太大”“单 bundle 太大”“mate 请求太多”“count 聚合太大”分别建模。保存输入序号、bundle 生命周期、RNG 状态或可等价回放事件；group 的完整记录可落盘并按引用回放。单个巨大 UMI 图需要磁盘索引或分块精确查询，不能简单把同一 bundle 拆成独立组。

验收：同输入在内存与 spill 模式下保持代表 read、组编号、顺序、标签和计数一致；人为降低预算必经 spill；中断和空间不足不遗留成功产物；资源随输入增长仍有明确上界。任意按文件大小切 BAM 会拆散分组，不应作为通用恢复建议。

### R2-2 优先改善 group 和 paired（P1，M–L）

`group_bundle` 改为消费 bundle 并移动 reads，去掉 `.reads.clone()`；统一使用 SortingWriter，减少 staged BAM 写入/重读。`coordinate_sort` 当前还有一层 chunk Vec，可直接向 sorter 提交以减少重复缓冲。

paired 的第二遍扫描先共享解压线程预算，再比较顺序重读、索引查询和磁盘 join。网络盘上大量随机 seek 未必比顺序读快，需按介质实测。流式读取、worker 和 writer 可用有界队列重叠，但汇合必须按稳定序号输出，不能改变 RNG 调用顺序。

验收：高重复率 group 与大量 mate 的独立基准显示有意义的 wall/RSS/I/O 改善；小输入无显著退化；结果通过 R1 全部差分和资源测试。实测见 `docs/bench/group_paired_before_after.json`：group CPU 1.62→0.81 s、RSS −26.3%；重复 pair 输出减半；15 万对在 64MiB 预算下基线 exit 4、当前完成。

### R2-3 精确算法与数据布局（P1，M–L，收益待测）

[candidate_pairs](../src/core/neighbors.rs) 先构造全部边，再由 [cluster](../src/core/cluster.rs) 构造邻接表；稠密图存在 O(U²) 边空间。优先评估逐条消费边、CSR 或按需邻居。无向 cluster 可研究 union-find，但 directional 不能直接替换成无向连通分量；保留计数、seed 和排序语义。

减少重复边排序、UMI 哈希查询和临时 counts 分配；为大 adjacency 组维护增量覆盖状态，避免每个候选前缀都重建删除集合。邻居策略选择应依据 UMI 数量、长度、阈值和桶偏斜校准，保留小组快速路径。

2-bit A/C/G/T 编码、长 UMI SIMD、arena 和字符串驻留都作为 profiling 后的候选。必须保持 N/非标准字符、长度和词典序语义，并提供可移植回退。先记录候选比较数、图边数、分配量和缓存行为，再决定是否投入 ISA 优化。

验收：暴力算法与优化算法边集合一致；不同策略分组与代表一致；稠密/稀疏/长 UMI 基准同时报告收益和退化，禁止以近似邻居搜索换取默认加速。

### R2-4 排序、压缩与硬件自适应（P1，M，收益待测）

在统一预算内比较 chunk 大小、压缩级别、I/O 线程和 fan-in；提供少量有明确含义的策略，如自动/低内存/高吞吐。cache×8 不是最优值证明，记录选择原因并用真实负载校准。

安全 watermark 重排可作为减少 spill 的研究方向，但坐标排序不保证修正后的 5′ 位置单调，soft clip、长 intron、mate 回收都会影响释放。未证明安全前保留外部排序回退，不能把 1000 bp 当成普遍安全边界。

验收：工作站、低 CPU 容器和服务器分别测量线程扩展性；参数变化不改变解压后的 BAM；压缩速度、文件大小与临时盘使用一起报告。

## 6. R3–R5：扩展真实工作流

| 能力 | 为什么值得做 / 实施边界 | 验收标准 |
|---|---|---|
| R3：gene/cell-aware dedup/group/count | 使现有归并核心服务更多单细胞流程；核对 gene/cell 提取、未分配/多分配、跨 contig 生命周期，避免简单叠加 key。 | 上游参数组合对照，gene×cell×UMI 成员和计数一致，跨预算和线程确定。 |
| R3：稀疏矩阵与 wide | 大规模 gene×cell 默认采用稀疏表示；Matrix Market 等作为显式扩展，dense wide 需先估算内存。 | barcode/feature 顺序、重复 ID、零计数和矩阵维度有固定契约；与 TSV 互校。 |
| R3：剩余单细胞组合 | 例如更多 map/tag 组合；区域过滤明确 mate 和边界语义，subset 明确 RNG 和采样对象。 | 独立 fixture 与选项组合验收；支持状态逐项公开。 |
| R4：extract 固定模式 | 先实现单/双端 FASTQ、barcode pattern、质量过滤/掩码、压缩 I/O，再做 regex。 | name/sequence/quality 同步裁剪；成对同步、短 read、损坏 FASTQ、失败输出和 gzip 多成员对照。 |
| R4：extract regex | 复杂模式有独立语义与性能成本；Rust regex 并非上游 regex 引擎的直接替代。 | 明确支持语法、命名捕获、模糊匹配等边界；不支持的模式提前拒绝。 |
| R4：whitelist | 高频 barcode、knee/density、纠错映射需要数值和歧义规则兼容。 | 平坦/多峰/极少细胞/并列频次输入对照；输出选择依据与诊断信息；手动阈值可复现。 |
| R5：stdin/stdout | 方便工作流串接；单遍能力先做，双端依赖 spool 或其它明确的两遍方案。 | pipe/file 内容一致，背压有界，SIGPIPE 和中断可处理；明确 stdout 不可回滚。 |
| R5：CRAM / region / index | 复用 HTSlib，明确参考基因组、MD5、reference cache 和索引要求。 | 参考缺失/错误提前报错；BAM↔CRAM 比较保留字段与分组语义，不承诺跨格式压缩字节一致。 |

## 7. 发布门槛与暂缓方向

每个迭代必须同时交付：实现、回归 fixture、兼容矩阵变更、资源证据和用户迁移说明。修正确认错误可改变受影响输入的输出，但必须记录；性能改动不能以减少正确性覆盖换取通过。

建议为每个任务使用统一卡片：`ID / 用户收益 / 源码证据 / 依赖 / 正确性门槛 / 资源门槛 / 性能证据 / 失败回退 / 文档变化`。R0 完成前，不把当前版本标为全面生产就绪。

暂缓以下方向：

- GPU 和分布式执行：先确认 CPU 时间在哪里，解决单机正确性和资源边界；目前没有证据证明传输和调度成本值得承担。
- 自研 BAM/BGZF 库：先优化现有 HTSlib 调度和内存，避免同时承担格式兼容风险。
- 无条件追求压缩 BAM 文件字节相同：主契约保持解压后的完整头部与记录一致；压缩字节复现仅在明确需求下固定整个工具链。
- 近似归并、新纠错模型：与兼容模式分开命名和验证，不能静默替换上游算法。
- GUI 和复杂调参面板：先让安装、错误信息、默认策略和工作流集成稳定。

最先建议拆出的任务是 R0-1 双端修复、R0-2 统计保护、R0-3 预算与归并线程约束、R0-4 输出安全，以及 R1-1 的回归基础。完成这些后，再用 profiling 决定 R2 中性能工作的具体顺序。

## 8. 双端反例与验证记录

本轮在 `558da69` 执行 release build 后，使用以下 SAM，经过 `samtools sort` 与 `samtools index`，分别运行两个实现。输入是同位置、同 UMI 的两对 reads；以下各列之间为制表符。

```text
@HD	VN:1.6	SO:coordinate
@SQ	SN:chr1	LN:10000
p1_AAAAAAAA	99	chr1	100	60	10M	=	150	60	AAAAAAAAAA	IIIIIIIIII
p2_AAAAAAAA	99	chr1	100	60	10M	=	150	60	AAAAAAAAAA	IIIIIIIIII
p1_AAAAAAAA	147	chr1	150	60	10M	=	100	-60	AAAAAAAAAA	IIIIIIIIII
p2_AAAAAAAA	147	chr1	150	60	10M	=	100	-60	AAAAAAAAAA	IIIIIIIIII
```

```bash
samtools sort -o in.bam in.sam
samtools index in.bam
umi_tools dedup -I in.bam -S upstream.bam --paired --random-seed 1
rust-umitools dedup -I in.bam -S rust.bam --paired --random-seed 1
samtools view upstream.bam
samtools view rust.bam
```

| 实现 | 输出 QNAME / FLAG | 条数 |
|---|---|---:|
| UMI-tools 1.1.6 | p2_AAAAAAAA / 99；p2_AAAAAAAA / 147 | 2 |
| rust-umitools 558da69 | p2_AAAAAAAA / 99；p1_AAAAAAAA / 147；p2_AAAAAAAA / 147 | 3 |

实验临时文件已清理；最小输入与命令保留于此。该结果不否定此前单端 small.bam 的对照结果，也不表示所有双端输入都会失败；它说明重复 pair 场景必须修复并补入自动回归。

相关文档：[当前兼容矩阵](COMPATIBILITY.md)、[已记录基准](BENCHMARK.md)、[早期实现设计](IMPLEMENTATION_PLAN.md)。本文保留历史迭代背景；当前优先级见[根开发计划](../../docs/DEVELOPMENT-PLAN.md)。
