# bioinfo-tools 开发计划

本文记录当前工作树的开发顺序。命令级支持状态以
[兼容矩阵](../compat/README.md) 为准；历史任务、验收数字和性能记录见文末链接，
不在这里重复维护。

目标是做好可组合、可验证的 Rust 生物信息学工具，而非单纯堆砌命令数量。每个批次只聚焦
一个工具或一组紧密相关的行为：先明确对照方法，再动手实现并同步更新文档。

## 1. 当前能力

| 组件 | 已有能力 | 主要边界 |
| --- | --- | --- |
| `bioinfo-common` / `bioinfo-fastx` | 公共诊断、摘要、FASTX 流式解析和压缩支持 | 各 CLI 的上游例外仍由适配层处理；共享流式读取不代表算法总内存固定 |
| `rust-cutadapt` | cutadapt 4.6 选项和报告矩阵、单双端、过滤与拆样 | cutadapt 5.x 不在目标内；压缩字节和已记录诊断存在差异 |
| `rust-umitools` | extract、whitelist、dedup、group、count、count_tab；`--output-stats` 的五种聚类方法与 NumPy null 模型 | gene-map 代表 read 受上游 hash 顺序影响；`--representative=stable` 是项目扩展；CRAM 缓冲有已记录限制 |
| `rust-fx` | FASTX 统计、变换、过滤、去重、排序、翻译和区间提取等已注册命令；全局 `--compress-level` | 未实现命令和剩余选项见矩阵；部分 two-pass、输入提交策略和上游错误路径仍为 experimental |
| `rust-bam` | view、flagstat、idxstats、quickcheck、index、sort、depth、coverage、flags、head | depth/coverage 为子集；CRAM 不支持；sort 单线程；其余 samtools 命令未覆盖 |
| `rust-fastqc` | fastqc_data、summary、数据表 HTML 和 ZIP；12 个 QC 模块；自定义 limits/adapter/contaminant；MultiQC 消费路径 | HTML/ZIP 为 experimental；不生成完整图形和辅助资源；已证明的计算修正以 exception 登记 |

发布工作流目前只打包 `rust-cutadapt` 和 `rust-umitools`。其余组件虽可从源码构建，
但不能算作已发布。

## 2. 工作规则

- 保持各 CLI 的参数、退出码、报告和确定性契约；共享库不抹平上游差异。
- 新行为先写退出条件。需要上游兼容时，必须有固定版本的差分或明确的语义检查。
- `verified` 只用于有可执行证据的表面；部分一致标为 `experimental`，缺失能力在输出前拒绝。
- `bioinfo-fastx` 的 zstd 依赖已升级到 `zstd 0.14.3` / `zstd-safe 8.2.0` / `zstd-sys 2.1.2`，
  以覆盖 CI 在线 RustSec 检查报告的 RUSTSEC-2026-0391/0398/0403/0410；来源仍为 crates.io，
  由 `Cargo.lock` 固定，API 与压缩格式不变，但需重新执行构建、解压差分和资源回归。
- `rust-fx` 对 `regex = "1"` 的直接依赖用于 SeqKit `replace` 的捕获变量和正则过滤；来源为 crates.io，
  版本由现有 `Cargo.lock` 固定，未引入新的包或运行时服务，只增加 rust-fx 与已有 regex crate 的编译/链接关系。
- 性能结果绑定输入、二进制、硬件、线程、压缩级别和同输入正确性，不使用历史倍数宣传当前构建。
- 内存说明区分输入流、算法缓冲和进程 RSS；需要落盘时同时记录临时盘与故障清理。
- 每批结束后冻结代码，再更新矩阵和文档并运行门禁。避免用不断新增 experimental 命令代替关闭缺口。
- 遵守 [DOC-AUDIT](DOC-AUDIT.md) 中的历史证据和受保护文件边界；发布、tag 和提交需要明确授权。

## 3. 批次顺序

### P0：可验证基线（已完成）

2026-10-26 已完成 UMI help/provenance 契约刷新，provenance 改为 noodles，parallel/commands
与实现对齐。M3 40/40 及负向控制通过，`bash tests/ci/check.sh` 和
`bash tests/ci/check.sh --with-fastx-qc-oracles` 均通过。

后续契约变更仍需先获批准并保留独立证据；失败项按实现错误、文档漂移或上游非确定性分类，
不通过删除断言或放宽比较来消除失败。

### P1：关闭现有 experimental 表面

目标是实现无感迁移：按工具分批推进，不靠删行或缩小覆盖范围来消除 experimental。
默认路径需对齐参数、记录内容与顺序、退出码、报错输出以及索引/报告文件；只有上游本身
不确定的字段才使用事先声明的语义比较。性能与交互改进不能破坏现有脚本兼容性。

本批聚焦 SeqKit `range`、`tab2fx` 的已知缺口，退出条件如下：

- `range`：对齐有效/非法区间、逐文件边界和负向尾部；`正数:-1` 对 stdin 单遍流式处理，
  不接受上游拒绝的混合符号区间。正向额外内存 O(最长记录)，负向 O(保留尾部的记录字节数)。
- `tab2fx`：对齐列数、空字段、逐文件 FASTQ 状态、原始字节、压缩输入和 scanner 边界；
  以大/小合法前缀后接错误验证部分输出，不把上游输出缓冲误认为全文件预扫描。
- 新增固定版本的直接差分，比较退出码、stdout/输出文件及诊断主体（允许工具诊断前缀不同）；
  不修改既有 oracle/fixture。完成后冻结代码，重跑组件 oracle 与两道 CI 门。

本批直接差分结果：`tab2fx` 99/99；`range` 203/206。后者的三个差异均为 FASTQ 后接
FASTA 时上游复用旧质量字段；Rust 保留各文件的格式，不生成虚假质量，该批结束时仍标为
experimental，之后已按第 4 节用户决定登记为有独立证据的 exception。区间限制、stdin
流式路径及 wrapped FASTQ 已对齐。具体比较规则、
内存边界与复现命令见 [兼容说明](../compat/README.md#seqkit-streaming-evidence-and-remaining-boundaries)。

同时修复共享压缩器完成后未显式刷新底层缓冲的问题：写入/flush 故障必须返回失败，
不能由析构函数静默吞掉。新增故障注入测试覆盖 plain/gzip/bzip2/xz/zstd；不改变
`Levels::DEFAULT/FAST` 或压缩级别映射。

后续各工具的既有范围：

- **T4 / UMI-tools**：`--output-stats` 的五种聚类方法、单/多 bundle 和多个 seed 已完成固定版本差分；
  保留 gene-map 和 `--representative=stable` 的实验状态，除非有新的上游证据。
- **T5 / samtools**：已完成 depth/coverage 的已登记选项和输出工件批次；CRAM、顺序 region 和其余命令仍是独立范围。
- **T6 / FastQC**：已完成 contaminant、adapter、tile、Kmer、limits、长度/分组选项和 HTML/ZIP 数据表批次；完整 plot 与辅助资源仍未覆盖。
- **T7 / SeqKit/fq**：已完成已实现命令的选项批次，包括 translate、subseq、sort、faidx、replace 等；
  未实现命令、压缩 two-pass 和剩余全局选项需要另行分批。

每个表面的退出条件：进程级回归通过、上游对照达到预先声明的比较规则、矩阵与当前 help 同步。
未解决的差异留在矩阵中，不改写为 verified。

### P2：资源、性能与交付

- 用 `bench/run.py` 维护结构正确性基线，用 `bench/upstream.py` 维护同输入上游对照。
- 先分析 profiling，再做并行、压缩或分配优化；优化前后必须在相同资源条件下比较。
- 扩展打包前先确定分发范围。现有两个 CLI 的发布配置不自动覆盖另外三个 CLI。
- coverage-guided fuzz、跨平台原生测试和发布签名仍是独立工作，不由基础单元测试替代。

## 4. 全量兼容性收尾（已授权）

以提交 `5b71fae` 的 11 项 experimental、61 项 unsupported 为起点，逐工具消除
迁移缺口。每项保留原矩阵行，新增实现、回归和固定版本对照后才更新状态；不通过删除
功能、缩小命令含义或委托上游可执行文件制造完整替代的结论。用户已授权必要的 UMI
实现/测试/文档改动；既有 oracle、golden 和历史证据仍保持受保护状态。

批次顺序：SeqKit 已有命令错误路径与索引 → UMI 剩余行为 → samtools 已有命令的
参数/CRAM/区域查询 → FastQC 完整报告与配置 → SeqKit/fq 未实现命令 → samtools
未实现命令。每个批次冻结后运行组件差分与两道质量门；提交和推送另按用户指示。
上游随机顺序用明确的语义比较。用户已决定：有确凿证据的上游错误不复现，必须用
独立正确性检查证明 Rust 的结果；FastQC 保持纯 Rust，JVM 启动专属选项明确拒绝。
这些已批准的差异用单独的 `exception` 状态登记，不能计作上游等价通过，也不能用来
掩盖未实现的科学计算功能。新增例外必须有登记规则、证明文档和可执行验证。

首批 `translate` 退出条件：非法碱基/短序列/空输入及 `-e/-x` 的退出码与输出提交策略
一致；正负六个阅读框、ORF 坐标、ID/描述和起始密码子处理逐字节对齐；保留现有表格
listing 结果。输入采用单遍读取，内存 O(最长记录和输出编码缓冲)，不增加全文件预扫描。
所有新错误/边界用例直接运行 SeqKit 2.14.0，对诊断仅标准化工具前缀和 ANSI 颜色。

首批实现直接差分 756/756、Rust 进程回归 15/15 已通过，`translate` 已转 verified；
矩阵变为 193 verified、10 experimental、61 unsupported（264 行不变）。差分覆盖每张
表的 3375 个 IUPAC 密码子、六个阅读框、ORF、54 个 listing/优先级场景及错误提交边界，
并纳入 FASTX/QC 门禁。固定上游的 trim、反向互补、ORF 坐标和 header quirks 逐项记录在
[兼容说明](../compat/README.md#seqkit-translate-evidence)。完整质量门的当次结果见
[本批证据](EVIDENCE-TRANSLATE-CLOSURE.md)。

按之后确认的“不复现已证明错误”策略，已进一步修正 `translate` 的 IUPAC/RNA 反向
互补、反向 ORF 端点、右端 trim 和描述字段丢字；原始直接差分仍明确报告 710/756，
46 项差异逐项保存双方输出哈希并配合独立正确性验证。`range` 保留输入类型和质量来源，
其原始差分仍为 203/206；独立 fq 验证器证实上游混合格式输出违反长度约束。
两者共通过 434 项正确性检查。FastQC `-j/--java` 与 `--debug` 明确在输入/输出访问前
以退出码 2 拒绝，正常 QC 在无 Java 的 PATH 下成功。见
[错误证明与原生例外](EVIDENCE-UPSTREAM-DEFECTS.md) 和
[`approved-exceptions.json`](../compat/approved-exceptions.json)。

这一策略修正取代前述初批的逐字节兼容结论：当前矩阵用独立的 exception 行公开这些
差异；历史 756/756 结果仅描述曾经复制上游错误的中间实现。未实现的科学功能继续按
批次实施，不自动套用例外。

随后原生实现 FastQC `-a/--adapters`，修复默认短读误报 pass 的问题；独立对照覆盖
44 个场景，其中 35 项完全一致、6 项接头计数修正、3 项空配置在输出前拒绝，并用
读段顺序置换证实上游会漏计等长接头及后续扩大统计范围时的短接头命中。Rust 保存首次
命中位置，在最终报告范围内累计。所有适用的小输入均通过独立计数，而非只比较两边输出。
原始 22 项 QC 基线排除了 Adapter Content，
不能作为该模块证据；本批新增专用验证，不改写旧比较。见
[接头配置证据](EVIDENCE-FASTQC-ADAPTERS.md)。

Kmer 和 tile 原先只在组件缺口说明中列出，现已补入矩阵，未删除任何原行。
当前共 266 行：189 verified、9 experimental、57 unsupported、11 exception。
`--limits` 连同 Kmer 的真实计算已实现；配置解析、ignore 开关、警告/失败阈值及模块数据
均有专用验收。完整 HTML/ZIP 资源继续保持 experimental。

按用户后续要求，以上批次已提交并推送为 `c3c7477`，对应远端 quality 运行
`41186678371` 通过。继续新增矩阵 `unachievable` 原因列（schema 3），仅 JVM 启动
相关的两个例外填写已批准的运行时约束；其余未完成工作和已修正的上游错误不填写。

原生 tile 模块已实现：保留样本抽取、header 识别、均值、排序和报告行为，同时修正
上游把缺失观测当作 Q0 的错误。专用 53 项对照包含 48 项完全一致、4 项缺失值修正和
1 项非法 Phred64 拒绝。独立有理数模型、负向控制与工件检查验证数据正确性。
并修复共享分组在 75 bp 和较大区间上的错误，以及科学计数法格式。
详见 [tile 与约束列证据](EVIDENCE-FASTQC-TILES.md)。该批次的 266 行为：189 verified、
9 experimental、59 unsupported、9 exception；其中 2 行具有非空 unachievable 原因。
已推送为 `1c7ee2b`，远程质量运行 `41192242981` 通过。

随后实现 `--limits` 与原生 Kmer，并用固定版本运行和独立 Decimal 二项概率模型证明
抽样计数缩放、排除观测值的尾概率、依赖 read 顺序的机会数和显著性判定缺陷。
原生实现修正这些问题，拒绝只加载一部分配置后继续产出报告；两行登记为 exception，
不填写 unachievable。另修复变长 read 的质量百分位分组、偶数样本中位长度、空 read
处理和长度分布分组。见 [Kmer 与 limits 证据](EVIDENCE-FASTQC-KMER-LIMITS.md)。
本批两道工作区质量门已通过（`/tmp/bioinfo-kmer-acceptance-*.log`，退出码均为 0）。
专用验收 141 项：111 项配置/模块一致、27 项 Kmer 修正、1 项结构性配置拒绝和
2 项一致的路径预检拒绝；九项证明控制及十二项 CI harness 控制通过。
后续 FastQC 重点是完整图形/资源、输入格式和并行参数。

## 5. 后续方向（用户已选择方案 C）

近期优先补全和验证现有工具。表达分析方向（区间处理、feature counting、稀疏矩阵）
与变异分析方向（VCF/BCF 查询与统计）均保留为未来候选，不承诺实现日期，也不同时启动。
启动前应明确目标用户、上游 oracle、坐标契约和首批准入测试。

本机 README 基准使用 10 万、100 万真实/合成 reads，以及千万条合成记录的流式扩展。
`rust-fx stats` 的 Q20/Q30 边界和 `fq2fa` 默认换行错误已修复，独立对照 22/22；
旧二进制为 16/22。原始失败、修复后 pilot 与正式结果分别保存，见
[实测报告](../bench/RESULTS-README.md)。未将较慢结果排除，也未改变兼容矩阵行数。
FastQC 的 150 bp GC 分布和百万 reads 重复度估计出现新的未归因差异，需在后续 QC
批次中独立调查；不能按现有 exception 自动豁免或宣称完整报告等价。

针对较慢命令的后续优化已覆盖 stats、fq2fa、lint、flagstat、sort 和 FastQC。
同一输入、原始二进制和固定 CPU 的 13 个条件通过；百万条真实 reads 的 stats
从 1.972 秒降至 0.621 秒，千万条单文件 lint 从 5.614 秒降至 2.140 秒，
峰值 RSS 从 829.0 MiB 降至 4.9 MiB。缓存保持浮点累加顺序，GC 缓存只保存
最近一个长度；配对 lint 仍保留 read-one 名称。104 项行为保持检查、八项测量证据
控制及两个完整质量门通过。中止候选、tmpfs 空间不足和三个千万级条件的重测均保留，
详见 [优化报告](../bench/OPTIMIZATION.md)。仍慢于上游的条件继续列出，QC 未归因差异
与图形缺口仍待后续批次处理。

UMI 的 consensus/duplex 和新分析模块也不属于当前兼容性收尾；
如需启动，应单独批准范围，而不是附带在某个小批次中实现。

## 6. 验证入口

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --release --locked
bash tests/ci/check.sh
bash tests/ci/check.sh --with-fastx-qc-oracles
```

组件 oracle、规模化验证和打包检查按各自文档运行。基础测试通过不等于上游兼容、
内存验收或发布就绪。

## 7. 记录索引

- [兼容矩阵](../compat/README.md)：当前支持状态和缺口。
- [基准方法与结果](../bench/README.md)：测量方法和原始结果入口。
- [文档分类](DOC-AUDIT.md)：现行文档、历史记录和受保护文件。
- [历史规划](DEVELOPMENT-PLAN-LEGACY.md)：旧 W0/W1/W2 任务和验收标准。
- `EVIDENCE-*.md`：特定构建的历史执行证据。
- [历史交付快照](DELIVERY.md)：早期本地候选交付记录，不是当前发布状态。
