#!/bin/sh
# Build static release binaries for x86_64 and aarch64 into dist/, with SHA256SUMS.
# Links with Rust's bundled rust-lld (.cargo/config.toml), so it works from macOS or Linux.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rm -rf dist
mkdir -p dist

for arch in x86_64 aarch64; do
  target="$arch-unknown-linux-musl"
  rustup target list --installed | grep -qx "$target" || rustup target add "$target"
  cargo build --locked --release --target "$target"
  out="dist/perf60-$version-$arch-linux-musl"
  cp "target/$target/release/perf60" "$out"
  if ! file "$out" | grep -Eq "static(-pie)? linked|statically linked"; then
    echo "FAIL: $out is not statically linked: $(file "$out")" >&2
    exit 1
  fi
done

(cd dist && shasum -a 256 perf60-* > SHA256SUMS && cat SHA256SUMS)
ls -l dist
