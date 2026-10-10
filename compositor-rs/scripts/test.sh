#!/usr/bin/env bash
# The whole test flow, in the order a person would run it, ending with a written report.
#
# Every step is recorded as ok / FAIL / skip, and the exit status is non-zero if anything failed.
# The report lands in TEST-REPORT.md at the crate root so a result can be pasted into a review.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
report="$here/TEST-REPORT.md"
log="$(mktemp)"
failures=0
skips=0

record() { # record <name> <status> <detail>
  printf '| %s | %s | %s |\n' "$1" "$2" "$3" >>"$log"
  if [[ "$2" == "FAIL" ]]; then failures=$((failures + 1)); fi
  if [[ "$2" == "skip" ]]; then skips=$((skips + 1)); fi
  printf '[%-4s] %s\n' "$2" "$1"
}

step() { # step <name> <command...>
  local name="$1"; shift
  local output
  if output="$("$@" 2>&1)"; then
    record "$name" "ok" "$(printf '%s' "$output" | tail -1 | cut -c1-120)"
  else
    record "$name" "FAIL" "$(printf '%s' "$output" | tail -3 | tr '\n' ' ' | cut -c1-200)"
    printf '%s\n' "$output" | tail -20
  fi
}

echo "== RustPhotoShop 测试流程 =="
echo

step "格式检查（cargo fmt）" cargo fmt --check
step "静态检查（cargo clippy）" cargo clippy --all-targets -- -D warnings
if test_output="$(cargo test --quiet 2>&1)"; then
  passed=$(printf '%s\n' "$test_output" | grep -oE '[0-9]+ passed' | awk '{sum += $1} END {print sum+0}')
  failed=$(printf '%s\n' "$test_output" | grep -oE '[0-9]+ failed' | awk '{sum += $1} END {print sum+0}')
  if [[ "$failed" -eq 0 ]]; then
    record "单元测试 + 集成测试（cargo test）" "ok" "共 $passed 个测试通过，0 失败"
  else
    record "单元测试 + 集成测试（cargo test）" "FAIL" "$failed 个测试失败"
    printf '%s\n' "$test_output" | tail -40
  fi
else
  record "单元测试 + 集成测试（cargo test）" "FAIL" "cargo test 返回非零"
  printf '%s\n' "$test_output" | tail -40
fi
step "发布构建（cargo build --release）" cargo build --release --quiet
step "内置自检（--self-test）" "$here/target/release/rustphotoshop" --self-test
# The entry is validated with desktop-file-validate inside the self-test and again in
# tests/cli_and_desktop.rs; this makes the result visible in the report.
if "$here/target/release/rustphotoshop" --self-test --self-test-verbose 2>/dev/null | grep -q '\[ok  \] 桌面启动项'; then
  record "桌面启动项校验（desktop-file-validate）" "ok" "启动项包含 Exec/Icon/Terminal=false 等字段"
else
  record "桌面启动项校验（desktop-file-validate）" "FAIL" "自检中的桌面启动项检查未通过"
  failures=$((failures + 1))
fi

# A window needs a display; without one the startup test cannot run.
display_available=""
if [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]]; then display_available="yes"; fi
if [[ -n "$display_available" ]]; then
  report_file="$(mktemp)"
  if "$here/target/release/rustphotoshop" --smoke-test 30 --smoke-report "$report_file" >/dev/null 2>&1; then
    record "启动冒烟测试（开窗绘制 30 帧）" "ok" "$(cat "$report_file")"
  else
    record "启动冒烟测试（开窗绘制 30 帧）" "FAIL" "进程返回非零"
    failures=$((failures + 1))
  fi
  rm -f "$report_file"
else
  record "启动冒烟测试（开窗绘制 30 帧）" "skip" "当前环境没有显示（DISPLAY/WAYLAND_DISPLAY 为空）"
fi

# The library's own report, so the run leaves evidence behind.
if [[ -x "$here/target/release/rustphotoshop" ]]; then
  {
    echo "# RustPhotoShop 测试报告"
    echo
    echo "生成时间：$(date '+%Y-%m-%d %H:%M:%S %Z')"
    echo "内核：$(uname -srm)"
    echo "工具链：$(rustc --version)"
    echo "显示环境：${WAYLAND_DISPLAY:-none} / ${DISPLAY:-none}"
    echo
    echo "| 项目 | 结果 | 说明 |"
    echo "|------|------|------|"
    cat "$log"
    echo
    echo "失败：$failures，跳过：$skips"
    echo
    echo "## 自检明细"
    echo '```'
    "$here/target/release/rustphotoshop" --self-test --self-test-verbose 2>/dev/null
    echo '```'
  } >"$report"
  echo
  echo "报告已写入 $report"
fi

rm -f "$log"
if [[ "$failures" -gt 0 ]]; then
  echo "有 $failures 项失败。" >&2
  exit 1
fi
echo "全部通过（跳过 $skips 项）。"
