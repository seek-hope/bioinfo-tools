#!/usr/bin/env bash
# Run a cargo command with the environment described in tests/ci/env.sh.
#
# Usage: bash tests/ci/check.sh fmt|clippy|build|test [extra cargo arguments]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=tests/ci/env.sh
. "$here/env.sh"

cd "$umitools_root"
exec cargo "$@"
