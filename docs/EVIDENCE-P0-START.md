# 第一阶段启动证据：FASTX 数据完整性与 QC 流式输入

> Execution note: this batch used offline cargo commands by choice. Offline and network isolation are not current project requirements.
> Historical scope: this batch covered PLAN/P0-01/P0-02 only. Later work completed P0-03/P0-04/P0-05 locally; “pending” below describes this snapshot, not the current workspace.

范围：新版 [项目规划](DEVELOPMENT-PLAN.md) 的 PLAN、P0-01、P0-02。
仅这批任务完成，不代表第一阶段全部完成，更不代表整个工具包已达到有界内存目标。
机器可读记录：[EVIDENCE-P0-START.json](EVIDENCE-P0-START.json)。

## 构建与变更边界

- 基础 commit：`49953ec50ae04b343123a214b77030ff31f39a4b`；验证的是本次**未提交工作树**。
- `rustc 1.99.1 (58cf8f9c3 2026-10-13)`，`cargo 1.99.1 (5f94df478 2026-08-27)`。
- 实现仅修改 `rust-fx/src/io.rs` 和 `rust-fastqc/src/main.rs`。
- 新增两个 crate 的 `tests/input_integrity.rs`，输入全部在临时目录生成。
- 没有新增依赖、修改 Cargo.lock、修改 oracle/expected、发布、打 tag 或提交。
- `rust-umitools/` 与 `rust-cutadapt/tests/fixtures/` 的 scoped git status 为空。
- 保留开始前已有的 `packaging/conda/check_recipe.py`、`packaging/conda/meta.yaml` 修改。

## P0-01：空行不再导致静默截断

原先 `RecordReader::next_fastq()` 看到空行就返回 EOF，导致后续记录消失且退出码为 0。
现在先验证其是否为真正的末尾空行：若后面还有非空输入，返回 input error（exit 3）及空行位置。
已有的文件开头/结尾空行容忍行为保持；FASTQ 中空序列/空质量的完整记录仍可转换。

回归覆盖六个共用 reader 的命令：`fq2fa`、`seq`、`stats`、`fx2tab`、`sample -p`、`split -s`，
以及 CRLF、gzip、多空行、截断、垃圾尾部、空文件与空序列。

修复后真实探针输出：

```text
FX interior blank: exit=3
ERROR: malformed FASTQ at line 5: unexpected blank line before more input
```

流式转换可能已向 stdout 写出正确前缀；本次保证非零退出，不承诺 stdout 的事务性回滚。
调用者必须检查退出码。该变化是对先前“错误地成功退出”的修复，不宣称与上游对所有畸形输入逐字一致。

## P0-02：QC 接入共享流式解压

`accumulate()` 不再使用 `std::fs::read`，改为 `File` → `bioinfo_fastx::Reader::from_stream` →
`next_chunk` → strict `Parser` → 统计。输入格式按内容检测；保留 Path，不将文件路径有损转换后再打开。
每个 chunk 使用全局 offset/line，因此错误定位不因分块重置。

回归覆盖：

- plain、gzip、BGZF、bzip2、xz、zstd 的完整报告逐字节一致；使用同一文件名，不归一化数值或报告正文。
- 压缩格式伪装成 `.fastq`、压缩空文件、多 member gzip。
- 多个 chunk、单条记录大于 chunk、分块后损坏记录的全局错误位置。
- 截断 FASTQ、缺少最终换行、gzip CRC 损坏和 footer 截断。
- 单个输入失败时不输出报告；损坏输入不创建最终报告文件。
- `.fastq.gz` 正常生成 `reads_fastqc/fastqc_data.txt` 并保留报告中的原始文件名。

修复后真实探针输出：

```text
QC gzip (plain suffix): exit=0; report=True
```

输入缓冲现在随 chunk 和最长记录增长，而不是全文件大小。
**QC 总内存仍未有界**：不同序列前缀计数 map、最长序列的逐位统计仍增长，规划 S-04 保持待办。
不据此声称已通过大规模内存或性能验收，也未添加 stdin 支持。

## 先失败、后通过

先添加测试并对未修改实现执行：

```bash
cargo test --release --locked --offline -p rust-fx -p rust-fastqc \
  --test input_integrity --no-fail-fast
```

修复前（最终测试输入；QC 跨块记录为 17 字节，不整除 256 KiB）：

```text
test result: FAILED. 2 passed; 7 failed; 0 ignored; 0 measured; 0 filtered out
test result: FAILED. 2 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out
```

分别为 QC 与 FASTX，命令 exit 101。修复后同一命令：

```text
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## 实际运行的检查

Cargo 操作均使用离线环境及 `--offline`（fmt 不解析依赖）；没有从 shell 下载任何内容。

| 命令 | 真实结果摘要 |
| --- | --- |
| `cargo fmt -p rust-fx -p rust-fastqc` | exit 0 |
| `cargo fmt --all -- --check` | 无输出，exit 0 |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | `Finished dev profile`，无 warning，exit 0 |
| `cargo test --workspace --release --locked --offline` | 全部通过；下面的重复运行逐 suite 汇总为 699 |
| `cargo test --workspace --release --locked --offline --quiet` | `TOTAL: 699 passed; 0 failed; exit=0`（由每个 `test result` 相加，含 doc tests） |
| `cargo build --release --locked --offline -p rust-fastqc -p rust-fx -p rust-cutadapt` | exit 0，差分使用该构建产物 |
| `(cd rust-cutadapt && python3 tests/check.py --bin ../target/release/rust-cutadapt)` | `all visible conformance cases passed` |
| `bash tests/oracle/run_w13_oracle.sh` | `50/50 cases passed` |
| `MULTIQC_NO_VERSION_CHECK=true unshare -Urn bash tests/oracle/run_qc_oracle.sh` | `22/22 fixtures passed`；MultiQC PASS |
| `unshare -Urn bash tests/oracle/run_qc_oracle.sh --self-test` | `SELFTEST PASS` |
| `git diff --check` | 无输出，exit 0 |

QC oracle 在隔离网络 namespace 中执行，并关闭 MultiQC 的联网版本检查。没有修改 oracle 脚本或比较规则。
使用本机已安装的 SeqKit 2.14.0、fq 0.12.0、FastQC 0.13.0、MultiQC 1.35。

部分原始输出：

```text
PASS simple.fastq -> .fastq
PASS simple.fastq.gz -> .fastq
PASS simple.fastq.gz -> .fastq.gz
all visible conformance cases passed

50/50 cases passed

22/22 fixtures passed
PASS  multiqc            parses our fastqc_data.txt without warnings
SELFTEST PASS
```

## 校验和

下表标识上述未提交工作树及本地 release 构建，不是发布包。

| 文件 | SHA-256 |
| --- | --- |
| `target/release/rust-fx` | `a9e33f66de10890522298e3e25704ccf3d2179d7d6ee62c0ea824894305b4783` |
| `target/release/rust-fastqc` | `6661804da2996dc6a48c6e9265854b40f14191eb14f89ae03653a9f758259912` |
| `rust-fx/src/io.rs` | `1d9acb449575522429363e46197c5c9b7445855f19a167528e9d9ab3631604ad` |
| `rust-fastqc/src/main.rs` | `4bc96d5db413a7bb8b102cb10cb120faaf6c8b4b5dfbca5bf22c21ed654606b2` |
| `rust-fx/tests/input_integrity.rs` | `d4976c03cc18d0bbfa720133887c5305adb7f2f5e5ed429ab570d76fc3a5dd5f` |
| `rust-fastqc/tests/input_integrity.rs` | `072d687390573aed30594c962a26a81d1f0ab97558ea5309c908c1325529d32c` |

## 尚未验证、尚未完成

- 本轮没有重跑完整 cutadapt 119-case、UMI exact/semantic/spill、BAM oracle 或打包套件。
  workspace release tests 包含 UMI 的 Rust 测试，但不能代替这些上游差分。
- 没有新的大型 benchmark、内存压力、跨架构、覆盖率引导 fuzz 或干净容器构建证据。
- 既有 QC 22-case oracle 的缺失模块及归一化规则保持不变，不扩张其兼容性声明。
- 下一批：P0-03 monorepo CI/统一入口；P0-04 文档/help 能力同步；P0-05 benchmark schema。
- BAM 外部排序、FASTX split/stats、cutadapt 辅助输出和 QC 序列 map 的内存治理仍在阶段 B。
