#!/usr/bin/env bash
# Copies the upstream C pixel kernels into csrc/reused, unchanged.
#
# The Rust build compiles these copies, and tests/kernel_parity.rs proves they still match the
# originals byte for byte. Run this after the macOS project's kernels change.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upstream="${RUSTPHOTOSHOP_UPSTREAM_DIR:-$here/../Compositor/Rendering}"
target="$here/csrc/reused"

if [[ ! -d "$upstream" ]]; then
  echo "找不到上游 C 源码目录：$upstream" >&2
  echo "可用 RUSTPHOTOSHOP_UPSTREAM_DIR 指定其它位置。" >&2
  exit 1
fi

mkdir -p "$target"
changed=0
for file in "$upstream"/*.c "$upstream"/*.h; do
  [[ -e "$file" ]] || continue
  name="$(basename "$file")"
  if [[ -f "$target/$name" ]] && cmp -s "$file" "$target/$name"; then
    continue
  fi
  cp "$file" "$target/$name"
  echo "更新 $name"
  changed=$((changed + 1))
done

# Files that upstream removed should not linger here.
for file in "$target"/*.c "$target"/*.h; do
  [[ -e "$file" ]] || continue
  name="$(basename "$file")"
  if [[ ! -f "$upstream/$name" ]]; then
    echo "上游已移除 $name，本地产物保留（build.rs 未引用它）"
  fi
done

echo "同步完成：更新 $changed 个文件"
echo "校验和："
sha256sum "$target"/*.c | sed 's/^/  /'
