# 最终一致性检查与剩余工作

> **历史验证基线（2026-10-25）**：`41b17e3`；GitHub Actions quality run
> [`40151959375`](https://github.com/seek-hope/bioinfo-tools/actions/runs/40151959375)
> 的 `native` 与 `dependency-policy` 两个 job 均通过；`cargo deny check --all`
> 通过；SBOM 已重新生成；rustdoc `-D warnings` 已加入 native gate。以下第 1/4/5
> 节的 718 测试、日志和 SHA 表是 2026-10-23 快照，保留作历史。

状态：本轮本地实现与门禁已完成，包括 C5/S-03、C7/T10、C11 靶向验证；
独立审计已通过（2026-10-23，审计对象为自检前快照）。2026-10-23 另行完成 noodles BAM I/O 迁移，证据见 [EVIDENCE-NOODLES.md](EVIDENCE-NOODLES.md)。随后全局自检修改了 `Config::new` 的错误返回、压缩器状态错误、采样校验分支和 `forbid(unsafe_code)`；这些改动已由本轮 K1/K4 本地复核；2026-10-25 独立复核已通过。该快照没有发布、打 tag 或提交。
机器可读记录：[EVIDENCE-FINAL.json](EVIDENCE-FINAL.json)。TeamAgents 只读审计（PASS WITH FINDINGS，reviewer 沙箱受阻）见 [EVIDENCE-AUDIT-TEAMAGENTS.md](EVIDENCE-AUDIT-TEAMAGENTS.md)；其 findings 的修复记录见 [整改证据](EVIDENCE-AUDIT-REMEDIATION.md)。
后续安全入口整改、依赖策略待决事项及新覆盖率测量见 [整改证据](EVIDENCE-AUDIT-REMEDIATION.md)。该快照时 `cargo deny` 仍失败；2026-10-25 已通过，独立复核见 [EVIDENCE-AUDIT-NOODLES.md](EVIDENCE-AUDIT-NOODLES.md)。下述历史通过结论不覆盖后续事项。

## 1. 最终门禁

```text
$ bash tests/ci/check.sh --with-fastx-qc-oracles
Quality gate (online; host network): rustc 1.99.1 (58cf8f9c3 2026-10-13)
Oracle: seqkit v2.14.0
Oracle: fq v0.12.0 (51779f50e 2024-07-08)
Oracle: FastQC v0.13.0
Oracle: multiqc, version 1.35
PASS documentation constraints (18 current files, 8 historical files)
PASS rust-umitools protection documented (manifest generator exception recorded)
...
all visible conformance cases passed
50/50 cases passed
22/22 fixtures passed
PASS  multiqc            parses our fastqc_data.txt without warnings
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
```

- workspace release 测试：**718 passed, 0 failed**（按 `test result` 汇总）。
- `git diff --check`：无输出，exit 0。
- oracle fixtures 的 scoped git status 为空；noodles 迁移前的审计快照中 `rust-umitools/` 运行时代码未改；2026-10-23 用户授权迁移后运行时已改为 noodles，见 [EVIDENCE-NOODLES.md](EVIDENCE-NOODLES.md)。既有生成器改动及后来遗漏记录的 `tests/ci/check.sh`、`tests/ci/env.sh` shellcheck 改动见 `docs/DOC-AUDIT.md`；该目录不是 clean，本次整改保持其既有 diff/status 不变。
- 最终日志：`/tmp/bioinfo-final-gate.Vq9aJP.log`；T8 单端增量后的门禁日志为
`/tmp/bioinfo-gate-t8b.IBuBvI.log`；paired AuxSink 后的门禁日志为
`/tmp/bioinfo-paired.SZn0Ev.log`；T10 后门禁为 `/tmp/bioinfo-t10-final.1XJ8Xl.log`（临时文件，不是持久依赖）。

## 2. 已完成

| 项 | 证据 |
| --- | --- |
| 文档约束审计与修正 | `docs/DOC-AUDIT.md`；AGENTS/TASK/README/PLAN/PINS/rust-cutadapt 文档修正；历史 EVIDENCE 标识；`check_docs.py` PASS |
| 质量门禁 | 默认联网、`--offline`/`--isolate-network`；12 项负向控制；在线 FASTX/QC oracle；`quality.yml` hosted runner（该快照时未远端执行；后续 quality run 40151959375 通过） |
| S-01 BAM 外部排序 | 分块、BAM run + `u64` sidecar、受限归并、TempDir 清理；内存/外部逐字节一致；损坏输入/非法 size 回归；samtools 1.24 对照一致，quickcheck/index/idxstats 通过 |
| S-02 FASTX | `stats` 长度直方图精确 N50/四分位；`split` 两遍扫描流式写 part；174 lib + 4 streaming tests；FASTX 50/50 |
| S-03 cutadapt | `AdapterTotals` 直方图；单端与 paired R1/R2 aux/demux 在有序阶段按 chunk 增量写出；cutadapt 119/119 差分、全量测试、K1/K4/full 通过 |
| S-04 QC 资源上限 | `--max-read-length`、`--max-distinct-sequences`；11 项输入回归；QC 22/22 |
| P0-05/C6 benchmark | `bench/run.py` + schema + README；五 CLI 1×/10×/100×、2 repeats、21 results；命令/hash/硬件/RSS/temp/正确性 |
| F-03/C8 workflow | `examples/umi-workflow/`；合成 pre-aligned consumer smoke；trim→QC→sort/index→dedup→count 通过；外部 aligner 缺失 exit 3 |
| D/C9 本地交付准备 | `docs/DELIVERY.md`；SBOM PASS；cutadapt+rust-bam 可复现构建 IDENTICAL；aarch64 交叉+qemu PASS；Docker 镜像构建/运行+trim IDENTICAL；conda-build 实际构建+测试 PASS；UMI 全套 + BAM 20/20；版本/SHA256/证据映射 |
| 靶向验证 | proptest 9、Miri 39、Kani 5、TLA+ 2；`FULL VERIFICATION PASS`；覆盖矩阵与未覆盖范围见 [verification/README.md](../verification/README.md) |
| LLVM 覆盖率 | 历史 stable/release Rust 测试：region 72.74%、function 71.44%、line 73.47%，未测 branch；后续 nightly/debug + UMI exact/M3 插桩 oracle：line 84.75%、branch 67.80%，详见整改证据（不同工具链/profile 不直接比较） |

## 3. 已完成（本地验证）

- **C5/S-03**：QC S-04 显式上限；cutadapt adapter 长度直方图；单端与 paired aux/demux 按 chunk 增量写出；cutadapt 119/119、K1/K4/full 通过。
- **C7/T10**：BAM `-f/-F/-q`、`--region`（BAI/CSI）、`index --csi`、`view -@` 多线程 BGZF、`sort -@` 明确单线程；QC Adapter Content（experimental exact-match）与 opt-in contaminant。
- **T11**：规模矩阵完成；`docs/PROFILING.md` 记录 profiling 与临时 run BGZF FAST（7.60→5.27s，解压输出一致）。
- **T14**：本地门禁、证据更新与独立审计已完成；独立审计通过。

## 4. 外部未验证 / 未执行

1. **远端 GitHub workflow**：`quality.yml` 已配置，但本环境没有远端执行证据；required checks/runner 策略未配置。
2. **容器/Conda 发布级签名/上传**：Docker 镜像和 conda 包已在本轮构建/运行；未签名、未上传，未纳入发布。
3. **发布**：未签名、未打 tag、未上传；发布需维护者另行批准。

## 5. 当前工作树关键 SHA-256

| 文件 | SHA-256 |
| --- | --- |
| `rust-fx/src/commands.rs` | `caf70f39d1374d6d5a494392e4a0a531cb47cf786b8b7f01bf9d980a02b6a1dd` |
| `rust-fx/src/table.rs` | `7c8ff5e1ceb27404a20c7692a5b45b38f739faf6a688e57ddafbd7d306d352c6` |
| `rust-bam/src/commands.rs` | `088d359a9bf058a90ec48fb2fec2707e8d7be1b846600beb0f2846e09ca1920e` |
| `rust-bam/src/main.rs` | `6468bc2aec3ff8a0f41a8db8dfa6dfccd7e201b6b2606a0704481e07247ab17f` |
| `rust-fastqc/src/main.rs` | `955241e4ceef2ad24ac8de545f473d14b750b986c2f5803e29606723f1f6f479` |
| `bench/run.py` | `2f0eedb5961b9e2e179fc498a0369ac01ef294de37dd0e75c5beaf28428c287d` |
| `bench/results/current.json` | `2f4170efe6c3f856eed7181efce3ed8adaa11ab5ebf51c13a9589701d8584c47` |

## 6. 阶段 E 保留

区间处理（BED/GTF/GFF）→ feature counting → 稀疏矩阵互操作仍是推荐主线；
VCF/BCF 作为备选，UMI `--output-stats`、consensus/duplex 保留在 backlog。
本轮没有取消这些要求，也没有把规划当作实现。

## 7. 靶向验证组合（C11/T15/T16）

工具：proptest 1.12、Miri（nightly-2026-05-28，miri 0.1.0）、Kani 0.68.0/CBMC 6.11.0、
TLA+ TLC 2026.10.23。

```text
$ bash tests/verify/check.sh --fast
== proptest invariants ==
7 tests passed (chunker, parser, histogram, SortKey, PartPlan coverage, QC limits, AdapterTotals)
== Miri: bioinfo-common ==
39 passed
FAST VERIFICATION PASS (proptest + Miri bioinfo-common)

$ bash tests/verify/check.sh --full
5 Kani harnesses: VERIFICATION SUCCESSFUL
TLA+ CutadaptPipeline: Model checking completed. No error has been found. 8 states
TLA+ BamSort: Model checking completed. No error has been found. 11 states
FULL VERIFICATION PASS (proptest + Miri + Kani + TLA+)
```

Kani 的有界 harness：chunker 边界、`parts_for_size` 上界、SortKey 反对称、N50 上界、
空直方图。 K4 在 Miri 缺失时会安装固定 nightly 的 Miri
组件；安装失败仍 exit 3，不会跳过 Miri。TLA+ 模型带 `Done` stuttering，检查 `NoLoss`/`BoundedQueue` 与
`NoLoss`/`NoDuplicate`。

未覆盖：Kani/TLA+ 是固定小边界，不是全输入证明；Miri fast 只跑 `bioinfo-common`
（FASTX chunker 已手动跑过）。全项目形式化（Creusot/Verus 等）未尝试。

## 8. T10 BAM/QC 扩展

- BAM：`view -f/-F/-q`、`view --region`（BAI/CSI 交集查询）、`index --csi`、`view -@ N`（N>1 使用多线程 BGZF 压缩；SAM 输出仍单线程）、`sort -@ N`（仅接受 1，明确单线程）。
- QC：Adapter Content 使用 5 个已知 adapter 的精确匹配子集（experimental；不冒充 FastQC k-mer 语义）；`--contaminants FASTA` 为 opt-in，默认仍为 `No Hit`，保持 22/22 oracle。
- 测试：`rust-bam/tests/view_filters.rs` 6 项；`rust-fastqc/tests/input_integrity.rs` 13 项；QC 22/22 + MultiQC PASS；cutadapt 119/119；K1/K4 fast/full 通过。
- 未声称：Adapter Content 与 FastQC 数值逐项一致；contaminant 是精确前缀匹配而非 FastQC 的完整比对算法；`sort` 不是并行排序。
