#!/usr/bin/env bash
# Build OneAsr Linux / macOS release artifacts into release/.
#
#   ./scripts/pack-unix.sh --os linux --arch x64
#   ./scripts/pack-unix.sh --os macos --arch arm64
#
# 版本号只从根 Cargo.toml 的 [workspace.package] version 读，本脚本不写它 ——
# 以前这里会把传入的版本号改写进 Cargo.toml，产物名、README 链接、git tag
# 因此可以各自为政。现在版本只有一个出处，CI 负责断言三者一致。
#
# ffmpeg 来自上游 Tyrrrz/FFmpegBin 的版本化 tag（tag 名 = ffmpeg 版本），URL 与哈希
# 都只声明在 scripts/ffmpeg-checksums.txt 里。这里下的是**归档**，放进产物前强制校验
# 归档的 sha256，再只解出需要的那枚 ffmpeg（不匹配直接失败，绝不降级放行）。
#
# Linux:  OneAsr_<ver>_linux_<arch>.tar.gz  +  OneAsr_<ver>_linux_<arch>.deb
# macOS:  OneAsr_<ver>_macos.dmg            (Apple Silicon .app + Applications shortcut
#                                            + 安装说明.txt + 安装 OneAsr.command)
set -euo pipefail

VERSION=""
OS=""
ARCH=""
SKIP_BUILD=0

usage() {
  echo "usage: $0 --os linux|macos --arch x64|arm64 [--skip-build]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --os) OS="${2:-}"; shift 2 ;;
    --arch) ARCH="${2:-}"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown arg: $1" >&2; usage ;;
  esac
done

[[ -n "$OS" && -n "$ARCH" ]] || usage
[[ "$OS" == "linux" || "$OS" == "macos" ]] || { echo "os must be linux|macos" >&2; exit 2; }
[[ "$ARCH" == "x64" || "$ARCH" == "arm64" ]] || { echo "arch must be x64|arm64" >&2; exit 2; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CHECKSUM_FILE="$ROOT/scripts/ffmpeg-checksums.txt"

# ffmpeg 的来源 URL 只声明在清单文件里（`# base-url <URL>` 一行），
# 打包脚本不再各抄一份 —— 换 tag 时只改清单，不会漏掉某一处。
ffmpeg_base() {
  local url
  url="$(awk '$1 == "#" && $2 == "base-url" { print $3; exit }' "$CHECKSUM_FILE")"
  [[ -n "$url" ]] || {
    echo "no '# base-url <URL>' line in $CHECKSUM_FILE" >&2
    exit 1
  }
  printf '%s' "$url"
}

FFMPEG_BASE="$(ffmpeg_base)"
# 归档缓存：同一份归档重复打包不必重下 60~80 MB。缓存命中与否不影响正确性 ——
# 命中也照样校验一次哈希，不对就删掉重下（内容哈希才是可信凭证，不是文件存在）。
FFMPEG_CACHE="$ROOT/dist/ffmpeg-cache"

workspace_version() {
  local v
  v="$(sed -n 's/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
  [[ -n "$v" ]] || {
    echo "[workspace.package] version not found in Cargo.toml" >&2
    exit 1
  }
  printf '%s' "$v"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# 从清单里取出该归档钉住的 sha256。清单缺失、清单里没有这个资产 —— 直接失败。
manifest_sha256() {
  local asset="$1" want
  [[ -f "$CHECKSUM_FILE" ]] || {
    echo "checksum manifest missing: $CHECKSUM_FILE" >&2
    exit 1
  }
  want="$(awk -v a="$asset" '$2 == a && $1 ~ /^[0-9a-fA-F]{64}$/ { print tolower($1); exit }' "$CHECKSUM_FILE")"
  [[ -n "$want" ]] || {
    echo "no sha256 for '$asset' in $CHECKSUM_FILE" >&2
    exit 1
  }
  printf '%s' "$want"
}

# 校验一个文件与清单里钉的哈希是否一致；不一致直接失败，不降级放行。
# 为什么：release 资产默认仍可被重新上传，URL 不保证内容，唯一可信的锚点是哈希。
assert_sha256() {
  local file="$1" asset="$2" want="$3" got
  got="$(sha256_of "$file")"
  if [[ "$got" != "$want" ]]; then
    echo "checksum mismatch for $asset" >&2
    echo "  file:     $file" >&2
    echo "  expected: $want" >&2
    echo "  actual:   $got" >&2
    echo "The ffmpeg archive does not match scripts/ffmpeg-checksums.txt. Either it is" >&2
    echo "not the pinned file, or the release asset behind $FFMPEG_BASE/$asset" >&2
    echo "was re-uploaded. Do not ship it. If upstream legitimately changed, refresh the" >&2
    echo "manifest with: python3 scripts/verify-ffmpeg-manifest.py --print-manifest" >&2
    exit 1
  fi
  echo "    sha256 ok  $asset  $want" >&2
}

# 拿到一份**已校验**的归档路径（优先用缓存）。进度信息走 stderr，stdout 只给路径。
fetch_ffmpeg_archive() {
  local asset="$1" archive="$FFMPEG_CACHE/$1" want
  want="$(manifest_sha256 "$asset")"
  mkdir -p "$FFMPEG_CACHE"
  if [[ -f "$archive" ]] && [[ "$(sha256_of "$archive")" == "$want" ]]; then
    echo "    cached   $asset" >&2
  else
    rm -f "$archive"
    echo "==> downloading $asset" >&2
    curl -L --fail --retry 3 -o "$archive" "$FFMPEG_BASE/$asset"
    # 快速失败：先校验再解压/编译，不让几 GB 的后续工作白做。
    assert_sha256 "$archive" "$asset" "$want"
  fi
  printf '%s' "$archive"
}

# 从归档里解出 ffmpeg 可执行文件到 $2。
# 按 basename 精确匹配而不是假设它在归档根目录：上游改归档布局时不会静默解错文件。
# 同时接受 `ffmpeg` 与 `ffmpeg.exe`（Linux/macOS 归档里是前者），并且要求唯一命中 ——
# 命中两个说明归档布局变了，宁可报错也不要猜。
# 一个归档里有 ffmpeg / ffplay / ffprobe，产物只需要 ffmpeg —— 顺手只取它。
extract_ffmpeg() {
  local archive="$1" dest="$2" entries entry
  command -v unzip >/dev/null 2>&1 || {
    echo "unzip is required to extract ffmpeg from $archive" >&2
    exit 1
  }
  entries="$(unzip -Z1 "$archive" | awk -F/ '$NF == "ffmpeg" || $NF == "ffmpeg.exe"')"
  entry="$(printf '%s\n' "$entries" | sed '/^$/d' | head -n 1)"
  if [[ -z "$entry" ]]; then
    echo "no 'ffmpeg' entry inside $archive" >&2
    exit 1
  fi
  if [[ "$(printf '%s\n' "$entries" | sed '/^$/d' | wc -l)" -ne 1 ]]; then
    echo "more than one ffmpeg entry inside $archive:" >&2
    printf '%s\n' "$entries" >&2
    exit 1
  fi
  unzip -p "$archive" "$entry" > "$dest"
  echo "    extracted $entry -> $dest" >&2
}

ffmpeg_asset() {
  case "$OS-$ARCH" in
    linux-x64) echo "ffmpeg-linux-x64.zip" ;;
    linux-arm64) echo "ffmpeg-linux-arm64.zip" ;;
    macos-x64) echo "ffmpeg-osx-x64.zip" ;;
    macos-arm64) echo "ffmpeg-osx-arm64.zip" ;;
  esac
}

deb_arch() {
  case "$ARCH" in
    x64) echo "amd64" ;;
    arm64) echo "arm64" ;;
  esac
}

VERSION="$(workspace_version)"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || {
  echo "invalid [workspace.package] version in Cargo.toml: '$VERSION'" >&2
  exit 1
}
echo "==> OneAsr pack-unix  version=$VERSION (from Cargo.toml)  os=$OS  arch=$ARCH"

# --locked：Cargo.lock 已入库，发布构建必须完全按锁定的版本编译。
# 不加 --locked 时，若锁文件与 Cargo.toml 不同步，cargo 会静默重新解析依赖，
# 于是「同一个 tag、同一份源码」在两个平台装上两套不同的依赖。
if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "==> cargo build --locked -p oneasr --release"
  cargo build --locked -p oneasr --release
  echo "==> cargo build --locked -p oneasr-core --release --bin oneasr-cli"
  cargo build --locked -p oneasr-core --release --bin oneasr-cli
fi

GUI="$ROOT/target/release/oneasr"
CLI="$ROOT/target/release/oneasr-cli"
[[ -x "$GUI" && -x "$CLI" ]] || { echo "missing release binaries" >&2; exit 1; }

STAGE="$ROOT/dist/OneAsr"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/models" "$STAGE/output" "$STAGE/runs"
cp "$GUI" "$STAGE/oneasr"
cp "$CLI" "$STAGE/oneasr-cli"
chmod +x "$STAGE/oneasr" "$STAGE/oneasr-cli"

ASSET="$(ffmpeg_asset)"
ARCHIVE="$(fetch_ffmpeg_archive "$ASSET")"
extract_ffmpeg "$ARCHIVE" "$STAGE/bin/ffmpeg"
chmod +x "$STAGE/bin/ffmpeg"

LINUX_LAYOUT=""
LINUX_ICON_NOTE=""
if [[ "$OS" == "linux" ]]; then
  # Linux does not embed an icon in the ELF executable. Ship the icon and a
  # small user-level installer so the portable build can register a launcher
  # with the desktop/taskbar without requiring root.
  cp "$ROOT/assets/icons/app-icon.png" "$STAGE/oneasr.png"
  cat > "$STAGE/install-desktop.sh" <<'INSTALL'
#!/usr/bin/env bash
set -euo pipefail

APP_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APPLICATIONS_DIR="$DATA_DIR/applications"
ICON_DIR="$DATA_DIR/icons/hicolor/256x256/apps"

mkdir -p "$APPLICATIONS_DIR" "$ICON_DIR"
install -m 0644 "$APP_DIR/oneasr.png" "$ICON_DIR/oneasr.png"
cat > "$APPLICATIONS_DIR/oneasr.desktop" <<EOF
[Desktop Entry]
Name=OneAsr
Comment=Local offline A/V to SRT
Exec="$APP_DIR/oneasr"
Icon=oneasr
Terminal=false
Type=Application
Categories=AudioVideo;Audio;
StartupWMClass=oneasr
EOF
chmod 0644 "$APPLICATIONS_DIR/oneasr.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPLICATIONS_DIR" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATA_DIR/icons/hicolor" >/dev/null 2>&1 || true
fi

printf 'OneAsr desktop entry and icon installed under %s\n' "$DATA_DIR"
INSTALL
  chmod +x "$STAGE/install-desktop.sh"
  LINUX_LAYOUT=$'  oneasr.png           application icon\n  install-desktop.sh   register the portable app with the Linux desktop'
  LINUX_ICON_NOTE=$'\nFor the portable Linux build, run `bash install-desktop.sh` once to register\nthe application icon and launcher with the desktop/taskbar. Linux does not\nembed an application icon inside the executable itself; the .deb package does\nthis registration automatically.'
fi

cat > "$STAGE/README.txt" <<EOF
OneAsr $VERSION ($OS $ARCH)
===========================

Local A/V → SRT (Qwen3-ASR + ForcedAligner)

Layout
------
  oneasr        main app
  oneasr-cli    headless CLI
  bin/ffmpeg    media convert
  models/       ASR / Aligner weights (download in Settings)
  output/       exported *.srt
  runs/         intermediate work files (safe to delete)
$LINUX_LAYOUT

Keep this folder somewhere you can write (your home directory) and everything
lands next to the app. Installed somewhere read-only (e.g. /opt)? Models, output
and settings then go to your user data directory instead; the exact path is
shown in Settings and in the startup log.
$LINUX_ICON_NOTE

GPU uses the system graphics driver via wgpu (Vulkan / Metal). No CUDA Toolkit.
EOF

mkdir -p "$ROOT/release"

if [[ "$OS" == "linux" ]]; then
  TAR="$ROOT/release/OneAsr_${VERSION}_linux_${ARCH}.tar.gz"
  echo "==> $TAR"
  tar -C "$ROOT/dist" -czf "$TAR" OneAsr

  DEB_ROOT="$ROOT/dist/deb"
  rm -rf "$DEB_ROOT"
  mkdir -p "$DEB_ROOT/DEBIAN" \
    "$DEB_ROOT/opt/OneAsr" \
    "$DEB_ROOT/usr/share/applications" \
    "$DEB_ROOT/usr/share/icons/hicolor/256x256/apps"
  cp -a "$STAGE/." "$DEB_ROOT/opt/OneAsr/"
  ICON="$ROOT/assets/icons/app-icon.png"
  if [[ -f "$ICON" ]]; then
    cp "$ICON" "$DEB_ROOT/usr/share/icons/hicolor/256x256/apps/oneasr.png"
  fi
  cat > "$DEB_ROOT/usr/share/applications/oneasr.desktop" <<'DESK'
[Desktop Entry]
Name=OneAsr
Comment=Local offline A/V to SRT
Exec=/opt/OneAsr/oneasr
Icon=oneasr
Terminal=false
Type=Application
Categories=AudioVideo;Audio;
StartupWMClass=oneasr
DESK
  # Maintainer 必须是 `名字 <邮箱>`；写成 URL 不符合 Debian 控制文件规范
  # （lintian 报错，部分工具解析失败）。邮箱用仓库 owner 的 GitHub noreply 形式。
  # Installed-Size 以 KiB 计，且要按文件实际大小（--apparent-size）算，
  # 缺了它 dpkg 只能显示「未知大小」。
  INSTALLED_SIZE_KB="$(du -ks --apparent-size "$DEB_ROOT/opt/OneAsr" | cut -f1)"
  cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: oneasr
Version: $VERSION
Section: sound
Priority: optional
Architecture: $(deb_arch)
Maintainer: OneAsr <eclipse005@users.noreply.github.com>
Homepage: https://github.com/eclipse005/OneAsr
Installed-Size: $INSTALLED_SIZE_KB
Description: Local offline batch audio/video to SRT
Depends: libc6, libgcc-s1, libegl1, libgl1, libvulkan1,
 libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxcb-xkb1,
 libxkbcommon0, libxkbcommon-x11-0,
 libasound2t64 | libasound2
EOF
  cat > "$DEB_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
chmod +x /opt/OneAsr/oneasr /opt/OneAsr/oneasr-cli /opt/OneAsr/bin/ffmpeg 2>/dev/null || true
command -v update-desktop-database >/dev/null 2>&1 && \
  update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
  gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
exit 0
EOF
  chmod 755 "$DEB_ROOT/DEBIAN/postinst"
  # 只 postinst 不够：卸载后 desktop 数据库与图标缓存里还留着 OneAsr 的条目，
  # 用户菜单里会挂一个点不开的死图标。postrm 里在 remove/purge 时清一遍。
  cat > "$DEB_ROOT/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
  command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
exit 0
EOF
  chmod 755 "$DEB_ROOT/DEBIAN/postrm"
  DEB="$ROOT/release/OneAsr_${VERSION}_linux_${ARCH}.deb"
  echo "==> $DEB"
  dpkg-deb --root-owner-group --build "$DEB_ROOT" "$DEB"
fi

if [[ "$OS" == "macos" ]]; then
  APP="$ROOT/dist/OneAsr.app"
  rm -rf "$APP"
  mkdir -p "$APP/Contents/MacOS/bin" \
    "$APP/Contents/MacOS/models" \
    "$APP/Contents/MacOS/output" \
    "$APP/Contents/MacOS/runs" \
    "$APP/Contents/Resources"
  cp "$STAGE/oneasr" "$APP/Contents/MacOS/oneasr"
  cp "$STAGE/oneasr-cli" "$APP/Contents/MacOS/oneasr-cli"
  cp "$STAGE/bin/ffmpeg" "$APP/Contents/MacOS/bin/ffmpeg"
  cp "$STAGE/README.txt" "$APP/Contents/Resources/README.txt"
  chmod +x "$APP/Contents/MacOS/oneasr" \
    "$APP/Contents/MacOS/oneasr-cli" \
    "$APP/Contents/MacOS/bin/ffmpeg"

  ICON_PNG="$ROOT/assets/icons/app-icon.png"
  if [[ -f "$ICON_PNG" ]] && command -v sips >/dev/null && command -v iconutil >/dev/null; then
    ICONSET="$ROOT/dist/AppIcon.iconset"
    rm -rf "$ICONSET"
    mkdir -p "$ICONSET"
    for dim in 16 32 64 128 256 512; do
      sips -z "$dim" "$dim" "$ICON_PNG" --out "$ICONSET/icon_${dim}x${dim}.png" >/dev/null
      dbl=$((dim * 2))
      sips -z "$dbl" "$dbl" "$ICON_PNG" --out "$ICONSET/icon_${dim}x${dim}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
    rm -rf "$ICONSET"
  fi

  cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>zh-Hans</string>
  <key>CFBundleDisplayName</key><string>OneAsr</string>
  <key>CFBundleExecutable</key><string>oneasr</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleIdentifier</key><string>com.eclipse005.oneasr</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>OneAsr</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

  DMG_DIR="$ROOT/dist/dmg"
  rm -rf "$DMG_DIR"
  mkdir -p "$DMG_DIR"
  cp -R "$APP" "$DMG_DIR/OneAsr.app"
  ln -s /Applications "$DMG_DIR/Applications"
  # 未签名分发：Gatekeeper 会把下载的 .app 判成「已损坏」，必须在映像里
  # 附上中文说明和一键修复脚本；脚本自己会把 App 装进「应用程序」并解除拦截。
  MACOS_ASSETS="$ROOT/installer/macos"
  [[ -f "$MACOS_ASSETS/install-notes.txt" && -f "$MACOS_ASSETS/fix-and-open.command" ]] || {
    echo "missing installer/macos assets (install-notes.txt / fix-and-open.command)" >&2
    exit 1
  }
  install -m 0644 "$MACOS_ASSETS/install-notes.txt" "$DMG_DIR/安装说明.txt"
  install -m 0755 "$MACOS_ASSETS/fix-and-open.command" "$DMG_DIR/安装 OneAsr.command"
  DMG="$ROOT/release/OneAsr_${VERSION}_macos.dmg"
  echo "==> $DMG"
  rm -f "$DMG"
  hdiutil create -volname "OneAsr $VERSION" -srcfolder "$DMG_DIR" -ov -format UDZO "$DMG"
fi

echo "==> Done."
ls -lh "$ROOT/release"
