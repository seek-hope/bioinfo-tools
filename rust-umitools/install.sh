#!/usr/bin/env bash
# Install a prebuilt rust-umitools release from the public distribution repo.
set -euo pipefail

repo="${RUST_UMITOOLS_DIST_REPO:-seek-hope/rust-umitools-dist}"
version="${RUST_UMITOOLS_VERSION:-latest}"
install_dir="${INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) target="x86_64-unknown-linux-gnu" ;;
  *)
    echo "unsupported platform: $(uname -s) $(uname -m)" >&2
    echo "currently published prebuilt target: x86_64-unknown-linux-gnu" >&2
    exit 1
    ;;
esac

asset="rust-umitools-${target}.tar.gz"
if [ "$version" = "latest" ]; then
  base="https://github.com/${repo}/releases/latest/download"
else
  base="https://github.com/${repo}/releases/download/${version}"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fL --retry 3 --retry-delay 2 -o "$tmp/$asset" "$base/$asset"
curl -fL --retry 3 --retry-delay 2 -o "$tmp/${asset}.sha256" "$base/${asset}.sha256"
(cd "$tmp" && sha256sum -c "${asset}.sha256")
tar -xzf "$tmp/$asset" -C "$tmp"

mkdir -p "$install_dir"
install -m 755 "$tmp/rust-umitools-${target}/rust-umitools" "$install_dir/rust-umitools"
echo "installed $install_dir/rust-umitools"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) echo "add $install_dir to PATH to run rust-umitools" >&2 ;;
esac
