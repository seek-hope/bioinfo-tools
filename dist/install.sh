#!/usr/bin/env bash
# Install prebuilt bioinfo-tools binaries from the public distribution repository.
#
# Usage:
#   install.sh [TOOL ...]
#
#   TOOL is one of: rust-cutadapt, rust-umitools, all (default: all).
#
# Environment:
#   BIOINFO_DIST_REPO  GitHub repository      (default: seek-hope/bioinfo-tools-dist)
#   BIOINFO_VERSION    release tag            (default: latest)
#   INSTALL_DIR        destination directory  (default: $HOME/.local/bin)
#
# Examples:
#   curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/seek-hope/bioinfo-tools-dist/main/install.sh | bash -s rust-cutadapt
#   BIOINFO_VERSION=v0.2.0 INSTALL_DIR=/opt/bin bash install.sh rust-cutadapt rust-umitools
set -euo pipefail

repo="${BIOINFO_DIST_REPO:-seek-hope/bioinfo-tools-dist}"
version="${BIOINFO_VERSION:-latest}"
install_dir="${INSTALL_DIR:-$HOME/.local/bin}"

usage() {
  sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
esac

known_tools=(rust-cutadapt rust-umitools)
tools=()
if [ "$#" -eq 0 ]; then
  tools=("${known_tools[@]}")
else
  for arg in "$@"; do
    case "$arg" in
      all) tools+=("${known_tools[@]}") ;;
      rust-cutadapt|rust-umitools) tools+=("$arg") ;;
      *)
        printf 'error: unknown tool %q (expected rust-cutadapt, rust-umitools or all)\n' "$arg" >&2
        exit 2
        ;;
    esac
  done
fi

# De-duplicate while preserving order.
declare -A seen=()
unique=()
for tool in "${tools[@]}"; do
  if [ -z "${seen[$tool]:-}" ]; then
    seen[$tool]=1
    unique+=("$tool")
  fi
done
tools=("${unique[@]}")

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) target="x86_64-unknown-linux-gnu" ;;
  *)
    echo "error: unsupported platform: $(uname -s) $(uname -m)" >&2
    echo "currently published prebuilt target: x86_64-unknown-linux-gnu (Linux)" >&2
    exit 1
    ;;
esac

if [ -z "$version" ] || [ "$version" = "latest" ]; then
  base="https://github.com/${repo}/releases/latest/download"
else
  base="https://github.com/${repo}/releases/download/${version}"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

for tool in "${tools[@]}"; do
  asset="${tool}-${target}.tar.gz"
  printf '==> downloading %s (%s)\n' "$asset" "$version"
  curl -fL --retry 3 --retry-delay 2 -o "$tmp/$asset" "$base/$asset"
  curl -fL --retry 3 --retry-delay 2 -o "$tmp/${asset}.sha256" "$base/${asset}.sha256"
  ( cd "$tmp" && sha256sum -c "${asset}.sha256" )
  tar -xzf "$tmp/$asset" -C "$tmp"
  mkdir -p "$install_dir"
  install -m 755 "$tmp/${tool}-${target}/${tool}" "$install_dir/${tool}"
  printf 'installed %s\n' "$install_dir/${tool}"
done

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) printf 'note: add %s to PATH to run the installed tools\n' "$install_dir" >&2 ;;
esac
