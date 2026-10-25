> Historical delivery snapshot. `rust-umitools` BAM I/O was subsequently migrated to noodles (2026-10-23); see [EVIDENCE-NOODLES.md](EVIDENCE-NOODLES.md). The hashes/status below describe the earlier snapshot.

# 本地候选交付准备

状态：本地候选交付检查已完成；**没有发布、打 tag 或上传任何产物**。
容器与 Conda 的实际构建已在本轮执行，见 §3/§4。已有
`packaging/conda/check_recipe.py` 和 `packaging/conda/meta.yaml` 的用户修改被保留。

## 1. CLI 成熟度

| CLI | 版本 | 当前定位 | 已验证范围 | 明确未覆盖 |
| --- | --- | --- | --- | --- |
| `rust-cutadapt` | 0.1.0 | experimental / cutadapt 4.6 兼容目标 | 本轮 119/119 差分已重跑；可见 fixture smoke | 压缩字节不等；cutadapt 5.x 未覆盖 |
| `rust-umitools` | 0.2.0 | 组件维护者管理的 UMI 工具 | 本轮 exact 77/77 + formats/spill/determinism/random/extract/oracle 全过；后续已迁移到 noodles | `--output-stats` 仍拒绝 |
| `rust-fx` | 0.1.0 | experimental | 本轮 174 lib + 4 streaming 回归；FASTX 50/50 | 未做全 SeqKit/fq 子命令全集 |
| `rust-bam` | 0.1.0 | experimental / 部分 samtools 替代 | 本轮外部排序、`-f/-F/-q`、`--region` BAI/CSI、`index --csi`、view 多线程 BGZF；samtools 1.24 对照 | 无 CRAM；sort 仍单线程（-@>1 明确拒绝） |
| `rust-fastqc` | 0.1.0 | FastQC 兼容数据报告 prototype | 本轮 22/22；输入/压缩/资源上限回归；Adapter Content experimental + opt-in contaminant | 无 HTML/zip；Adapter Content 非 FastQC 数值等价；Kmer/tile 缺失 |

这些状态不是发布声明。`rust-umitools` 运行时代码在该快照中未修改（后续迁移见 EVIDENCE-NOODLES）；`tests/compat/matrix.py` 与生成的 `docs/compatibility.tsv` 在用户授权下同步。

## 2. 源码与产物映射

- 源码 commit：`49953ec50ae04b343123a214b77030ff31f39a4b`，工作树 dirty（本轮改动未提交）。
- 主工具链：`rustc 1.99.1 (58cf8f9c3 2026-10-13)`；workspace MSRV 1.91。
- 当前本地 release 二进制：

| 二进制 | 版本 | SHA-256 |
| --- | --- | --- |
| `rust-cutadapt` | 0.1.0 | `a3a4b7488f8f804bcee8595d927c6524f549f7dd37bc6b2af7c00b1851182755` |
| `rust-umitools` | 0.2.0 | `2a5caa61cebefcb6ab26904f3c530e63aa03788a4f3df2068471fccae6f0d5ef` |
| `rust-fx` | 0.1.0 | `09d401cef22f5826cace210a5fe0d18b8bb8ef22d088d6ddb92942aca298b7c0` |
| `rust-bam` | 0.1.0 | `a0f6481692dcf4b1e21e9611d39df9ebe810f5719224912703d188b7522bcc35` |
| `rust-fastqc` | 0.1.0 | `084d32ca536b11dd45bc64eb87a042fc734c64d28979c9665c397a7e0b8f11a8` |

- 可复现构建、aarch64 和 SBOM 结果是**自检前快照**；Docker 与 conda-build 已在本轮实际执行；发布前仍应重跑全部打包检查。
- 可复现构建（`packaging/repro-check.sh`）：
  - rust-cutadapt：plain/remapped 均 IDENTICAL；remapped `16d35e0a7e9eb6ae655a1315058c91b123e400758b5f9c283d73dfe668f7c997`；0 个绝对源码路径。
  - rust-bam：plain/remapped 均 IDENTICAL；remapped `83b96e22782dd61b8936888ca0f239b2e2e92f3ab319399a3f15f8b1002b5314`；0 个绝对源码路径。
- aarch64：`bash packaging/cross-build-aarch64.sh` PASS；artifact SHA-256
  `32ffc643d12b9b9a15fe0928664d2774ae11128d4a58b45720255b417ebdbd99`；
  qemu trim 输出与宿主一致。
- SBOM：`python3 packaging/sbom.py --check` PASS；`--self-test` PASS。
- Conda 配方结构：`python3 packaging/conda/check_recipe.py` PASS；真实 conda-build 已执行，产出 `rust-cutadapt-0.1.0-hfb04f09_0.conda`（818,637 bytes）并通过包内 smoke test。
- 证据索引：`docs/EVIDENCE-P0-DOCS-CI.md`、`EVIDENCE-P0-START.md`、
  `EVIDENCE-FINAL.md`（本轮 S-01/S-02/C11）、`bench/results/current.json`。

## 3. 安装/产物 smoke

本轮的 smoke 直接运行当前 release 二进制，不使用公共 `install.sh`（它会下载
已发布产物，不能验证当前源码）：

```text
rust-cutadapt  rust-cutadapt 0.1.0
rust-umitools  rust-umitools 0.2.0
rust-fx        rust-fx 0.1.0
rust-bam       rust-bam 0.1.0
rust-fastqc    rust-fastqc 0.1.0
```

每个 CLI 的 `--version` 均 exit 0。实际数据处理由质量门禁、BAM 外部排序测试、
FASTX streaming 测试和工作流 consumer smoke 覆盖；没有把 `--version` 当作唯一 smoke。

容器与 Conda 实际产物：
- Docker：`bioinfo-tools-selftest:local`，镜像 `sha256:3bdafb5849aaec3ead1204b533c85280159c45475e891ed4818db9e3b7b3f774`，
  `--version` exit 0；容器内 trim 与宿主 `target/release/rust-cutadapt` 输出逐字节一致。
- Conda：`rust-cutadapt-0.1.0-hfb04f09_0.conda`（818,637 bytes），真实 conda-build 与测试通过。


## 4. 未验证 / 阻塞

1. **GitHub workflow 未实际执行**：`quality.yml` 已改为 ubuntu-latest，但本环境没有
   远端执行证据；required checks 和 runner 策略未配置。
2. **容器/Conda 发布级签名/上传**：Docker 镜像 `bioinfo-tools-selftest:local`
   （`sha256:3bdafb58…`）和 conda 包已本地构建/运行；未签名、未上传、未纳入发布。
3. **签名/attestation、SLSA、发布**：未执行。任何发布都需要维护者另行批准。
4. **签名/attestation、SLSA、发布**：未执行。任何发布都需要维护者另行批准。
5. **多平台**：只有 linux x86_64 实测；aarch64 是交叉构建+qemu smoke，不是原生性能。

## 5. 候选交付判断

在当前证据范围内，可以称为“本地候选交付准备完成”：版本/hash/证据映射齐全，
SBOM、可复现构建、aarch64 和配方结构检查通过，CLI 能执行真实输入。
**不能**称为“可公开发布”：远端 CI 和签名/上传仍未完成。
