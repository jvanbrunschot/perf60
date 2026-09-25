#!/bin/sh
# End-to-end check: build a static musl binary and run it in minimal containers that have no
# procps/sysstat.
# Usage: scripts/verify.sh [--deep] [target-triple]   (default target: matches the docker arch)
#   --deep  build with the eBPF probes (feature `deep`) and add a privileged `--deep` run whose
#           probe sections must not be SKIPPED. On Linux this needs the pinned nightly and
#           bpf-linker (see CI); on macOS the binary is built by scripts/build-deep.sh.
set -eu
cd "$(dirname "$0")/.."

deep=0
if [ "${1:-}" = "--deep" ]; then deep=1; shift; fi

case "$(docker info --format '{{.Architecture}}' 2>/dev/null)" in
  x86_64|amd64) default=x86_64-unknown-linux-musl ;;
  *) default=aarch64-unknown-linux-musl ;;
esac
target=${1:-$default}
case "$target" in
  x86_64-*) platform=linux/amd64 ;;
  aarch64-*) platform=linux/arm64 ;;
  *) echo "unsupported target $target"; exit 3 ;;
esac

if [ "$deep" = 1 ] && [ "$(uname -s)" != Linux ]; then
  scripts/build-deep.sh "$target"
elif [ "$deep" = 1 ]; then
  cargo build --locked --release --features deep --target "$target"
else
  cargo build --locked --release --target "$target"
fi
bin="target/$target/release/perf60"
file "$bin" | grep -Eq "static(-pie)? linked|statically linked" || { echo "FAIL: $bin is not statically linked"; exit 1; }

fail=0
# run <image> <docker args...> -- <perf60 args...>
run() {
  image=$1; shift
  dargs=""
  while [ "$1" != "--" ]; do dargs="$dargs $1"; shift; done
  shift
  # shellcheck disable=SC2086
  docker pull -q --platform "$platform" "$image" >/dev/null
  cid=$(docker create --platform "$platform" $dargs "$image" /perf60 "$@")
  docker cp "$bin" "$cid:/perf60" >/dev/null
  set +e
  out=$(docker start -a "$cid" 2>&1)
  code=$?
  set -e
  docker rm "$cid" >/dev/null
  printf '%s\n' "$out"
  echo "--- exit code $code"
  if [ "$code" -gt 2 ]; then echo "FAIL: $image exited $code"; fail=1; fi
  LAST_OUT=$out
}

for image in alpine debian:stable-slim busybox; do
  echo "=== $image (text)"
  run "$image" -- --interval 0.5 --count 2 --no-color
  echo "$LAST_OUT" | grep -q "^OVERALL:" || { echo "FAIL: no OVERALL line"; fail=1; }

  echo "=== $image (json)"
  run "$image" -- --interval 0.5 --count 2 --json
  echo "$LAST_OUT" | sed '/^--- exit code/d' | python3 -m json.tool >/dev/null \
    || { echo "FAIL: invalid JSON from $image"; fail=1; }
done

echo "=== alpine (privileged, kernel log readable)"
run alpine --privileged -- --interval 0.5 --count 1 --no-color

if [ "$deep" = 1 ]; then
  echo "=== alpine (privileged, --deep)"
  run alpine --privileged -- --deep --interval 0.5 --count 2 --no-color
  echo "$LAST_OUT" | grep -q "^\[.*\] execsnoop" || { echo "FAIL: no execsnoop section"; fail=1; }
  if echo "$LAST_OUT" | grep -E "^\[SKIP\] (deep|execsnoop|runqlat|biolatency|tcpretrans) "; then
    echo "FAIL: an eBPF probe was SKIPPED"; fail=1
  fi
fi

[ "$fail" -eq 0 ] && echo "verify: PASS" || { echo "verify: FAIL"; exit 1; }
