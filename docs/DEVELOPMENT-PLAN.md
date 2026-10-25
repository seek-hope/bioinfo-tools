# bioinfo-tools 项目推进规划

状态：新版路线图已落档；[文档约束审计](DOC-AUDIT.md) 已完成。阶段 A–D 的本地实现与门禁已完成，
包括 C5/S-03、C7/T10、C11 靶向验证；独立审计已通过（2026-10-23，对象为自检前快照）；随后全局自检的代码改动仅经本地 K1/K4 复核，未再次独立审计。
剩余为远端 CI、签名/上传和发布等外部未验证项；容器/Conda、UMI 全套和 BAM 20/20 已在本地执行。
最终快照见 [EVIDENCE-FINAL.md](EVIDENCE-FINAL.md) 和 [JSON](EVIDENCE-FINAL.json)。
本文不是发布授权或全阶段完成声明。

目标：建设一套在大规模真实数据上可靠、性能可证明、易于集成的生物信息学工具包。
当前优先服务短读长预处理、UMI 分析和常用格式处理，不重写整个生物信息学生态。

本规划基于 monorepo `49953ec` 的源码、仓库内历史证据及本次本地探针。
原规划保存在 [DEVELOPMENT-PLAN-LEGACY.md](DEVELOPMENT-PLAN-LEGACY.md)，其中的外部资料、
旧任务 ID 和验收要求保留供追溯，但旧的“尚未创建”等状态不再描述当前项目。
排期按 1–2 名全职 Rust 工程师及领域验证支持估算，是工作区间而不是交付承诺。

## 1. 推进原则与边界

顺序：数据完整性 → 规模化与资源约束 → 可复现性能 → 工作流与交付 → 新分析模块。

- 保留独立 CLI 和共享库；不为统一入口破坏各上游的参数、退出码、报告和随机性契约。
- 默认严格处理输入；失败不能伪装成成功，宽松模式必须显式启用并披露处理行为。
- 兼容性限定到上游版本、参数组合、输入范围及比较规则；通过有限用例不等于全面兼容。
- 性能结论必须附带同一输入上的正确性证明。解压后内容与压缩字节分开评价。
- 内存预算必须说明是算法缓冲预算还是进程 RSS 上限；流式读取不等于整个算法内存有界。
- 网络与安装权限开放；优先使用现有依赖，新增依赖记录理由、版本、来源和影响。
  使用 `--locked`；`--offline` 只用于有意隔离且缓存齐全的环境。
- 保留安全边界：不改 oracle/expected 文件、不修改 `rust-umitools/`（除非获授权）、
  不擅自发布、不打 tag、不擅自提交。已有 ADR 保留其明确范围。
- `rust-umitools` 的后续源码调整、发布及新的依赖政策由维护者另行明确授权。
- 保留开始工作前已有的 `packaging/conda/check_recipe.py` 与 `packaging/conda/meta.yaml` 修改。

## 2. 当前资产与成熟度

| 组件 | 当前能力 | 尚需补足 |
| --- | --- | --- |
| `bioinfo-common` | 退出码、诊断、摘要与来源信息 | 分清公共扩展契约和各上游兼容例外；资源策略尚未统一 |
| `bioinfo-fastx` | FASTX 解析、分块、压缩检测、gzip/BGZF/bzip2/xz/zstd | CLI 接入一致性、coverage-guided fuzz、分配测量；借用式 parser 的多行 FASTA 限制仍需明确 |
| `rust-cutadapt` | 广泛的 cutadapt 4.6 参数、单双端、过滤、报告、拆样 | cutadapt 5.x 未覆盖；压缩字节不等；完整差分需按发布流程重跑 |
| `rust-umitools` | 提取、白名单、去重、分组、计数、落盘与确定性测试 | 保持现有能力；本轮只验证，不修改 |
| `rust-fx` | stats/seq/fq2fa/fx2tab/sample/split/lint | SeqKit/fq 全子命令未覆盖；无实际并行；性能基线有限 |
| `rust-bam` | view/flagstat/idxstats/quickcheck/index/sort | 无 CRAM；sort 仍单线程；samtools 选项仅覆盖已记录子集 |
| `rust-fastqc` | 多项 QC 指标及 fastqc_data.txt、MultiQC 对接 | Adapter Content 为 experimental exact-match；Kmer/tile 缺失；无 HTML/zip；contaminant 仅 opt-in 前缀匹配 |

历史记录包括 cutadapt 119/119、UMI 77 个逐字节用例及 39 个语义用例、FASTX 50/50、
BAM 20/20、QC 22/22；来源为 `EVIDENCE-*.md`。这些是指定构建的记录，不是本轮重跑结论。
QC 的 Adapter Content 等缺失模块不在其差分比较范围内。

已有 SBOM、可复现构建、容器、Conda 和 aarch64 准备，不应重复列为“从零建设”。
正式分发配置目前只覆盖两个 x86_64 CLI；准备成功不等于全工具、多架构已交付。

## 3. 缺口登记

下表是修复前基线。实施后更新任务状态并链接新证据，不覆盖历史观测。

| ID | 优先级 | 缺口及证据 | 目标 |
| --- | --- | --- | --- |
| P0-01 | P0 | `rust-fx/src/io.rs` 的 `next_fastq` 将空行当 EOF；两条记录中间空行的探针 exit 0、只输出一条 | 非法中段输入明确失败；不静默丢数据 |
| P0-02 | P0 | `rust-fastqc/src/main.rs::accumulate` 使用 `std::fs::read`；plain 成功，gzip/bzip2/xz 探针均失败 | 接入共享流式解压 reader，端到端验证压缩与错误路径 |
| P0-03 | P0 | 当前跟踪的 workflow 只有 `.github/workflows/release.yml`，smoke 仅执行版本命令 | PR、定期及发布前三级门禁；缺 oracle 不算通过 |
| P0-04 | P0 | 旧规划、CLI help、README、证据记录的成熟度与当前实现有差异 | 当前能力矩阵、版本范围与实际命令同步 |
| P0-05 | P0 | cutadapt/UMI 有历史性能数据，三个新 CLI 未见统一性能基线 | 统一 benchmark schema/runner，先采基线再优化 |
| S-01 | P0 | `rust-bam::sort` 将全部记录收集进 Vec | 分块排序、受限路数归并、临时盘与故障清理 |
| S-02 | P0 | `rust-fx::split` 全量保存记录；stats 保存并复制每条长度 | 流式拆分、两遍扫描或外存；精确聚合统计 |
| S-03 | P0 | cutadapt `Stats::add` 累积拆样、辅助输出及逐条统计 | 输出增量写入、统计直方图、有限文件句柄和有序输出 |
| S-04 | P0 | QC 序列计数 map 随不同序列数增长，逐位统计随最长 read 增长 | 明确精确/兼容语义、外存或受限策略；不能静默改成近似值 |
| F-01 | P1 | BAM 无区域查询、常见 FLAG/MAPQ 过滤、CSI 和多线程 | 日常查询与过滤闭环，消费自己生成的索引 |
| F-02 | P1 | QC 无有效 Adapter Content、contaminant 匹配及完整报告产品 | 先补检测及 MultiQC，HTML 后置 |
| F-03 | P1 | 缺可运行的端到端示例、样本表及工作流消费测试 | 一条 UMI 工作流从 FASTQ 到报告/计数产物 |
| D-01 | P1 | 发布未绑定完整质量门禁，分发范围窄 | 本地候选产物、版本映射、SBOM、来源及安装验证；发布另批 |
| X-01 | P2 | 无通用区间处理/feature counting/稀疏矩阵契约 | 先形成表达与 UMI 分析闭环 |
| X-02 | P2 | 无 VCF/BCF 基础操作 | 按目标用户选择；先查询统计，复杂 norm 后置 |

### 修复前探针记录

在 `49953ec` 工作树构建三个新 CLI 后，对同一份两条记录输入运行 QC：

```text
QC plain.fq: exit=0; report=True
QC gzip.fq.gz: exit=1; report=False
QC bzip.fq.bz2: exit=1; report=False
QC xz.fq.xz: exit=1; report=False
FX interior blank: exit=0; output_records=1; stderr=''
```

这不是完整测试套件或大型 benchmark。每项修复必须有可执行回归用例，不能仅修改描述。

## 4. 分阶段实施

### A：数据完整性与可信基线（第 1–2 周）

| 任务 | 交付 | 验收 |
| --- | --- | --- |
| P0-01 | FASTX 空行/截断回归及修复 | 中段空行不能返回成功；有效输入无损；诊断可定位 |
| P0-02 | QC 流式解压及进程级回归 | plain 与支持的压缩格式得到相同报告（仅文件名可归一化）；损坏/截断非零退出；跨 chunk 记录正确 |
| P0-03 | monorepo PR CI 及统一检查入口 | fmt/clippy/test/关键 CLI；记录 oracle 缺失与失败，不允许假通过 |
| P0-04 | 当前能力与已知限制文档 | help/README/实现一致；保留历史证据，不伪装为当前结果 |
| P0-05 | benchmark schema 与 runner 初版 | 绑定输入、构建、环境和正确性证据，能重复运行 |

阶段 A 进展：规划落档、文档约束审计、P0-01–P0-05 均已完成（本地）；P0-03 的远端执行
仍待维护者配置。外部未验证项不暗示整个发布阶段已完成。P0-02 只消除全文件读取，
不据此宣称 QC 整体已有固定内存上限；S-04 已独立完成。

### B：规模化（第 3–6 周）

依赖 A 的正确性回归。执行 S-01 至 S-04，明确线程、缓冲、临时盘及文件句柄预算。
BAM 的同键排序规则、FASTX 精确分位数、QC 重复统计和 cutadapt 报告保持既定契约。

验收：

- 固定资源预算，输入按 1×/10×/100× 扩大，测量 RSS 与临时盘，不只看代码形式。
- 内存/落盘、不同 chunk、不同线程数的输出遵循相同确定性契约。
- 超预算显式失败，说明已有部分输出；禁止让失败产物被误认为完整成功。
- ENOSPC、低文件句柄上限、写失败、SIGPIPE/SIGINT、临时文件清理得到测试。
- 对超长单条记录、不同序列数爆炸等情况明确边界；“按最大记录增长”不能写成固定内存。

### C：性能与工作流（第 7–10 周）

依赖 A 的 runner 与 B 的资源行为。为五个 CLI 建立基线，再由 profiling 决定并行、
压缩、分配或算法优化。优先完成 F-01 的查询/过滤和 F-02 的 adapter 检测。

首条工作流：FASTQ → 按建库协议安排 UMI 提取与接头处理 → 外部比对器 → 排序/索引 →
UMI 去重或计数 → MultiQC/计数产物。UMI 提取与 trimming 的顺序由协议决定，不硬编码通用顺序。

交付样例数据、样本表、参数模板、环境/参考版本、预期产物以及现有工作流系统中的运行示例。
不自研工作流引擎；对工具缺口显式调用外部程序，不冒充已由本项目实现。

验收：同一输入的正确性证明、默认配置和同资源预算两组性能结果、完整工作流可复现。

### D：候选交付（第 11–12 周）

依赖前述门禁。为每个 CLI 声明稳定/实验状态；扩展现有打包验证而非重建重复系统。
本地验证容器、Conda、aarch64 及安装脚本的实际数据处理；QEMU smoke 不替代原生性能测量。
提供源码 commit、组件版本、分发版本、hash、兼容矩阵、SBOM 与构建来源的映射。
签名/attestation 属于交付准备，不进行上传或发布。

验收：安装后的产物通过真实输入 smoke，用户能定位限制及复现证据。是否公开发布由维护者另行批准。

### E：下一季度，选择一条分析主线

推荐：区间处理（BED/GTF/GFF，显式坐标约定）→ feature counting → 稀疏矩阵互操作。

- feature counting：与现有按 gene tag 的 UMI counting 区分；保留旧 W2-2 的
  union/intersection-strict/intersection-nonempty 与 nonunique 模式验收。
- 矩阵：保留旧 W2-3 的 Matrix Market、barcode/feature 顺序及 R/Python round-trip 验证。
- 若目标用户侧重变异分析，则优先 VCF/BCF 查询、统计和限定过滤；旧 W2-1 的
  norm/isec 等仍在 backlog，不能把它们默认为已有或取消。
- UMI `--output-stats`（旧 W2-4）：在授权后实现完整 null model 并对照，或继续明确拒绝；禁止占位值。
- consensus/duplex（旧 W2-5）：单独命令与 oracle，不混入 UMI-tools 兼容承诺；受当前修改限制约束。

不同时铺开表达和变异两条大主线。近期不自研比对器、变异检测器、GUI、分布式引擎；
没有 profiling 依据不启动 GPU 路线。

## 5. 统一质量与性能标准

### 正确性和兼容性

1. 固定并检查 oracle 版本；测试输入固定 hash 或可由 seed 重建。
2. 区分解压字节、字段、数值容差与语义不变量；归一化规则逐项命名。
3. 兼容性证据绑定 commit、dirty 状态、二进制 hash、命令和结果；缺失 oracle 不计为通过。
4. 保留测试框架的负向控制，确保错误结果能被检测；新增修复先跑失败回归，再跑修复后通过。
5. 确定性 fuzz harness 保留；coverage-guided fuzz、每 parser 至少 10 分钟和分配测量仍是待办。
6. 共享库不打印、不退出；上游不同行为通过 CLI 适配层保留，不强行统一退出码或 stdout/stderr。
7. unsupported 在可预先判断时写前拒绝；流处理中才发现的错误非零退出并说明部分输出政策。

### 性能

每个 benchmark 记录：命令、工具/oracle 版本、commit、binary/input SHA256、CPU/内存/系统、
线程数、压缩级别、重复次数、wall/CPU、RSS、临时盘、输出体积、原始计时及同输入正确性结果。

同时报告默认配置体验与同资源预算对比。覆盖短/长读长、单双端、高重复/高复杂度、压缩格式，
标明冷/热缓存和共享机器负载。固定 runner 上超过 10% 的退化触发复核，避免将噪声直接判为代码回归。
旧 W2-6 提前为 P0-05，不再等新分析模块完成才建性能体系。

cutadapt 的历史 6.60× 使用默认四 worker；单线程历史值约 1.89×。
UMI 的历史 15.49× 仅属于记录的输入与构建。两者均不能作为当前全工具包的性能承诺。

### 持续集成与发布门禁

| 层级 | 检查 |
| --- | --- |
| PR | fmt、clippy、单元/集成、关键 CLI 回归、可用的固定 oracle 差分 |
| 定期 | 全 oracle、随机差分、coverage-guided fuzz、内存/落盘/确定性与性能趋势 |
| 候选发布 | 上述证据、MSRV/主工具链、各目标平台、实际打包产物 smoke、SBOM、版本/hash 映射 |

本地基础检查（不触发发布；默认可联网；隔离环境可加 `--offline`）：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --release --locked
(cd rust-cutadapt && python3 tests/check.py --bin ../target/release/rust-cutadapt)
```

统一入口现为 `bash tests/ci/check.sh`；加 `--with-fastx-qc-oracles` 启用已集成的 FASTX/QC 对照，
加 `--offline` 进入隔离模式。
详细前提、范围和远端限制见 [CI 说明](../tests/ci/README.md)。

完整差分检查还包括既有 cutadapt、UMI、FASTX、BAM、QC suites；运行哪些必须逐项记录。
普通单元测试通过不替代上游差分或规模化验收。

## 6. 当前执行看板

| ID | 状态 | 下一步/证据 |
| --- | --- | --- |
| PLAN | 已落档 | 本文及旧规划存档；其他阶段不标为完成 |
| P0-01 | 已完成（本地验证） | 空行不再静默截断；5 项进程级回归、既有 FASTX 50/50；见执行证据 |
| P0-02 | 已完成（输入路径） | 9 项进程级回归、QC 22/22 与 MultiQC；共享解压/分块已接入，S-04 已完成 |
| P0-03 | 已完成（本地 + 远端 CI） | `tests/ci/check.sh` 默认联网、保留 `--offline`/`--isolate-network`；12 项负向控制、文档检查、rustdoc gate 及 GitHub-hosted workflow 已就绪；quality run 40151959375 已远端通过；required checks、完整 oracle/定期/发布门禁仍待配置。证据：[EVIDENCE-P0-DOCS-CI](EVIDENCE-P0-DOCS-CI.md) |
| P0-04 | 已完成（文档审计与 2026-10-25 清理） | [DOC-AUDIT.md](DOC-AUDIT.md) 记录现行/历史/安全/受保护四类；2026-10-25 修正 `rust-umitools` MSRV/CI 文案、移除未使用 helper，并修复 rustdoc 链接；剩余 CLI help 与实现差异继续审计 |
| P0-05 | 已完成（本地） | `bench/run.py`、schema、五 CLI 1×/10×/100× 矩阵；见 `bench/results/current.json` |
| S-01 | 已完成（本地） | BAM 外部排序 + samtools 1.24 对照；见 `docs/EVIDENCE-FINAL.md` |
| S-02 | 已完成（本地） | FASTX 直方图 stats + 流式 split；174 lib + 4 streaming tests + 50/50 |
| S-03 | 已完成（本地） | adapter 长度直方图；单端 + paired aux/demux 增量写出；119/119 差分、K1/K4/full 通过 |
| S-04 | 已完成（本地） | QC `--max-read-length`/`--max-distinct-sequences`；11 回归 + 22/22 |
| F-01/F-02 | 已完成（本地） | BAM -f/-F/-q、--region（BAI/CSI）、index --csi、view -@ 多线程 BGZF、sort -@ 明确单线程；QC Adapter Content + opt-in contaminant；见 EVIDENCE-FINAL §8 |
| F-03 | 已完成（本地） | `examples/umi-workflow/` consumer smoke；aligner 缺失 exit 3 |
| D-01 | 本地完成 | `docs/DELIVERY.md`；SBOM/可复现/aarch64/配方结构 PASS；未发布 |
| T11 | 已完成（本地） | 规模矩阵 + docs/PROFILING.md；临时 run BGZF FAST，bam_sort lowmem 7.60→5.27s |
| T14 | 已完成（本地门禁；独立复核通过） | QUALITY PASS（718 tests, 50/50, 22/22）+ K4/K5/K6；自检前快照审计通过；2026-10-25 独立复核与 quality run 40151959375 通过 |
| C11 | 已完成（本地） | proptest 9、Miri 39、Kani 5、TLA+ 2；`FULL VERIFICATION PASS`；见 `verification/`、`EVIDENCE-FINAL.md` §7 |

## 7. 证据入口

- [第一阶段首批执行证据](EVIDENCE-P0-START.md) / [JSON](EVIDENCE-P0-START.json)：
  新增 14 项回归、workspace release 699 项通过及当前工作树 hash；不替代尚未运行的检查。
- [最终一致性快照](EVIDENCE-FINAL.md) 与 [JSON](EVIDENCE-FINAL.json)：最终门禁 718 tests、
  FASTX 50/50、QC 22/22；记录已完成项与外部未验证项。
- [本地候选交付准备](DELIVERY.md)：版本/SHA256/证据映射；SBOM、可复现构建、aarch64、
  配方结构检查；未发布。
- [五 CLI benchmark 原始结果](../bench/results/current.json) 与 [runner](../bench/run.py)。
- [UMI 工作流 consumer smoke](../examples/umi-workflow/README.md)。
- [文档审计与质量门禁证据](EVIDENCE-P0-DOCS-CI.md) / [JSON](EVIDENCE-P0-DOCS-CI.json)：
  现行约束修正、12 项负向控制、在线门禁与 FASTX/QC 对照；没有远端执行或发布声明。
- [旧质量门禁证据](EVIDENCE-P0-CI.md) / [JSON](EVIDENCE-P0-CI.json)：上一版离线门禁的历史记录；
  保留原文，当前状态以新证据为准。
- [文档约束审计](DOC-AUDIT.md)：现行规则、历史记录、安全边界和受保护组件的分类清单。
- [根 README](../README.md)：workspace 与分发边界。
- [历史规划](DEVELOPMENT-PLAN-LEGACY.md)：旧 W0/W1/W2、来源和原验收要求。
- [共享库证据](EVIDENCE-W1-2.md)、[FASTX 证据](EVIDENCE-W1-3.md)、
  [BAM 证据](EVIDENCE-W1-4.md)、[QC 证据](EVIDENCE-QC.md)：均按各自构建理解。
- [打包证据](EVIDENCE-PACKAGING.md)：已经完成的准备及其限制。
- [cutadapt 性能](../rust-cutadapt/README.md#performance)、
  [UMI 性能](../rust-umitools/docs/BENCHMARK.md)：历史工作负载和参数，不推广为全局保证。
