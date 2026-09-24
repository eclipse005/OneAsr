#!/usr/bin/env bash
# Build OneAsr Linux / macOS release artifacts into release/.
#
#   ./scripts/pack-unix.sh --version 1.0.1 --os linux --arch x64
#   ./scripts/pack-unix.sh --version 1.0.1 --os macos --arch arm64
#
# Linux:  OneAsr_<ver>_linux_<arch>.tar.gz  +  OneAsr_<ver>_linux_<arch>.deb
# macOS:  OneAsr_<ver>_macos.dmg            (Apple Silicon .app + Applications shortcut
#                                            + 安装说明.txt + 安装 OneAsr.command)
set -euo pipefail

VERSION=""
OS=""
ARCH=""
SKIP_BUILD=0
FFMPEG_BASE="https://github.com/eclipse005/OneAsr/releases/download/tools"

usage() {
  echo "usage: $0 --version 1.0.1 --os linux|macos --arch x64|arm64 [--skip-build]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="${2:-}"; shift 2 ;;
    --os) OS="${2:-}"; shift 2 ;;
    --arch) ARCH="${2:-}"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown arg: $1" >&2; usage ;;
  esac
done

[[ -n "$VERSION" && -n "$OS" && -n "$ARCH" ]] || usage
[[ "$OS" == "linux" || "$OS" == "macos" ]] || { echo "os must be linux|macos" >&2; exit 2; }
[[ "$ARCH" == "x64" || "$ARCH" == "arm64" ]] || { echo "arch must be x64|arm64" >&2; exit 2; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

set_workspace_version() {
  python3 - "$1" <<'PY'
import pathlib, re, sys
ver = sys.argv[1]
path = pathlib.Path("Cargo.toml")
text = path.read_text(encoding="utf-8")
updated, n = re.subn(
    r'(?m)^(\s*version\s*=\s*")[^"]+(")',
    rf"\g<1>{ver}\2",
    text,
    count=1,
)
if n != 1:
    raise SystemExit("version field not found in Cargo.toml")
if updated != text:
    path.write_text(updated, encoding="utf-8")
    print(f"    Cargo.toml version -> {ver}")
PY
}

ffmpeg_asset() {
  case "$OS-$ARCH" in
    linux-x64) echo "ffmpeg-linux-x64" ;;
    linux-arm64) echo "ffmpeg-linux-arm64" ;;
    macos-x64) echo "ffmpeg-macos-x64" ;;
    macos-arm64) echo "ffmpeg-macos-arm64" ;;
  esac
}

deb_arch() {
  case "$ARCH" in
    x64) echo "amd64" ;;
    arm64) echo "arm64" ;;
  esac
}

echo "==> OneAsr pack-unix  version=$VERSION  os=$OS  arch=$ARCH"
set_workspace_version "$VERSION"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "==> cargo build -p oneasr --release"
  cargo build -p oneasr --release
  echo "==> cargo build -p oneasr-core --release --bin oneasr-cli"
  cargo build -p oneasr-core --release --bin oneasr-cli
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
echo "==> downloading $ASSET"
curl -L --fail --retry 3 -o "$STAGE/bin/ffmpeg" "$FFMPEG_BASE/$ASSET"
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

Put this folder somewhere you can write (home directory). Models download
next to the app; a read-only install location will fail the first download.
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
  cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: oneasr
Version: $VERSION
Section: sound
Priority: optional
Architecture: $(deb_arch)
Maintainer: OneAsr <https://github.com/eclipse005/OneAsr>
Homepage: https://github.com/eclipse005/OneAsr
Description: Local offline batch audio/video to SRT
Depends: libc6, libgcc-s1, libegl1, libgl1, libvulkan1,
 libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxcb-xkb1,
 libxkbcommon0, libxkbcommon-x11-0
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
