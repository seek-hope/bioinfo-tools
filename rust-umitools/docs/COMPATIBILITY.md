# 兼容性矩阵

对照基线：**UMI-tools 1.1.6**（源码提交 `3f2cb4f41e07fe4a6cea3b3389f534bee957176b`）。
本实现版本：0.2.0。

状态含义：

| 状态 | 含义 |
|---|---|
| `verified` | 有可执行对照证据（oracle、exact、格式或 spill 套件），结果一致 |
| `experimental` | 已实现，但只有部分上游对照，或仍有明确差异 |
| `unsupported` | 创建任何输出文件之前明确拒绝 |

机器可读的逐项状态见 [`compatibility.tsv`](compatibility.tsv)（由 `tests/compat/matrix.py` 生成，`tests/ci/check_manifest.sh` 校验同步）。

复现全部 `verified` 结论：

```bash
bash tests/oracle/run_oracle.sh      # 39 项语义对照
bash tests/oracle/run_exact.sh       # 77 项逐字节对照
bash tests/oracle/run_spill.sh       # 有界内存路径与内存路径一致
bash tests/oracle/run_extract.sh     # extract 11 + whitelist 9
bash tests/oracle/run_formats.sh     # stdin/stdout、CRAM、摘要
```

## dedup

| 能力 | 参数 | 状态 | 证据 |
|---|---|---|---|
| 五种归并方法 | `--method=unique\|percentile\|cluster\|adjacency\|directional` | `verified` | oracle C2 + exact 用例 |
| 距离阈值 | `--edit-distance-threshold=0,1,2,≥长度` | `verified` | exact 用例 |
| 单端 / 双端 | `--paired` | `verified` | exact `paired-*` 与 6 个 `paired-repeat`；oracle C8 |
| UMI 来源 | `read_id`、`tag`、`umis` | `verified` | exact `tag-source`、`umis source`；后两者均有上游逐字节证据 |
| 固定 seed 常规路径 | `--random-seed` | `verified` | 全部 exact + 全量 small.bam 记录摘要；gene-map 例外见下 |
| 默认坐标排序 / 保留顺序 | `--no-sort-output` | `verified` | oracle B04 |
| 代表 read | `--representative=upstream` | `verified` | oracle C3；`stable` 为扩展（`experimental`） |
| MAPQ 过滤 | `--mapping-quality` | `verified` | exact `mapq filter` |
| 多重比对检测 | `--multimapping-detection-method=NH\|X0\|XT` | `verified` | exact `detection NH/X0/XT`（含替换后的 RNG 消耗） |
| 分组维度 | `--spliced-is-unique`、`--ignore-tlen`、`--read-length` | `verified` | exact `spliced`、`ignore-tlen`、`read-length` |
| 抽样 | `--subset P` | `verified` | exact `subset`（与平局共享 RNG） |
| 逐基因 / 逐细胞 | `--per-gene --gene-tag`、`--per-cell --cell-tag` | `verified` | exact `per-gene`、`per-cell` |
| gene/transcript 聚合 | `--per-gene --per-contig [--gene-transcript-map]` | `experimental` | 已写 MC tag；上游 contig set 顺序受 `PYTHONHASHSEED` 影响，代表 read 可能不同；五次探针后接受该限制 |
| 单 contig | `--chrom NAME` | `verified` | exact `--chrom`；未知名写前拒绝 |
| 有界内存 | `--memory-limit`、`--partition-genes`、磁盘邻居 | `verified` | K9 强制路径与内存路径逐字节一致 |
| 统计输出 | `--output-stats` | `verified` | unique/directional/cluster/adjacency/percentile 逐字节一致；含多 bundle 与 NumPy null 模型 |
| 白名单过滤 / 整 contig 缓冲 | `--filter-umi --umi-whitelist`（含 `--umi-whitelist-paired`）、`--buffer-whole-contig` | `verified` | exact 单端/配对白名单、soft-clip 窗口 |
| 头部溯源 | `--add-pg` | 扩展 | 默认不写；显式开启会改变输出字节 |

## group / count / count_tab

| 能力 | 状态 | 证据 |
|---|---|---|
| `group --group-out` 成员、代表 UMI、组计数、表头 | `verified` | exact 5×2 + oracle C2 |
| `group` 的 UG/BX 标签、`--output-bam` | `verified` | exact 逐字节对比 |
| `group --per-gene`、`--per-gene --per-cell` | `verified` | exact `group --per-gene`（BAM + TSV） |
| `count --per-gene --gene-tag` | `verified` | exact `count --per-gene` |
| `count --per-cell`（长表稀疏三元组） | `verified` | exact `count --per-cell` |
| `count --wide`（稠密 gene×cell） | `verified` | exact `count --wide` |
| `count --per-contig` | `verified` | exact `count --per-contig` |
| `count --per-contig --gene-transcript-map` | `verified` | exact `count --gene-transcript-map` |
| `count_tab` / `--per-cell` | `verified` | exact `count_tab` |

## extract / whitelist

| 能力 | 状态 | 证据 |
|---|---|---|
| `--extract-method=string`（`-p`、5′/3′、配对、gzip、质量过滤/掩码） | `verified` | K10 与上游逐字节对比 |
| `--extract-method=regex`（`umi_*`/`discard_*` 命名分组） | `verified` | K10 `regex extraction` |
| `--quality-encoding phred64/solexa`（quality filter/mask offset） | `verified` | extract oracle；phred64 过滤逐字节一致 |
| `--extract-method` 其他值、缺 `umi_` 分组 | `verified` | 与上游一致，写前 exit 2 |
| `whitelist --knee-method=distance\|density`、`--set-cell-number` | `verified` | K10 五组 TSV 逐字节一致 |
| `whitelist --ed-above-threshold=discard\|correct` | `verified` | K10 `correct/discard above threshold` |
| `whitelist --method=umis`、`--read2-in [--bc-pattern2]` | `verified` | whitelist oracle；单端 UMI 计数与配对 pattern2 逐字节一致 |

## 格式与管道

| 能力 | 状态 | 证据 |
|---|---|---|
| `-I -` / `-S -`（stdin/stdout，两遍处理经 spool） | `verified` | K11 字节一致；SIGPIPE 不挂起 |
| CRAM 读写（`--reference`） | `verified` | K11 核心字段一致；缺参考写前拒绝 |
| 摘要 JSON | `verified` | K11 schema 校验 |

## 剩余差异（不可对齐或有意的行为）

- 默认 BGZF 压缩级别为 1（上游为 6）；`--compresslevel 6` 可恢复接近上游的体积，解压后内容不变。

1. **adjacency 的非代表成员顺序**：上游用 Python `set` 迭代顺序，依赖 `PYTHONHASHSEED`，上游自身跨运行也无法保证；本实现按计数降序、再按 UMI 字节排序，保证可复现。成员集合与代表一致；只有大组时 TSV 行序可能不同，不影响 dedup 结果。
2. **未指定 seed 时的随机性**：上游用 `os.urandom`；本实现默认 seed=0。
3. **压缩后的字节**：解压后头部与记录流逐字节一致；压缩字节可能不同。
4. **`--add-pg`**：默认不写自有 `@PG`。
5. **输入排序校验**：本实现校验 `(tid, pos)` 单调，未排序报 exit 3；上游不校验。
6. **混合 UMI 长度**：本实现 exit 3 并给出可操作信息。
7. **退出码与错误文本**：定义 0/2/3/4 契约。
8. **CRAM 派生标签**：noodles 解码可能补出 `MD/NM`，按核心字段比较。
9. **有界内存路径的机制**：bundle 窗口在位置边界提前结束（无需持久化状态回放）；mate 请求/gene 聚合/超大 bundle 的 pair 索引与 CSR 落盘，结果与内存路径逐字节一致。
10. **单个超大 gene**：gene 分区后仍可能超出预算，报 exit 4（不 OOM）。
11. **gene-map 代表 read**：上游按 Python `set` 遍历 contig，`PYTHONHASHSEED` 会改变 RNG 消耗顺序；固定 hash seed 可重复，但不把跨 hash seed 的逐字节一致作为目标。

## 资源行为

| 项目 | 行为 |
|---|---|
| 排序内存策略 | chunk 取 `memory/32`，上限 `memory/4`、cache×8、tmp free/8（下界 1MiB）；最小预算 64MiB |
| bundle 窗口 | 1000 bp 释放规则；位置边界提前结束以保持有界 |
| mate 请求 | 超限落盘，按块重扫 join；排序输出一致，`--no-sort-output` 组合拒绝 |
| gene 聚合 | 超预算自动按 gene 哈希分区（`--partition-genes`） |
| 超大 bundle | 分块精确邻居 + 磁盘 pair/CSR；`RUST_UMITOOLS_DISK_UMIS` 控制启用阈值 |
| 线程预算 | 聚类由 `--threads` 控制；BGZF writer 用其一半；归并每 run 单线程、fan-in 32 |
| 临时盘 | 仅在需要 spill 时检查可用空间，不足 exit 4 |
| 确定性 | threads 1/2/8/16、spill、多轮归并输出逐字节一致 |
