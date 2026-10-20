#!/usr/bin/env bash
# Shared build environment for the project.
#
# The verification sandbox runs checks read-only and without network: the source
# tree cannot be written and `$HOME` is a temporary directory. This script makes
# the build work in that setting by reading the toolchain and the crate cache
# from the account's home directory, writing build artifacts to a temporary
# directory, and keeping cargo offline.
#
# Source this file; it only exports variables.

umitools_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
umitools_account_home="$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f6 || true)"
[ -n "${umitools_account_home:-}" ] || umitools_account_home="${HOME:-/tmp}"

# Prefer the current home, then fall back to the account home.
umitools_pick() { # $1 = directory name, $2 = marker inside it
  if [ -e "${HOME:-/tmp}/$1/$2" ]; then
    printf '%s' "${HOME:-/tmp}/$1"
  elif [ -e "$umitools_account_home/$1/$2" ]; then
    printf '%s' "$umitools_account_home/$1"
  else
    printf '%s' "${HOME:-/tmp}/$1"
  fi
}

export RUSTUP_HOME="${RUSTUP_HOME:-$(umitools_pick .rustup settings.toml)}"
export CARGO_HOME="${CARGO_HOME:-$(umitools_pick .cargo registry)}"

umitools_scratch="${TMPDIR:-/tmp}/rust-umitools-build"
mkdir -p "$umitools_scratch/target" 2>/dev/null || true
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$umitools_scratch/target}"

# Everything the build needs is already cached, and the sandbox has no network.
export CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}"
