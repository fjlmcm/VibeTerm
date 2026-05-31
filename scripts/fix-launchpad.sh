#!/usr/bin/env bash
# 修复"启动器/Spotlight 找不到 VibeTerm"。
#
# 背景: 反复构建 + 打开新 DMG(未弹出)会让 macOS LaunchServices 把 VibeTerm.app
# 登记在一堆 /Volumes/VibeTerm* DMG 卷上; 直接启动构建目录里的 .app 做测试也会
# 把那份(dev 隐藏路径)登记成 "VibeTerm"。结果 /Applications 那份从没成为规范登记,
# 启动器/Spotlight 就找不到 / 指错。本脚本把 /Applications/VibeTerm.app 复位为唯一规范。
#
# 用法: scripts/fix-launchpad.sh        (装完 DMG 后跑一次即可)
# 幂等: 可重复运行, 非破坏性(只弹只读镜像 + 刷新 LaunchServices 登记 + 重启 Dock)。

set -uo pipefail

LSR="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP="/Applications/VibeTerm.app"
BUILD_APP="$REPO_ROOT/src-tauri/target/release/bundle/macos/VibeTerm.app"

echo "==> 1) 弹出残留 VibeTerm DMG 卷"
shopt -s nullglob
ejected=0
for v in /Volumes/VibeTerm*; do
  if hdiutil detach "$v" -force >/dev/null 2>&1; then
    echo "    弹出 $v"
    ejected=1
  else
    echo "    无法弹出 $v(可能正占用, 可手动在 Finder 弹出)"
  fi
done
shopt -u nullglob
[ "$ejected" = 0 ] && echo "    (无残留卷)"

echo "==> 2) 注销构建目录副本(避免与 /Applications 抢 'VibeTerm' 解析)"
if [ -d "$BUILD_APP" ]; then
  "$LSR" -u "$BUILD_APP" >/dev/null 2>&1 && echo "    已注销 $BUILD_APP" || echo "    (注销跳过)"
else
  echo "    (构建目录无副本)"
fi

echo "==> 3) 重登记 /Applications/VibeTerm.app 为规范"
if [ ! -d "$APP" ]; then
  echo "    ✗ 未找到 $APP" >&2
  echo "      请先把 VibeTerm 从 DMG 拖入 /Applications, 再跑本脚本。" >&2
  exit 1
fi
touch "$APP"                       # 刷新 mtime, 让 LS 优先这份(版本号相同的情况下按新旧裁决)
"$LSR" -f "$APP" >/dev/null 2>&1 && echo "    已重登记 $APP"

echo "==> 4) 刷新启动器(重启 Dock)"
killall Dock >/dev/null 2>&1 && echo "    Dock 已重启(启动器重新索引 /Applications)"

sleep 2
echo ""
resolved="$(osascript -e 'POSIX path of (path to application "VibeTerm")' 2>/dev/null || true)"
echo "==> 验证: 'VibeTerm' 现在解析到: ${resolved:-<暂不可用, 稍候 Spotlight 索引>}"
if [ "$resolved" = "/Applications/VibeTerm.app/" ]; then
  echo "✅ 已复位为规范。打开启动器 / Cmd+Space 搜 VibeTerm 即可见。"
else
  echo "⚠️ 解析未指向 /Applications(可能索引未完成)。可稍候或重登录一次。"
fi
