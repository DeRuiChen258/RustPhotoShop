#!/usr/bin/env bash
# Regenerates assets/icons/hicolor/<size>x<size>/apps/rustphotoshop.png from the artwork.
#
# The icon set is committed, so this only has to run when the artwork changes.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_image="${1:-$here/../图标/30f8cfffc3d09087aa (Edited).png}"
sizes=(16 32 48 64 128 256 512)

if [[ ! -f "$source_image" ]]; then
  echo "找不到图标源文件：$source_image" >&2
  echo "用法：scripts/make-icons.sh [源图片路径]" >&2
  exit 1
fi

converter="$(command -v magick || command -v convert || true)"
if [[ -z "$converter" ]]; then
  echo "需要 ImageMagick（magick 或 convert）" >&2
  exit 1
fi

echo "源图：$source_image"
"$converter" identify "$source_image" | head -1

for size in "${sizes[@]}"; do
  target="$here/assets/icons/hicolor/${size}x${size}/apps/rustphotoshop.png"
  mkdir -p "$(dirname "$target")"
  "$converter" "$source_image" -filter Lanczos -resize "${size}x${size}" -strip \
    -define png:color-type=6 "$target"
  echo "  ${size}x${size} → $target"
done

echo "完成：${#sizes[@]} 种尺寸"
