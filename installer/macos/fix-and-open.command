#!/bin/bash
# OneAsr macOS one-click install / repair helper — double-click in Finder.
#
# Does three things:
#   1. Copies OneAsr.app from this window into /Applications (overwrites an old copy)
#   2. Clears the quarantine mark macOS puts on downloaded files (without it
#      the app reports "damaged and can't be opened")
#   3. Ad-hoc signs the app and its bundled tools, then launches it
#
# Asks for an administrator password only when writing to /Applications or
# changing ownership requires it.
# Manual equivalent, if you'd rather not run a script:
#   sudo xattr -cr /Applications/OneAsr.app
#   sudo codesign --force --deep --sign - /Applications/OneAsr.app
#
# OneAsr macOS 一键安装 / 修复助手 —— 在访达里双击运行。
# 做三件事：复制 App 进「应用程序」、清隔离标记、补临时签名并启动。
# 仅在需要写「应用程序」文件夹或改归属时弹一次管理员密码。
# 手动等价操作：sudo xattr -cr /Applications/OneAsr.app && sudo codesign --force --deep --sign - /Applications/OneAsr.app

set -u

APP_NAME="OneAsr.app"
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/$APP_NAME"
DST="/Applications/$APP_NAME"

cd "$HOME" 2>/dev/null || true

# ── UI language: Chinese on zh systems, English otherwise ──────────────
if /usr/bin/defaults read -g AppleLocale 2>/dev/null | /usr/bin/grep -qi '^zh'; then
  ZH=1
else
  ZH=0
fi

if [ "$ZH" = 1 ]; then
  TITLE="OneAsr 安装助手"
  BTN="好"
  MSG_NO_APP="没有找到 $APP_NAME。请把本文件和 $APP_NAME 放在同一个磁盘映像窗口里，再双击运行一次。"
  MSG_INSTALLING="正在把 OneAsr 安装到「应用程序」文件夹，请稍等几秒…"
  MSG_INSTALL_FAILED="安装失败：没能写入「应用程序」文件夹。请手动把 OneAsr 拖进「应用程序」，再从那里双击本脚本。"
  MSG_NO_DST="安装失败：$DST 不存在。"
  MSG_FIX_FAILED="清除下载拦截失败。请打开「终端」手动执行： sudo xattr -cr /Applications/OneAsr.app"
  MSG_STILL_QUARANTINED="下载拦截标记还在，没能清掉。请打开「终端」手动执行： sudo xattr -cr /Applications/OneAsr.app"
  MSG_LAUNCH_FAILED="已经修复完成，但自动启动失败。请到「应用程序」文件夹里双击 OneAsr。"
  MSG_DONE="搞定！OneAsr 已安装并启动。\n\n首次运行需要下载语音模型（几百 MB 起），界面里会显示进度。\n现在可以推出磁盘映像了。"
else
  TITLE="OneAsr Installer"
  BTN="OK"
  MSG_NO_APP="Could not find $APP_NAME. Keep this script and $APP_NAME in the same disk-image window, then double-click the script again."
  MSG_INSTALLING="Installing OneAsr into your Applications folder, please wait a few seconds…"
  MSG_INSTALL_FAILED="Install failed: could not write to the Applications folder. Drag OneAsr into Applications manually, then double-click this script from there."
  MSG_NO_DST="Install failed: $DST does not exist."
  MSG_FIX_FAILED="Could not clear the download block. Open Terminal and run: sudo xattr -cr /Applications/OneAsr.app"
  MSG_STILL_QUARANTINED="The quarantine mark is still there and could not be cleared. Open Terminal and run: sudo xattr -cr /Applications/OneAsr.app"
  MSG_LAUNCH_FAILED="Repair finished, but the app did not launch. Double-click OneAsr in your Applications folder."
  MSG_DONE="Done! OneAsr is installed and running.\n\nThe first launch downloads the speech models (a few hundred MB); progress shows in the app.\nYou can eject this disk image now."
fi

note() {
  /usr/bin/osascript -e "display dialog \"$1\" buttons {\"$BTN\"} default button 1 with title \"$TITLE\" with icon note" >/dev/null 2>&1 ||
    printf '%s\n' "$1"
}

halt() {
  /usr/bin/osascript -e "display dialog \"$1\" buttons {\"$BTN\"} default button 1 with title \"$TITLE\" with icon caution" >/dev/null 2>&1 ||
    printf '%s\n' "$1" >&2
  exit 1
}

[ -d "$SRC" ] || [ -d "$DST" ] ||
  halt "$MSG_NO_APP"

# 1. Copy into /Applications
if [ -d "$SRC" ]; then
  note "$MSG_INSTALLING"
  if ! { /bin/rm -rf "$DST" 2>/dev/null && /usr/bin/ditto "$SRC" "$DST" 2>/dev/null; }; then
    /usr/bin/osascript -e "do shell script \"/bin/rm -rf '$DST'; /usr/bin/ditto '$SRC' '$DST'\" with administrator privileges" >/dev/null 2>&1 ||
      halt "$MSG_INSTALL_FAILED"
  fi
fi

[ -d "$DST" ] || halt "$MSG_NO_DST"

# 2 + 3. Clear quarantine + ad-hoc sign (passwordless first, admin if needed)
fix_cmd() {
  printf '%s' "/usr/bin/xattr -cr '$DST'"
  printf '%s' "; /usr/bin/codesign --force --sign - '$DST/Contents/MacOS/oneasr' 2>/dev/null"
  printf '%s' "; /usr/bin/codesign --force --sign - '$DST/Contents/MacOS/oneasr-cli' 2>/dev/null"
  printf '%s' "; /usr/bin/codesign --force --sign - '$DST/Contents/MacOS/bin/ffmpeg' 2>/dev/null"
  printf '%s' "; /usr/bin/codesign --force --deep --sign - '$DST'"
}

if ! /bin/bash -c "$(fix_cmd)" >/dev/null 2>&1; then
  /usr/bin/osascript -e "do shell script \"$(fix_cmd)\" with administrator privileges" >/dev/null 2>&1 ||
    halt "$MSG_FIX_FAILED"
fi

if /usr/bin/xattr "$DST" 2>/dev/null | /usr/bin/grep -q "com.apple.quarantine"; then
  halt "$MSG_STILL_QUARANTINED"
fi

/usr/bin/open "$DST" >/dev/null 2>&1 ||
  halt "$MSG_LAUNCH_FAILED"

note "$MSG_DONE"
