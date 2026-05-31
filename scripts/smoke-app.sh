#!/usr/bin/env bash
# .app launch smoke loop(M14-A)
#
# 用法:scripts/smoke-app.sh [--build] [--duration N]
#   --build      :跑前先 `pnpm tauri build`(默认跳过,用现有 bundle)
#   --duration N :app 运行 N 秒后采集证据并 kill(默认 8 秒)
#
# 验证 4 个东西:
#   1. .app launch 不立刻崩(进程 N 秒后仍存活)
#   2. macOS log 没有 panic / abort / FATAL / segfault
#   3. Rust tracing 出现 "pty spawned terminal_id="(证明 PTY 真起)
#   4. screencapture 截一张图存到 .smoke/(便于人工 / CI artifact 检查)
#
# Exit 0:全 4 项通过
# Exit 非 0:打印失败项

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_PATH="$REPO_ROOT/src-tauri/target/release/bundle/macos/VibeTerm.app"
BINARY="$APP_PATH/Contents/MacOS/vibeterm"
SMOKE_DIR="$REPO_ROOT/.smoke"
mkdir -p "$SMOKE_DIR"

DURATION=8
DO_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) DO_BUILD=1 ;;
    --duration) DURATION="$2"; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

if [[ "$DO_BUILD" == "1" ]]; then
  echo "==> rebuild bundle"
  (cd "$REPO_ROOT" && APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-}" pnpm tauri build)
fi

if [[ ! -x "$BINARY" ]]; then
  echo "FAIL: binary not found at $BINARY (run with --build first)" >&2
  exit 1
fi

# 清理之前可能残留的进程
pkill -f "$BINARY" 2>/dev/null || true
sleep 1

TS=$(date +%Y%m%d-%H%M%S)
STDOUT_LOG="$SMOKE_DIR/$TS-stdout.log"
SCREENSHOT="$SMOKE_DIR/$TS-screen.png"

# 隔离 config:smoke 实例绝不读写用户真实 ~/Library/Application Support/VibeTerm.
# 否则与用户运行中的实例 last-writer-wins 抢 tasks.json,会把用户已删的任务覆盖回来.
export VIBETERM_CONFIG_DIR="${VIBETERM_CONFIG_DIR:-$SMOKE_DIR/config-$TS}"
mkdir -p "$VIBETERM_CONFIG_DIR"
trap 'rm -rf "$VIBETERM_CONFIG_DIR"' EXIT
echo "==> isolated config: $VIBETERM_CONFIG_DIR"

echo "==> launching binary directly to capture Rust tracing"
# 必须设 RUST_LOG, 否则 release 二进制默认不输出 tracing → 下面"无 panic"
# 与"pty spawned"两项日志扫描会变成空判(恒过/恒挂). 尊重外部已设的 RUST_LOG.
RUST_LOG="${RUST_LOG:-info}" "$BINARY" > "$STDOUT_LOG" 2>&1 &
APP_PID=$!

# 等启动稳定
echo "==> waiting $DURATION seconds for app to settle"
sleep "$DURATION"

# 验证 1:进程仍存活
if ! kill -0 "$APP_PID" 2>/dev/null; then
  echo "FAIL: process died within $DURATION seconds"
  echo "--- stdout/stderr ---"
  cat "$STDOUT_LOG"
  exit 1
fi
echo "PASS: process alive after ${DURATION}s (pid=$APP_PID)"

# 验证 4:截图(在 kill 之前,让窗口还在)
if /usr/sbin/screencapture -x -o "$SCREENSHOT" 2>/dev/null; then
  echo "PASS: screenshot saved → $SCREENSHOT"
else
  echo "WARN: screencapture failed(possibly no display permission;not fatal)"
fi

# kill app
kill "$APP_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true

# 验证 2:stdout/stderr 中无 panic / abort / FATAL / segfault
# (macOS `log show` 在某些机器上极慢;Rust panic 一定会写 stderr,
#  改用 stdout log 扫描更快更可靠)
echo "==> scanning captured stderr for panic/abort/segfault"
if grep -iE "panicked at|fatal error|segmentation fault|RUST_BACKTRACE|abort\(\)" "$STDOUT_LOG" > /dev/null; then
  echo "FAIL: panic/abort detected in stderr:"
  grep -iE "panicked at|fatal error|segmentation fault|RUST_BACKTRACE|abort\(\)" "$STDOUT_LOG" | head -5
  exit 1
fi
echo "PASS: no panic/abort in stderr"

# 验证 3:Rust tracing 出现 'pty spawned terminal_id='
# (说明 PTY 真 spawn 成功,IPC handler 真跑了)
if grep -q "pty spawned" "$STDOUT_LOG"; then
  echo "PASS: 'pty spawned terminal_id=' tracing seen"
else
  echo "FAIL: no 'pty spawned' tracing in stdout — IPC handler may not be triggered"
  echo "--- stdout tail ---"
  tail -20 "$STDOUT_LOG"
  exit 1
fi

echo
echo "==> SMOKE PASSED"
echo "    log:        $STDOUT_LOG"
echo "    screenshot: $SCREENSHOT"
