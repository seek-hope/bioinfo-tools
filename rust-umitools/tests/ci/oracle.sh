#!/usr/bin/env bash
# Resolve the pinned UMI-tools/samtools oracle without personal paths.
#
# Source this after ROOT is set. Discovery order:
#   1. $UMI_TOOLS_ORACLE_PATH
#   2. $CONDA_PREFIX/bin (an activated conda environment)
#   3. tests/ci/env/bin (the prefix declared by tests/ci/env.yml)
#   4. the caller's PATH
#
# A candidate is used only when it contains both umi_tools and samtools. If no
# candidate is found, the calling script reports the missing tool and exits 3.

umitools_oracle_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
oracle_candidates=()
if [ -n "${UMI_TOOLS_ORACLE_PATH:-}" ]; then
    oracle_candidates+=("$UMI_TOOLS_ORACLE_PATH")
fi
if [ -n "${CONDA_PREFIX:-}" ]; then
    oracle_candidates+=("$CONDA_PREFIX/bin")
fi
oracle_candidates+=("$umitools_oracle_root/tests/ci/env/bin")

for candidate in "${oracle_candidates[@]}"; do
    if [ -d "$candidate" ] && [ -x "$candidate/umi_tools" ] && [ -x "$candidate/samtools" ]; then
        case ":$PATH:" in
            *":$candidate:"*) ;;
            *) PATH="$candidate:$PATH"; export PATH ;;
        esac
        break
    fi
done
