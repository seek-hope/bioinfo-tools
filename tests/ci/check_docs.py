#!/usr/bin/env python3
"""Fail when current documentation reintroduces retired constraints.

Historical evidence is allowed to mention offline runs; current-facing docs are not
allowed to present offline-only operation, the old dependency limit, or an offline
runner as a requirement.
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]

CURRENT_DOCS = [
    "AGENTS.md",
    "TASK.md",
    "README.md",
    "README.zh-CN.md",
    "bench/README-PROTOCOL.md",
    "bench/RESULTS-README.md",
    "bench/README.md",
    "bench/OPTIMIZATION-PROTOCOL.md",
    "bench/OPTIMIZATION.md",
    "docs/DEVELOPMENT-PLAN.md",
    "tests/ci/README.md",
    "tests/ci/check.sh",
    ".github/workflows/quality.yml",
    "rust-cutadapt/README.md",
    "tests/ci/PINS.md",
    "tests/ci/PINS-QC.md",
    "rust-umitools/README.md",
    "rust-umitools/README.en.md",
    "tests/security/README.md",
    "packaging/README.md",
    "rust-cutadapt/docs/adr/0001-dependency-policy.md",
]

BANNED = [
    (re.compile(r"No shell network access", re.I), "no-network rule"),
    (re.compile(r"do not download crates", re.I), "no-download rule"),
    (re.compile(r"offline quality", re.I), "offline-only gate name"),
    (re.compile(r"self-hosted,\s*linux,\s*x64,\s*bioinfo-tools-offline", re.I), "offline runner label"),
    (re.compile(r"不下载依赖"), "no-download rule (zh)"),
    (re.compile(r"Cargo 构建/检查均离线"), "all-cargo-offline claim"),
    (re.compile(r"Rust 1\.88 or newer", re.I), "old MSRV"),
    (re.compile(r"需要 Rust 1\.88\+"), "old MSRV (zh)"),
    (re.compile(r"must use --offline", re.I), "forced offline"),
    (re.compile(r"RUSTUP_AUTO_INSTALL:\s*[\"']0[\"']"), "forbidden toolchain install"),
    (re.compile(r"仍未实现"), "stale unimplemented claim"),
    (re.compile(r"C5/C7 未满足"), "stale C5/C7 failure claim"),
    (re.compile(r"EVIDENCE-SCALE\.md"), "missing evidence document reference"),
    (re.compile(r"\| T11 \| 部分完成"), "stale T11 partial status"),
    (re.compile(r"\| T14 \| 部分完成"), "stale T14 partial status"),
]

HISTORICAL = [
    "docs/EVIDENCE-2026-10.md",
    "docs/DELIVERY.md",
    "docs/EVIDENCE-FINAL.md",
    "docs/PROFILING.md",
    "docs/EVIDENCE-PACKAGING.md",
    "docs/EVIDENCE-QC.md",
    "docs/EVIDENCE-W1-1.md",
    "docs/EVIDENCE-W1-2.md",
    "docs/EVIDENCE-W1-3.md",
    "docs/EVIDENCE-W1-4.md",
    "rust-cutadapt/docs/EVIDENCE-2026-10.md",
]

EXECUTION_NOTES = [
    "docs/EVIDENCE-P0-START.md",
    "docs/EVIDENCE-P0-CI.md",
]

def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")

def main() -> int:
    errors: list[str] = []

    for rel in CURRENT_DOCS:
        path = ROOT / rel
        if not path.is_file():
            errors.append(f"{rel}: missing")
            continue
        text = read(rel)
        for pattern, label in BANNED:
            for match in pattern.finditer(text):
                line = text.count("\n", 0, match.start()) + 1
                errors.append(f"{rel}:{line}: retired constraint: {label}")

    # Production crates must not hide dead code behind an allow attribute. The
    # Kani proof harness is separate (`verification/kani`) and documents its
    # intentional allow.
    for path in sorted((ROOT / "rust-*").glob("src/**/*.rs")):
        text = path.read_text(encoding="utf-8")
        if "allow(dead_code)" in text:
            rel = path.relative_to(ROOT).as_posix()
            errors.append(f"{rel}: production dead-code allow is not allowed")

    agents = read("AGENTS.md")
    if not re.search(r"Network access.*allowed", agents, re.I | re.S):
        errors.append("AGENTS.md: missing the current network/install permission")
    if "Historical trial brief" not in read("TASK.md"):
        errors.append("TASK.md: missing the historical-trial banner")
    audit = read("docs/DOC-AUDIT.md")
    for needle in ("现行约束", "历史记录", "安全或发布边界", "受保护", "rust-umitools", "compatibility.tsv"):
        if needle not in audit:
            errors.append(f"docs/DOC-AUDIT.md: missing classification/mention: {needle}")

    for rel in HISTORICAL:
        text = read(rel)
        head = "\n".join(text.splitlines()[:15])
        if not re.search(r"Historical (record|snapshot)|历史", head, re.I):
            errors.append(f"{rel}: missing historical banner in the first 15 lines")
    for rel in EXECUTION_NOTES:
        head = "\n".join(read(rel).splitlines()[:15])
        if "Execution note" not in head:
            errors.append(f"{rel}: missing execution note")

    check = read("tests/ci/check.sh")
    for needle in ("--offline", "--isolate-network", "--with-fastx-qc-oracles"):
        if needle not in check:
            errors.append(f"tests/ci/check.sh: missing {needle}")
    if "CARGO_NET_OFFLINE=true" not in check or 'if [ "$offline" -eq 1 ]' not in check:
        errors.append("tests/ci/check.sh: offline mode is not explicitly gated")

    workflow = read(".github/workflows/quality.yml")
    if "ubuntu-latest" not in workflow:
        errors.append(".github/workflows/quality.yml: expected GitHub-hosted ubuntu-latest")
    if re.search(r"self-hosted|bioinfo-tools-offline", workflow, re.I):
        errors.append(".github/workflows/quality.yml: still references an offline self-hosted runner")
    if "dtolnay/rust-toolchain" not in workflow:
        errors.append(".github/workflows/quality.yml: missing pinned toolchain action")

    ci_readme = read("tests/ci/README.md")
    if "Network access" not in ci_readme and "default allows network" not in ci_readme.lower():
        errors.append("tests/ci/README.md: does not state that network access is allowed by default")

    final_json = ROOT / "docs/EVIDENCE-FINAL.json"
    if not final_json.is_file():
        errors.append("docs/EVIDENCE-FINAL.json: missing machine-readable final evidence")
    else:
        try:
            import json
            json.loads(final_json.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            errors.append(f"docs/EVIDENCE-FINAL.json: invalid JSON: {error}")

    plan = read("docs/DEVELOPMENT-PLAN.md")
    for stale in [
        "辅助输出及逐条统计的内存治理",
        "split/stats 内存治理",
        "外部排序、区域查询、过滤、CSI",
        "adapter/contaminant 检测",
        "P0-04/P0-05 继续推进",
    ]:
        if stale in plan:
            errors.append(f"docs/DEVELOPMENT-PLAN.md: stale completed-capability claim: {stale}")
    final = read("docs/EVIDENCE-FINAL.md")
    if "10 current files" in final:
        errors.append("docs/EVIDENCE-FINAL.md: stale K3 output referencing 10 current files")
    delivery = read("docs/DELIVERY.md")
    for stale in ["本轮未重跑完整 119 差分", "cutadapt 119、UMI exact"]:
        if stale in delivery:
            errors.append(f"docs/DELIVERY.md: contradictory oracle status: {stale}")
    try:
        final_doc = json.loads(read("docs/EVIDENCE-FINAL.json"))
        if final_doc.get("completion_scope") is None:
            errors.append("docs/EVIDENCE-FINAL.json: missing completion scope")
        if final_doc.get("audit_status") not in {"passed", "passed_pre_self_check_snapshot"}:
            errors.append("docs/EVIDENCE-FINAL.json: invalid historical audit status")
        if not final_doc.get("audit_scope"):
            errors.append("docs/EVIDENCE-FINAL.json: missing audit scope")
        if not final_doc.get("current_worktree_audit_status"):
            errors.append("docs/EVIDENCE-FINAL.json: missing current worktree audit status")
        if final_doc.get("current_worktree_audit_status", "").startswith("teamagents") and not final_doc.get("current_worktree_audit_evidence"):
            errors.append("docs/EVIDENCE-FINAL.json: TeamAgents status lacks evidence path")
        if final_doc.get("protected_paths_clean") is False and not final_doc.get("protected_exception"):
            errors.append("docs/EVIDENCE-FINAL.json: protected paths not clean but no exception recorded")
        if final_doc.get("completed") is None:
            errors.append("docs/EVIDENCE-FINAL.json: missing completed criteria/tasks")
    except ValueError as error:
        errors.append(f"docs/EVIDENCE-FINAL.json: invalid JSON: {error}")

    for rel in ["docs/DEVELOPMENT-PLAN.md", "docs/EVIDENCE-FINAL.md", "docs/DELIVERY.md"]:
        if "173 lib" in read(rel):
            errors.append(f"{rel}: stale rust-fx test count (173; current is 174)")

    # Local Markdown links in current-facing docs must resolve. Historical
    # documents keep their prose, but a broken pointer is still a defect.
    for path in sorted((ROOT / "docs").glob("*.md")):
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(r'(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)', text):
            target = match.group(1).strip()
            if target.startswith(("http://", "https://", "mailto:", "#", "<")):
                continue
            target = target.split("#", 1)[0]
            if not target:
                continue
            if not (path.parent / target).resolve().exists():
                line = text.count("\n", 0, match.start()) + 1
                errors.append(f"{rel}:{line}: broken local link: {target}")

    if errors:
        for error in errors:
            print(f"FAIL {error}", file=sys.stderr)
        return 1

    print(f"PASS documentation constraints ({len(CURRENT_DOCS)} current files, {len(HISTORICAL)} historical files)")
    print("PASS rust-umitools protection documented (manifest generator exception recorded)")
    return 0

if __name__ == "__main__":
    sys.exit(main())
