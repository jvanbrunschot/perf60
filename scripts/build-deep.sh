#!/bin/sh
# Build perf60 with `--features deep` (embedded eBPF programs) inside a Linux container, so no
# nightly/bpf-linker/LLVM is needed on the host. Output: target/<triple>/release/perf60 for each
# requested target (default: both musl targets).
# Usage: scripts/build-deep.sh [target-triple ...]
set -eu
cd "$(dirname "$0")/.."
targets=${*:-"x86_64-unknown-linux-musl aarch64-unknown-linux-musl"}
nightly=$(sed -n 's/^const EBPF_TOOLCHAIN: &str = "\(.*\)";/\1/p' build.rs)
image=rust:1
name=perf60-build-deep-$$
# One target volume per checkout: parallel worktrees must not overwrite each other's builds.
key=$(pwd | shasum | cut -c1-10)

cat > /tmp/perf60-build-deep.sh <<INNER
set -eu
apt-get update -qq >/dev/null && apt-get install -y -qq zstd >/dev/null
rustup toolchain install $nightly --profile minimal --component rust-src >/dev/null 2>&1
rustup target add $targets >/dev/null
/src/scripts/install-bpf-linker.sh /usr/local/bin >/dev/null
cd /src
for t in $targets; do
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
    cargo build --locked --release --features deep --target "\$t"
done
INNER

# Cargo registry/target caches persist in named volumes (bind mounts are not required).
docker create --name "$name" -w /src \
  -v perf60-cargo-registry:/usr/local/cargo/registry \
  -v "perf60-deep-target-$key:/src/target" \
  "$image" sh /build.sh >/dev/null
trap 'docker rm -f "$name" >/dev/null 2>&1' EXIT
docker cp /tmp/perf60-build-deep.sh "$name:/build.sh"
git ls-files -co --exclude-standard | tar -s ',^,src/,' -cf - -T - | docker cp - "$name:/"
docker start -a "$name"
for t in $targets; do
  mkdir -p "target/$t/release"
  docker cp "$name:/src/target/$t/release/perf60" "target/$t/release/perf60"
  file "target/$t/release/perf60" | sed 's/, BuildID[^,]*//'
done
