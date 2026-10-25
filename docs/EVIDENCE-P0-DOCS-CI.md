# 文档约束审计与质量门禁（阶段 A 本地证据）

状态：文档审计、现行约束修正、质量门禁改造和负向控制已完成本地验证。
远端 GitHub workflow 尚未执行；`rust-umitools/` 文档中的过期 MSRV/离线表述
只登记、未修改；基础门禁不替代完整 cutadapt/UMI/BAM 差分。

对应 [项目规划](DEVELOPMENT-PLAN.md)；分类清单见 [DOC-AUDIT.md](DOC-AUDIT.md)。
机器可读记录：[EVIDENCE-P0-DOCS-CI.json](EVIDENCE-P0-DOCS-CI.json)。

## 1. 本轮基线

| 项目 | 值 |
| --- | --- |
| 源码 commit | `49953ec50ae04b343123a214b77030ff31f39a4b` |
| 工作树 | 未提交；包含本批文档/门禁/前批修复 |
| 主工具链 | `rustc 1.99.1 (58cf8f9c3 2026-10-13)`，`cargo 1.99.1 (5f94df478 2026-08-27)` |
| 网络/安装 | 允许；本批门禁默认在线，保留 `--offline` 和 `--isolate-network` |
| 保护对象 | `rust-umitools/`、`rust-cutadapt/tests/fixtures/` 的 scoped git status 为空 |
| 既有修改 | `packaging/conda/check_recipe.py`、`packaging/conda/meta.yaml` 保留 |

## 2. 文档审计结论

完整分类见 [DOC-AUDIT.md](DOC-AUDIT.md)。摘要：

- **修正的现行约束**：`AGENTS.md`、`TASK.md`、根 `README.md`、
  `docs/DEVELOPMENT-PLAN.md`、`tests/ci/PINS*.md`、`rust-cutadapt/README.md`、
  `rust-cutadapt/docs/adr/0001-dependency-policy.md`、`tests/ci/check.sh`、
  `tests/ci/test_check.py`、`.github/workflows/quality.yml`、`tests/ci/README.md`。
- **历史记录**：`docs/EVIDENCE-*.md` 和 `rust-cutadapt/docs/EVIDENCE-2026-10.md`
  加历史说明；`TASK.md` 加历史试运行 banner。
- **保留的安全边界**：不擅自发布/tag/commit、保留 oracle/expected、
  保留既有 packaging 修改、默认严格失败。
- **受保护未修改**：`rust-umitools/README.md`、`README.en.md`、
  `rust-umitools/tests/ci/env.sh` 等；只记录在 `DOC-AUDIT.md`。

新增 `tests/ci/check_docs.py` 防止现行文档重新引入被排除的约束。

## 3. 质量门禁改造

- 默认允许联网和安装；`--offline` 才设置 `CARGO_NET_OFFLINE=true` 并给 Cargo 加
  `--offline`。
- 新增 `--isolate-network` 作为可选隔离；oracle 版本和运行都在该模式下才使用
  `unshare -Urn`。
- 工具链从 `rust-toolchain.toml` 读取；在线缺失时用 `rustup toolchain install`
  安装固定版本，离线缺失时 exit 3。
- oracle 版本先验证，路径一经解析即导出给 wrapper；缺工具/版本不符 exit 3。
- `.github/workflows/quality.yml` 改为 GitHub-hosted `ubuntu-latest`，安装固定
  Rust 和系统依赖；可选 oracle 步骤要求预装，缺工具明确失败。
- `tests/ci/test_check.py` 12 项负向控制覆盖：参数错误、缺工具链（在线安装/离线失败）、
  错误 Rust 版本、默认在线、`--offline` 标志、错误/缺失 oracle、隔离模式、
  fmt/clippy 失败不继续。

## 4. 实际命令与结果

```text
$ python3 tests/ci/check_docs.py
PASS documentation constraints (10 current files, 8 historical files)
PASS rust-umitools documented as protected/not modified

$ python3 tests/ci/test_check.py -v
Ran 12 tests in 0.387s
OK

$ bash tests/ci/check.sh --with-fastx-qc-oracles
Quality gate (online; host network): rustc 1.99.1 (58cf8f9c3 2026-10-13)
Oracle: seqkit v2.14.0
Oracle: fq v0.12.0 (51779f50e 2024-07-08)
Oracle: FastQC v0.13.0
Oracle: multiqc, version 1.35
PASS documentation constraints (10 current files, 8 historical files)
PASS rust-umitools documented as protected/not modified
... workspace release tests ...
all visible conformance cases passed
50/50 cases passed
22/22 fixtures passed
PASS  multiqc            parses our fastqc_data.txt without warnings
SELFTEST PASS
QUALITY PASS (base + FASTX/QC oracles)
```

- workspace release 测试按 `test result` 汇总：**699 passed, 0 failed**。
- 完整日志：`/tmp/bioinfo-quality-gate.3xs544.log`（临时文件，不是持久依赖）。
- `git diff --check` exit 0；保护路径 scoped git status 为空。
- 未运行：远端 GitHub workflow、完整 cutadapt 119 差分、UMI 全部 oracle、BAM 差分、
  coverage-guided fuzz、性能/规模矩阵。这些不计为通过。

## 5. 校验和

| 文件 | SHA-256 |
| --- | --- |
| `AGENTS.md` | `319731188ee86b153a1693ea121a786687e5f6134baf94f60a8bf780d7fd3015` |
| `TASK.md` | `d6fba4cfeefbd1ec25d9666920667f5d584da98fe38fee7a7f19ad9a5081b4c3` |
| `README.md` | `3ab7e057e646c6c354f03081874fbe67dbede91f280c011068d900d97f782206` |
| `docs/DEVELOPMENT-PLAN.md` | `76e42ae8e400ca8d5c327c7a2264b053f58b969b6cafb17aa84cca53dc24ed3c` |
| `docs/DOC-AUDIT.md` | `5a65586d2e88732667be522d4112938d8c26c99e198c5a9122f193ca40c3bae8` |
| `tests/ci/check.sh` | `6ec4c850a1178f44e2e23c14afe0d3424bcb890b29881bc74eef9ef8b4d5f2be` |
| `tests/ci/test_check.py` | `942b2ddc8037b8156234e560b9ebf4b3bff151e1f8976f8177fe3bf9adb3e0e7` |
| `tests/ci/check_docs.py` | `ef44ae1a2f6c1c65d2619b6b3a7441cbd987c14946ffdfd133e0a4d81f866b89` |
| `tests/ci/README.md` | `5b500ab7cff712143de784a640002dacd7e7135b4b22e84723addc34762ccc33` |
| `.github/workflows/quality.yml` | `ec774ea40b7375c7ae6f0be56a57f6e4fb38a7024c1aaafc14cc21a284cb213d` |
| `rust-cutadapt/README.md` | `3ec2c4a1ea10e60cc15a27491212e6235505c95596cd4e91b0892e7086b9a857` |
| `rust-cutadapt/docs/adr/0001-dependency-policy.md` | `9d6bec6a8d3e6900f2c059fb9c1ccd93a531afb46585bdf19d72532f5beb3087` |

## 6. 未完成/后续

- GitHub workflow 只做了本地 YAML/入口检查，未实际在 GitHub 执行；required checks
  和 runner 策略是维护者事项。
- 完整 cutadapt/UMI/BAM/QC 差分、coverage-guided fuzz、性能矩阵尚未接入该门禁。
- `rust-umitools` 组件文档中的 1.88 和离线默认值仍待授权后修正。
- 下一批：S-02 FASTX split/stats，S-01 BAM 外部排序，然后 S-03/S-04 和 P0-05。
