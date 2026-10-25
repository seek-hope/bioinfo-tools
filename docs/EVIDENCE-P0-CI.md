# P0-03：统一离线门禁的本地验证（历史记录）

> Superseded for current status by [EVIDENCE-P0-DOCS-CI.md](EVIDENCE-P0-DOCS-CI.md).
> The gate now defaults to network-enabled operation; `--offline` is opt-in.

> Execution note: this batch used offline cargo commands by choice. Offline and network isolation are not current project requirements.

状态：统一入口、负向控制和 workflow 配置已实现并本地验证；**远端 CI 尚未执行**。
完整 P0-03 仍为部分完成：runner 注册、required checks、更多 oracle/定期检查及发布门禁尚未落地。

对应 [项目规划](DEVELOPMENT-PLAN.md)；操作说明见 [tests/ci/README.md](../tests/ci/README.md)。
机器可读记录：[EVIDENCE-P0-CI.json](EVIDENCE-P0-CI.json)。

## 变更

- `tests/ci/check.sh`：统一 native gate，严格读取主工具链 pin，强制 Cargo 离线且 locked。
  覆盖 fmt、clippy、workspace release tests、binary build 和 cutadapt visible smoke。
- `--with-fastx-qc-oracles`：显式启用 SeqKit/fq 与 FastQC/MultiQC 对照。四个 oracle 的版本先验证，
  缺工具/版本不符 exit 3；解析后的同一路径传给原 wrapper，不修改比较规则。
- oracle probe 和实际运行使用 `unshare -Urn`；禁用 MultiQC 联网版本检查。
- `tests/ci/test_check.py`：8 项 stdlib 测试，使用临时工具 shim 验证参数、缺工具链、版本错误、
  缺 oracle、oracle 版本错误、fmt/clippy 失败不会继续或假通过。
- `.github/workflows/quality.yml`：main push、同仓库 PR、手动触发；手动可启用 FASTX/QC oracle。
  read-only 权限、不持久化 checkout 凭证、不运行 fork PR、不请求发布 secret。

## 实际运行结果

基础 commit：`49953ec50ae04b343123a214b77030ff31f39a4b`，带首批修复和本批新增文件的未提交工作树。
主工具链 `rustc 1.99.1 (58cf8f9c3 2026-10-13)`。
没有 shell 下载，没有修改依赖、oracle、expected 文件或 `rust-umitools/`。

```text
$ bash -n tests/ci/check.sh
(exit 0; no output)

$ python3 tests/ci/test_check.py -v
Ran 8 tests in 0.149s
OK

$ bash tests/ci/check.sh
all visible conformance cases passed
OFFLINE QUALITY PASS (base only; external oracles NOT RUN)

$ bash tests/ci/check.sh --with-fastx-qc-oracles
Oracle: seqkit v2.14.0
Oracle: fq v0.12.0 (51779f50e 2024-07-08)
Oracle: FastQC v0.13.0
Oracle: multiqc, version 1.35
all visible conformance cases passed
50/50 cases passed
22/22 fixtures passed
PASS  multiqc            parses our fastqc_data.txt without warnings
SELFTEST PASS
OFFLINE QUALITY PASS (base + FASTX/QC oracles)
```

两次真实 gate 均 exit 0，包含 workspace release **699 项**通过。
以上仅摘录输出；不是用 shim 替代真实 gate。shim 只用于 `test_check.py` 的负向控制。
本地完整日志分别为 `/tmp/bioinfo-base-gate.9pgssu.log`、`/tmp/bioinfo-offline-gate.xqku7Z.log`；
临时日志不是仓库持久依赖，复现请运行上述命令。

另使用本机既有 MultiQC 环境的 PyYAML `BaseLoader` 检查 workflow YAML 的触发器、runner labels、
fork guard、权限、凭证设置及执行入口，输出：

```text
PASS workflow YAML structure (not a GitHub execution)
```

这是本地结构检查，不是 actionlint 或 GitHub 实际执行。`shellcheck` 未安装，未下载，也不宣称运行过。
`git diff --check` exit 0；保护路径 scoped git status 为空。

## 校验和

| 文件 | SHA-256 |
| --- | --- |
| `tests/ci/check.sh` | `c1ffb9ee0e820e57105370a898eb009e582f46cda33fd0dc165447f12a72c273` |
| `tests/ci/test_check.py` | `afa37d533ccc5a0663da9ee64973b5a9d676ab98bb4b25a69f3b7c4ea83299c2` |
| `.github/workflows/quality.yml` | `e04b15c9b0d5fec23efa7df80c29b410f95354026d4d6b7cdf4ff32f8999e0df` |

本批 workspace build 后 `rust-fx`、`rust-fastqc` hash 与 [首批证据](EVIDENCE-P0-START.md) 相同。

## 远端前提和剩余工作

workflow 使用 `self-hosted, linux, x64, bioinfo-tools-offline` runner labels。
没有查询或注册远端 runner，没有触发 GitHub workflow；如果没有相应 runner，任务会排队，不能视为通过。
维护者需预装 pin 对应的 Rust/rustfmt/clippy、Cargo.lock 所需离线缓存、系统编译依赖及可选 oracle，
建议使用一次性 runner。现有仓库规则不授权本轮联网配置这些资源。

full cutadapt differential、UMI exact/semantic/spill/determinism、BAM oracle、coverage-guided fuzz、
MSRV、性能趋势、跨平台产物和发布 gate 仍待集成。既有 `release.yml` 未修改、未触发。
因此本批不是“完整 CI 已上线”或“可发布”的证据。
