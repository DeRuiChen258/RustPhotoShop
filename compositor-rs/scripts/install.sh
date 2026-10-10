#!/usr/bin/env bash
# Installs RustPhotoShop for the current user: the executable, the icons, a launcher entry and an
# icon on the Desktop that can be double-clicked.
#
# Everything this does is inside the calling user's own directories; nothing needs root.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$here/target/release/rustphotoshop"

if [[ ! -x "$binary" ]]; then
  echo "还没有构建，先运行 scripts/build.sh" >&2
  exit 1
fi

exec "$binary" --install "$@"
