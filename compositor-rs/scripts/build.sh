#!/usr/bin/env bash
# Builds the release binary, and tells you what the build can do.
#
# The Dither filter needs a Blocks-capable compiler (clang) plus a Blocks runtime. build.rs looks
# for one, links it statically when it can, and prints what it found; without it everything else
# still builds and that one filter reports itself as unavailable.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"

echo "== 环境 =="
rustc --version
cargo --version
if command -v clang >/dev/null 2>&1; then
  echo "clang: $(clang --version | head -1)"
else
  echo "clang: 未安装（抖动滤镜将不编入，其余功能不受影响）"
fi

echo
echo "== 构建 =="
cargo build --release --locked 2>&1 | tail -20

binary="$here/target/release/rustphotoshop"
echo
echo "== 产物 =="
ls -lh "$binary"
file "$binary" 2>/dev/null || true
if ldd "$binary" 2>/dev/null | grep -qi blocks; then
  echo "提示：该二进制依赖运行时库 libBlocksRuntime，安装后需保证它在动态库搜索路径中。"
else
  echo "依赖检查：没有动态链接 Blocks 运行时（自带或不需要）"
fi

echo
echo "== 自检 =="
"$binary" --self-test
