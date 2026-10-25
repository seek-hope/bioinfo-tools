# 文档约束审计

状态：本轮审计结论。分类为四项，修改只作用于“现行约束”和“历史标注”；
`rust-umitools/` 运行时代码在此前审计快照中保持原样；2026-10-23 用户另行授权 BAM I/O 迁移到 noodles，当前运行时已改变（见 [迁移证据](EVIDENCE-NOODLES.md)），oracle/expected 文件仍未修改。此前按用户“全部解决”请求修复了 `tests/compat/matrix.py` 的尾随 tab 生成器并重新生成 `docs/compatibility.tsv`；后续 shellcheck 处理还遗留了 `tests/ci/check.sh` 和 `tests/ci/env.sh` 的注解/source 指示修改，旧记录遗漏了这两项。此次补记不追认额外授权；这些既有改动仍需维护者复核。本次整改未修改或还原 `rust-umitools/`，其既有 diff/status 的前后哈希一致。

最终独立审计：2026-10-23 通过（对象为自检前快照）；随后全局自检的代码改动仅经本地 K1/K4 复核。快照见 [EVIDENCE-FINAL.md](EVIDENCE-FINAL.md)。安全入口、依赖策略草案和分支覆盖率补测见 [整改证据](EVIDENCE-AUDIT-REMEDIATION.md)；它们是本地复核，不是新的独立审计。

## 1. 现行约束：已修正

| 文件 | 过时或不合理内容 | 修正 |
| --- | --- | --- |
| `AGENTS.md` | “No shell network access”、必须 `--offline`、仅 `clap`+`flate2`、单端试验范围、12 小时试运行限制 | 改为当前 monorepo 规则：允许联网/安装，`--locked` 为默认，`--offline` 仅用于隔离；保留依赖审阅、streaming、严格失败、发布审批边界。旧试验限制明确指向 `TASK.md` 历史文件 |
| `TASK.md` | 将 v0.1.0 单端子集、禁止联网、仅 `clap`+`flate2` 写成任务要求 | 顶部加历史试运行说明；正文保留作为来源记录，并链接当前 README/FULL-COMPATIBILITY |
| `README.md` | 构建命令强制 `--offline`；称门禁为 “offline gate”；把离线 runner 当作前提 | 改为 `--locked`；说明网络/安装可用、`--offline` 可选；门禁改称 quality gate；链接当前 CI 说明 |
| `docs/DEVELOPMENT-PLAN.md` | “离线构建、不下载依赖”被写成不可解除的约束；检查命令强制 `--offline`；P0-03 状态仍是“离线门禁” | 改为网络/安装开放、保留安全边界；更新命令、状态和证据链接；增加文档审计入口 |
| `tests/ci/PINS.md` | “Shell network access is forbidden”；安装说明假定必须人工放置 | 说明允许联网/CI/脚本安装；门禁负责校验版本、路径和已记录校验和 |
| `tests/ci/PINS-QC.md` | 同上 | 同上 |
| `rust-cutadapt/README.md` | Rust 1.88、强制离线、把旧提交 `c8b0249` 当作当前树 | 改为 workspace MSRV 1.91；命令使用 `--locked`；明确快照提交与当前 HEAD 的区别 |
| `rust-cutadapt/docs/adr/0001-dependency-policy.md` | 要求离线构建；MSRV 仍写 1.88 | 改为允许联网、离线为可选；MSRV 1.91 |
| `tests/ci/check.sh` | 无条件 `CARGO_NET_OFFLINE=true`、禁止自动安装工具链、oracle 强制 `unshare -Urn` | 默认联网；新增 `--offline` 与 `--isolate-network`；仅在隔离时设置离线/命名空间；工具链缺失时在线安装、离线明确失败 |
| `tests/ci/test_check.py` | 只验证离线路径 | 增加默认在线、`--offline`、工具链安装/缺失和 oracle 隔离的负向控制 |
| `.github/workflows/quality.yml` | 使用 `self-hosted` 且标注 `bioinfo-tools-offline`，只跑离线 | 改为 GitHub-hosted `ubuntu-latest`，允许网络；安装固定工具链与系统依赖；保留可选 oracle 任务并明确需要预装 |
| `tests/ci/README.md` | 以 “Offline quality gate” 为标题和前提 | 改为 quality gate：默认可联网，`--offline` 可选，说明 runner/安全边界和未执行事项 |

## 2. 历史记录：保留原文并加标识

以下文件记录的是特定 commit/环境下的运行结果。`--offline`、旧版本和旧 runner
是当时的环境选择，不是当前规则；正文结果不重写。

| 文件 | 处理 |
| --- | --- |
| `docs/EVIDENCE-2026-10.md`、`docs/EVIDENCE-PACKAGING.md`、`docs/EVIDENCE-QC.md`、`docs/EVIDENCE-W1-1.md` 至 `W1-4.md` | 顶部加 “Historical record” 说明 |
| `rust-cutadapt/docs/EVIDENCE-2026-10.md` | 顶部加历史说明 |
| `docs/DEVELOPMENT-PLAN-LEGACY.md` | 已有 historical banner；保留旧任务 ID、来源和旧验收标准 |
| `docs/EVIDENCE-P0-START.md`、`docs/EVIDENCE-P0-CI.md` | 本批执行记录；注明当时选择离线命令，但离线不是项目要求 |
| `rust-cutadapt/docs/HISTORY.md`、`rust-cutadapt/docs/RELEASE-FREEZE.md` | 已明确为历史/已取代；不改写历史结论 |

## 3. 安全或发布边界：保留

这些不是过时的技术约束，而是当前仍需保留的边界：

- 不擅自发布、打 tag 或提交；发布需要维护者批准和完整证据。
- 不修改 `rust-cutadapt/tests/fixtures/`、expected FASTQ 或其他 oracle 判定文件。
- 不覆盖用户开始工作前已有的 `packaging/conda/check_recipe.py`、`packaging/conda/meta.yaml` 修改。
- 默认严格输入；失败不能伪装成成功；性能声明必须绑定同输入正确性。
- 流式读取不等于整个算法内存有界；必须说明逐位/逐序列结构的实际增长边界。

## 4. 受保护、只记录不修改

| 文件 | 发现 | 处置 |
| --- | --- | --- |
| `rust-umitools/README.md` | “需要 Rust 1.88+”，与 workspace MSRV 1.91 不一致 | 历史受保护记录；2026-10-23 迁移授权范围仅覆盖 BAM I/O，未顺手修改该 MSRV 文案，仍在待办 |
| `rust-umitools/README.en.md` | “Rust 1.88+ is required” | 同上 |
| `rust-umitools/tests/ci/env.sh`、`tests/ci/check.sh` | 默认 `CARGO_NET_OFFLINE=true`；另有上轮 shellcheck 注解/source 指示改动 | 隔离选择未改；如实补记既有测试脚本 diff，本次不再修改，待维护者复核 |
| `rust-umitools/tests/*` | 多个 oracle 脚本依赖既有环境/路径假设 | 受保护；只通过根门禁调用，不修改比较逻辑。例外：`tests/compat/matrix.py` 的尾随 tab 生成器已按用户授权修复，重新生成的 `docs/compatibility.tsv` 通过 manifest gate |
| `tests/oracle/run_w13_oracle.sh` | 注释仍说 oracle “cannot be obtained offline”；构建提示含 `--offline` | 属 oracle 脚本保护范围，保留；根门禁使用 `--rust` 传入当前构建，不依赖该提示 |
| `tests/oracle/run_qc_oracle.sh` | 构建提示含 `--offline` | 同上 |
| `packaging/repro-check.sh`、`packaging/cross-build-aarch64.sh`、`rust-cutadapt/tests/gate/m0.sh` | 脚本内使用 `--offline` 作为可复现/隔离手段 | 保留：离线是这些检查的实现选择，不是全项目前提 |
| `bioinfo-common/Cargo.toml` | 注释提到 `cargo --offline` | 属库设计说明，不影响现行构建；暂不改动 |

## 5. 后续仍需同步的项

- `rust-umitools` 文档中的 1.88/MSRV 和 CI 描述需要组件维护者授权后修正。
- `tests/oracle/run_w13_oracle.sh` 的过时注释需要在 oracle 脚本变更窗口内修正。
- 根 README 的组件版本、发布状态和性能声明在阶段 D 再统一生成，避免再次手写漂移。
- 文档检查只覆盖明确的过时约束，不替代对每条兼容性声明的逐项复核。

## 6. 2026-10-23 noodles migration

用户明确授权把 `rust-umitools` BAM I/O 从 rust-htslib 迁移到 noodles；运行时源码和 `Cargo.lock` 已改变。迁移证据、oracle 结果、性能与限制见 [EVIDENCE-NOODLES.md](EVIDENCE-NOODLES.md)；ADR 见 [0001-noodles-migration.md](../rust-umitools/docs/adr/0001-noodles-migration.md)。此前“运行时未改”的表述仅适用于旧审计快照。
