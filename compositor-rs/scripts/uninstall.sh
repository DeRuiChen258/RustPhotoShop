#!/usr/bin/env bash
# Removes what scripts/install.sh wrote, from the current user's directories only.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$here/target/release/rustphotoshop"
installed_binary="$HOME/.local/share/rustphotoshop/bin/rustphotoshop"

if [[ -x "$installed_binary" ]]; then
  exec "$installed_binary" --uninstall
fi
if [[ -x "$binary" ]]; then
  exec "$binary" --uninstall
fi

echo "找不到已安装的 RustPhotoShop。" >&2
exit 1
