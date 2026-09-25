#!/bin/sh
# End-to-end check: build a static musl binary and run it in minimal containers that have no
# procps/sysstat. Usage: scripts/verify.sh [target-triple]   (default: matches the docker arch)
set -eu
cd "$(dirname "$0")/.."

case "$(docker info --format '{{.Architecture}}' 2>/dev/null)" in
  x86_64|amd64) default=x86_64-unknown-linux-musl ;;
  *) default=aarch64-unknown-linux-musl ;;
esac
target=${1:-$default}

cargo build --release --target "$target"
bin="target/$target/release/perf60"
file "$bin" | grep -q "statically linked" || { echo "FAIL: $bin is not statically linked"; exit 1; }

fail=0
# run <image> <expect-skip-kernel-log:yes|no> <docker args...> -- <perf60 args...>
run() {
  image=$1; shift
  dargs=""
  while [ "$1" != "--" ]; do dargs="$dargs $1"; shift; done
  shift
  # shellcheck disable=SC2086
  cid=$(docker create $dargs "$image" /perf60 "$@")
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

[ "$fail" -eq 0 ] && echo "verify: PASS" || { echo "verify: FAIL"; exit 1; }
