#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <dest-libdir> <binary-or-lib>..." >&2
  exit 2
fi

DEST=$1
shift
mkdir -p "$DEST"

SEARCH_DIRS=(
  "/opt/target/lib"
  "/opt/target/lib64"
  "/usr/lib/aarch64-linux-gnu"
  "/lib/aarch64-linux-gnu"
  "/usr/aarch64-linux-gnu/lib"
)

skip_lib() {
  case "$1" in
    libc.so.*|libm.so.*|libdl.so.*|libpthread.so.*|librt.so.*|ld-linux-aarch64.so.*|libgcc_s.so.*)
      return 0 ;;
    libEGL.so.*|libGL.so.*|libGLX.so.*|libGLES*.so.*|libgbm.so.*|libdrm*.so.*|libvulkan.so.*|libMali.so.*|libmali.so.*)
      return 0 ;;
  esac
  return 1
}

resolve_lib() {
  local name=$1 d
  for d in "${SEARCH_DIRS[@]}"; do
    if [ -e "$d/$name" ]; then
      readlink -f "$d/$name"
      return 0
    fi
  done
  return 1
}

queue=("$@")
declare -A seen=()

while [ "${#queue[@]}" -gt 0 ]; do
  current=${queue[0]}
  queue=("${queue[@]:1}")

  [ -e "$current" ] || continue

  while read -r needed; do
    [ -n "$needed" ] || continue
    if skip_lib "$needed"; then
      continue
    fi
    if [ -n "${seen[$needed]:-}" ]; then
      continue
    fi

    path=$(resolve_lib "$needed" || true)
    if [ -z "$path" ]; then
      echo "warning: could not resolve runtime library: $needed" >&2
      continue
    fi

    seen[$needed]=1
    cp -L "$path" "$DEST/$needed"
    queue+=("$DEST/$needed")
  done < <(
    aarch64-linux-gnu-readelf -d "$current" 2>/dev/null \
      | sed -n 's/.*Shared library: \[\(.*\)\].*/\1/p'
  )
done

echo "Bundled runtime libraries:"
find "$DEST" -maxdepth 1 -type f -printf '  %f\n' | sort
