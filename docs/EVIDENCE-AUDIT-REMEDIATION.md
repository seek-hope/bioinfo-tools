# 审计遗留项整改记录

日期：2026-10-23。性质：当前工作树的本地整改与复核，**不是新的独立审计**。
未提交、发布、打 tag 或重录 oracle；没有修改依赖、Cargo.lock 或 `rust-umitools/`。此边界仅适用于本文件记录的自检/整改快照；2026-10-23 用户另行授权 BAM I/O 迁移到 noodles，见 [EVIDENCE-NOODLES.md](EVIDENCE-NOODLES.md)。

## 1. 安全入口

`tests/gate/acceptance.sh` 的自动删除和两次录制路径已删除：

- 无参数、`-h`、`--help`：只显示帮助。
- `--verify`：委托既有 `m3.sh`，保留环境覆盖和原退出码。
- `--record`、未知/空参数、多个参数：在运行工具前 exit 2。
- 正常验证没有任何重新生成 golden 的入口。

`tests/gate/test_acceptance.py` 在临时 workspace 测试以上行为、golden 内容不变、
任意工作目录、环境覆盖、失败传播和 verifier 缺失；已接入 `tests/ci/check.sh`。
比较规则、40 个 golden JSON 和 cutadapt fixtures 均未修改。

真实命令输出摘录：

```text
$ python3 tests/gate/test_acceptance.py
Ran 9 tests in 0.070s
OK
$ python3 tests/ci/test_check.py
Ran 12 tests in 0.806s
OK
$ bash tests/gate/acceptance.sh --verify  # 本轮传入插桩二进制
10 criterion/criteria: 10 passed, 0 failed
M3 PASS
$ bash tests/ci/check.sh --offline --with-fastx-qc-oracles
all visible conformance cases passed
50/50 cases passed
22/22 fixtures passed
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
```

K1 包含 fmt 检查、workspace clippy `-D warnings`、release tests（718 passed）、
release build 和 cutadapt `tests/check.py`。修改的 shell 脚本通过 `bash -n`、
`shellcheck -S warning`；两个 workflow 通过 actionlint，均 exit 0。

## 2. 依赖策略：已批准；advisory 不豁免

2026-10-23 维护者明确批准了许可证 allow-list 与 crates.io-only 来源策略，
并将草案启用为根 `deny.toml`。决策记录见
[tests/security/README.md](../tests/security/README.md)；不再是“待审批草案”。

- 精确 allow-list：`MIT`、`Apache-2.0`、`BSD-3-Clause`、`ISC`、`Unicode-3.0`、
  `Zlib`、`bzip2-1.0.6`。
- 无 blanket allow、clarification 或包级例外；未声明/不在名单中的许可证仍拒绝。
- `MIT OR Apache-2.0 OR LGPL-2.1-or-later` 通过 MIT/Apache 分支满足，不额外放行 LGPL。
- 依赖来源仅 crates.io；未知 registry/git 来源拒绝。
- `[advisories].ignore = []`；没有临时豁免。

```text
$ cargo deny --offline --locked check licenses sources
licenses ok
sources ok
# exit 0
$ cargo deny --offline --locked check advisories
error[unmaintained]: custom_derive crate is unmaintained
advisories FAILED
# exit 1（预期；未添加 ignore）
$ cargo deny --offline --locked check
advisories FAILED, bans ok, licenses ok, sources ok
# exit 1；六条重复版本 warning 仍然显示
```

`custom_derive` 的用途与风险（TeamAgents 复核确认）：

- 它是 `rust-htslib` 的编译期 `macro_rules!` 辅助 crate，用于为 newtype
  包装器生成 `NewtypeFrom`/`NewtypeDeref`/`NewtypeDerefMut`/`NewtypeIndex`/
  `NewtypeIndexMut` 等 trait impl；不是 BAM/BCF 运行时解析器。
- 调用点在 `rust-htslib-0.49.0/src/bam/record.rs:1741`、
  `src/bcf/record.rs:1220`、`src/bcf/header.rs:49`；它生成 `CigarString`、
  `Genotype`、`Id` 等类型的便捷实现。
- crate 源码是 `macro_rules!`，无 `build.rs`、无 proc-macro、无 `unsafe`；
  因此风险是**停止维护的编译期代码生成**，不是已知的运行时漏洞或数据路径暴露。
- `rust-htslib 1.0.1` 仍声明 `custom_derive = "0.1"`，简单升级不能消除告警。
  同族的 `newtype_derive` 也没有 advisory，不能把“移除 custom_derive”误当成
  “整个宏家族已清理”。
- 维护者决定不豁免，因此 advisory gate 保持失败；需要单独安排依赖迁移/维护
  评估，不能通过 ignore 或放宽 allow-list 来变绿。

本轮扫描只使用本地缓存，无网络更新；`cargo audit` 也未检查 yanked 状态。
“未检出漏洞”仅针对该数据库快照。

- Cargo.lock SHA-256：`f6a95b462b2bffa326810e451788214d59d6c1dbaad5dd41de18044297edf8bc`。
- RustSec 本地 DB commit：`f7b2f6bc663452200bbe639ebb75e0c762b3422b`。
- cargo-audit：0.22.2；cargo-deny：0.20.2。
- CI `dependency-policy` 运行 license/source 检查；advisory 步骤是阻断型，
  在依赖迁移前远端 CI 会保持红色；这不是发布豁免。

## 3. 分支覆盖率与外部 oracle

工具：`nightly-2026-05-28`（已装 llvm-tools）、cargo-llvm-cov 0.9.1。
独立 debug build 目录：`target/audit-branch/llvm-cov-target`。没有更改正式 release 二进制。

| 范围/指标 | Rust tests only | 同一插桩构建 + UMI exact + M3 |
| --- | --- | --- |
| Regions | 29057/39913 = 72.80% | 33345/39913 = 83.54% |
| Functions | 1748/2444 = 71.52% | 1949/2444 = 79.75% |
| Lines | 17772/24177 = 73.51% | 20489/24177 = 84.75% |
| Branches | 1778/3286 = 54.11% | 2228/3286 = 67.80% |
| UMI lines | 2226/6212 = 35.83% | 4492/6212 = 72.31% |
| UMI branches | 216/1020 = 21.18% | 571/1020 = 55.98% |

更正：Python oracle 可以贡献 Rust 覆盖率，前提是调用插桩二进制并继承
`LLVM_PROFILE_FILE`。旧报告只是未将这些 workload 纳入采样，不是 Python 技术限制。

复现（要求已安装固定 nightly 和 oracle；UMI-tools 1.1.6、samtools 1.24 在 PATH）：

```bash
export CARGO_NET_OFFLINE=true
export PYTHONDONTWRITEBYTECODE=1
export CARGO_TARGET_DIR="$PWD/target/audit-branch"
cargo +nightly-2026-05-28 llvm-cov --workspace --tests --branch --offline --locked \
  --json --summary-only --output-path /tmp/branch-unit.json

# exact.py 固定寻找 <target>/release/rust-umitools。
# 临时软链接指向 debug 插桩构建，不改脚本或正式二进制，也不冒称为 release。
cov="$CARGO_TARGET_DIR/llvm-cov-target"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/release"
ln -s "$cov/debug/rust-umitools" "$scratch/release/rust-umitools"
export LLVM_PROFILE_FILE="$cov/bioinfo-tools-%p-%m.profraw"
CARGO_TARGET_DIR="$scratch" python3 rust-umitools/tests/oracle/exact.py
CUTADAPT_BIN="$cov/debug/rust-cutadapt" UMITOOLS_BIN="$cov/debug/rust-umitools" \
  bash tests/gate/acceptance.sh --verify
cargo +nightly-2026-05-28 llvm-cov report --branch --json --summary-only \
  --output-path /tmp/branch-with-oracles.json
```

输出：`77/77 byte-identical, 0 differ`；`10 criterion/criteria: 10 passed, 0 failed`；
`M3 PASS`。报告 JSON 和日志当前位于 `/tmp/bioinfo-remediation/`（临时路径，
非长期归档）；上表保留计数，便于复算。

限制：nightly branch instrumentation 是实验功能；并非 MC/DC、全分支测试或形式化证明。
未纳入 C/C++ FFI 和其余 Python oracle suites；与旧 stable/release 的 73.47%
不能直接作前后比较。未设未经决策的覆盖率门槛。

## 4. 受保护路径与历史更正

运行前后比较 UMI 的既有 git diff/status 哈希，以及 golden/fixtures 的每文件 SHA-256：

```text
UMI pre-existing diff/status unchanged
Oracle file hashes unchanged
```

`rust-umitools/` 起始状态已有三个修改：`tests/compat/matrix.py`、
`tests/ci/check.sh`、`tests/ci/env.sh`。本次未修改、还原或追认额外授权；
它不是 clean。旧回复“这三项已全部写入 DOC-AUDIT”并不准确，本次才补齐记录。
保留用户既有 packaging 修改。

## 5. TeamAgents 独立审计与后续修复

TeamAgents 0.2.0 的一次性只读审计于 2026-10-23 完成，结果为
**PASS WITH FINDINGS**。报告归档在
[EVIDENCE-AUDIT-TEAMAGENTS.md](EVIDENCE-AUDIT-TEAMAGENTS.md)。两个被委派的
reviewer 因沙箱无 shell 而 blocked，所有 findings 由 leader 自行复核；这不冒充
外部认证审计。

已据此修复：

- 40 个 golden 增加 `tests/gate/golden.sha256`，`m3.sh` 在验证前校验；
- `snapshot.py --record` 需要 `--confirm-record`，README 不再把它当常规路径；
- `acceptance.sh` 无参数 exit 2，`--help` exit 0，拒绝 `GOLDEN_DIR`/`GOLDEN_MANIFEST`
  重定向；
- 测试断言精确 argv（`m3.sh` 无参数）、退出码 127 和 record 确认；
- 根 `deny.toml`、审批记录、CI license/source 检查已落盘；advisory 保持失败；
- `EVIDENCE-FINAL.json` 明确区分自检前审计与当前工作树审计。

仍未关闭：`custom_derive`/`newtype_derive` 的依赖迁移；`multiple-versions=warn`
的六组重复版本；远端 CI 未在本环境执行；发布/签名未做。
