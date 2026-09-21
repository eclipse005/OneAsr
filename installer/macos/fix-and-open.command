#!/bin/bash
# OneAsr macOS 一键安装 / 修复助手 —— 在访达里双击运行。
#
# 做三件事：
#   1. 把同一个窗口里的 OneAsr.app 复制进「应用程序」文件夹（已存在则覆盖重装）
#   2. 清掉 macOS 给下载文件打的隔离标记（不清会报「已损坏，无法打开」）
#   3. 给 App 及其内部程序补一个本机临时签名，然后启动它
#
# 只在需要写「应用程序」文件夹或改归属时，才会弹一次管理员密码。
# 不想跑脚本的话，手动等价操作是：
#   sudo xattr -cr /Applications/OneAsr.app
#   sudo codesign --force --deep --sign - /Applications/OneAsr.app

set -u

APP_NAME="OneAsr.app"
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/$APP_NAME"
DST="/Applications/$APP_NAME"
TITLE="OneAsr 安装助手"

cd "$HOME" 2>/dev/null || true

note() {
  /usr/bin/osascript -e "display dialog \"$1\" buttons {\"好\"} default button 1 with title \"$TITLE\" with icon note" >/dev/null 2>&1 ||
    printf '%s\n' "$1"
}

halt() {
  /usr/bin/osascript -e "display dialog \"$1\" buttons {\"好\"} default button 1 with title \"$TITLE\" with icon caution" >/dev/null 2>&1 ||
    printf '%s\n' "$1" >&2
  exit 1
}

[ -d "$SRC" ] || [ -d "$DST" ] ||
  halt "没有找到 $APP_NAME。请把本文件和 $APP_NAME 放在同一个磁盘映像窗口里，再双击运行一次。"

# 1. 复制到「应用程序」
if [ -d "$SRC" ]; then
  note "正在把 OneAsr 安装到「应用程序」文件夹，请稍等几秒…"
  if ! { /bin/rm -rf "$DST" 2>/dev/null && /usr/bin/ditto "$SRC" "$DST" 2>/dev/null; }; then
    /usr/bin/osascript -e "do shell script \"/bin/rm -rf '$DST'; /usr/bin/ditto '$SRC' '$DST'\" with administrator privileges" >/dev/null 2>&1 ||
      halt "安装失败：没能写入「应用程序」文件夹。请手动把 OneAsr 拖进「应用程序」，再从那里双击本脚本。"
  fi
fi

[ -d "$DST" ] || halt "安装失败：$DST 不存在。"

# 2 + 3. 清隔离标记 + 补临时签名（先试免密码，失败再弹管理员密码）
fix_cmd() {
  printf '%s' "/usr/bin/xattr -cr '$DST'"
  printf '%s' "; /usr/bin/codesign --force --sign - '$DST/Contents/MacOS/oneasr' 2>/dev/null"
  printf '%s' "; /usr/bin/codesign --force --sign - '$DST/Contents/MacOS/oneasr-cli' 2>/dev/null"
  printf '%s' "; /usr/bin/codesign --force --sign - '$DST/Contents/MacOS/bin/ffmpeg' 2>/dev/null"
  printf '%s' "; /usr/bin/codesign --force --deep --sign - '$DST'"
}

if ! /bin/bash -c "$(fix_cmd)" >/dev/null 2>&1; then
  /usr/bin/osascript -e "do shell script \"$(fix_cmd)\" with administrator privileges" >/dev/null 2>&1 ||
    halt "清除下载拦截失败。请打开「终端」手动执行： sudo xattr -cr /Applications/OneAsr.app"
fi

if /usr/bin/xattr "$DST" 2>/dev/null | /usr/bin/grep -q "com.apple.quarantine"; then
  halt "下载拦截标记还在，没能清掉。请打开「终端」手动执行： sudo xattr -cr /Applications/OneAsr.app"
fi

/usr/bin/open "$DST" >/dev/null 2>&1 ||
  halt "已经修复完成，但自动启动失败。请到「应用程序」文件夹里双击 OneAsr。"

note "搞定！OneAsr 已安装并启动。\n\n首次运行需要下载语音模型（几百 MB 起），界面里会显示进度。\n现在可以推出磁盘映像了。"
