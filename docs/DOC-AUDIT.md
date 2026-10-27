# 文档与受保护文件分类

本文定义文档维护边界，避免把历史日志当成当前说明，也避免通过修改期望值来制造通过。
它不是某次运行的工作日志。

## 1. 现行约束

以下入口描述当前工作树，修改后必须与实现和可执行证据同步：

| 文件 | 用途 |
| --- | --- |
| `AGENTS.md` | monorepo 工作规则、依赖政策、资源与发布边界 |
| `README.md` | 组件、构建、文档和分发入口 |
| `docs/DEVELOPMENT-PLAN.md` | 当前优先级和批次退出条件 |
| `compat/README.md`、`compat/matrix.tsv` | 五个 CLI 对固定上游版本的支持状态 |
| `rust-umitools/docs/compatibility.tsv` | UMI-tools 组件矩阵，由 `tests/compat/matrix.py` 生成 |
| `tests/ci/README.md` | 质量门禁的运行方式和覆盖范围 |
| 各 crate README 与 CLI help | 用户可见的安装、参数和限制 |

文档中的版本、命令数量和测试数字应能由当前文件或命令复核。容易漂移的计数优先交给
生成器；历史数字必须标明对应构建，不能改写成“当前结果”。

## 2. 历史记录

`docs/EVIDENCE-*.md`、配套 JSON、`docs/DELIVERY.md`、
`docs/DEVELOPMENT-PLAN-LEGACY.md`、`rust-cutadapt/docs/HISTORY.md` 和各组件 benchmark
记录保留其原始结论。它们用于追溯，不用于证明当前工作树仍然通过同一检查。

维护规则：

- 不重写旧日志来匹配后来的实现；需要澄清时在文件顶部加历史说明或链接新证据。
- 旧性能倍数只适用于记录的输入、硬件、参数和二进制，不作为当前性能承诺。
- 历史文件中的旧命令、离线环境、HTSlib 或缺失功能描述保留其时代语境。
- 当前入口不得把历史快照中的 hash、测试总数或发布状态复制成现状。

## 3. 安全或发布边界

- 不擅自提交、打 tag、上传或发布；发布需要维护者批准和完整证据。
- 不修改 `tests/gate/golden`、`rust-cutadapt/tests/fixtures` 或其他 oracle 判定数据来消除失败。
- 纳入版本控制的期望输出和比较脚本只能在明确的契约变更窗口内更新，并保留独立的上游证据。
- 保留用户已有的 `packaging/conda/check_recipe.py` 和 `packaging/conda/meta.yaml` 修改。
- 性能结论必须绑定同输入正确性；失败后的部分输出不能描述为完整成功。
- 新增依赖需要记录理由、版本、来源和构建/运行影响。

## 4. 受保护组件

`rust-umitools/` 默认需要维护者明确授权才能修改。2026-10-26 用户先后批准了代码/文档整理和
help/provenance 契约刷新。契约变更只涉及本节约定的文件；其他 golden、fixtures 和 oracle
判定数据不改。

当前特别边界：

- `rust-umitools/src/cli.rs` 的 `--output-stats` help 已改为描述当前实验性统计表；
  `tests/cli_contract.rs` 与 `tests/cli_help/dedup.txt` 已在获批的 CLI 契约批次同步刷新。
- `rust-umitools/src/main.rs` 的 provenance/help 契约已于 2026-10-26 经维护者批准更新：
  字段改为 noodles，parallel/commands 与实现对齐，dedup help 快照同步刷新。
  后续契约变更仍需单独批准。
- `rust-umitools/tests/compat/matrix.py` 是组件矩阵的生成源。修改说明后必须重新生成
  `docs/compatibility.tsv` 并运行 manifest 检查，不能只改生成结果。
- `verification/kani` 中的 dead-code allow 属于证明 harness，不是生产代码豁免；生产 crate
  不允许用 `allow(dead_code)` 隐藏未使用实现。

## 5. 自动检查

`tests/ci/check_docs.py` 检查已退役约束、历史标识、部分陈旧声明、本地链接和受保护规则。
`tests/ci/check_compat.py` 检查统一矩阵、当前 help 和固定上游快照的同步。
`rust-umitools/tests/compat/matrix.py --check` 检查组件矩阵与生成源同步。

这些检查只能发现已编码的漂移，不能替代人工审读或上游差分。
