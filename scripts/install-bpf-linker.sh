#!/bin/sh
# Install the pinned bpf-linker release binary (static musl build) after verifying its SHA-256.
# Usage: scripts/install-bpf-linker.sh [bin-dir]   (default: ~/.local/bin)
# Pinned per supply-chain rules: a new version means new digests here, from
#   gh release view v<version> --repo aya-rs/bpf-linker --json assets
set -eu
version=0.11.1
case "$(uname -m)" in
  x86_64)  asset=bpf-linker-x86_64-unknown-linux-musl.tar.zst
           sha256=e058a6aecc9e65fa4c977b298a8e4b738424d7629769fd352eed409fb57e16e8 ;;
  aarch64|arm64)
           asset=bpf-linker-aarch64-unknown-linux-musl.tar.zst
           sha256=341ec1c595496877cae2b073544c2226d78a922739632b5732dbaa48507f1380 ;;
  *) echo "unsupported host $(uname -m) (bpf-linker is only needed on Linux build hosts)" >&2; exit 1 ;;
esac
dest=${1:-$HOME/.local/bin}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/$asset" "https://github.com/aya-rs/bpf-linker/releases/download/v$version/$asset"
echo "$sha256  $tmp/$asset" | sha256sum -c - >/dev/null || { echo "bpf-linker checksum mismatch" >&2; exit 1; }
mkdir -p "$dest"
tar --zstd -xpf "$tmp/$asset" -C "$dest"
"$dest/bpf-linker" --version
