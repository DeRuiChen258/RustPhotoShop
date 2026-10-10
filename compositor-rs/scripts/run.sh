#!/usr/bin/env bash
# Runs the application from the source tree, opening any files given as arguments.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$here/target/release/rustphotoshop"

if [[ ! -x "$binary" ]]; then
  echo "还没有构建，先运行 scripts/build.sh" >&2
  exit 1
fi

exec "$binary" "$@"
