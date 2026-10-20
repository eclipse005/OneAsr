#!/usr/bin/env bash
# Build Electron release artifacts for Linux or macOS.
#
#   ./scripts/pack-electron.sh --os linux --arch x64
#   ./scripts/pack-electron.sh --os macos --arch arm64
#
# Requires: cargo, Node 24, apps/electron/node_modules (npm ci).
# Version comes only from Cargo.toml; apps/electron/package.json is asserted to match.
set -euo pipefail

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
[[ "$OS" == "linux" || "$OS" == "macos" ]] || usage
[[ "$ARCH" == "x64" || "$ARCH" == "arm64" ]] || usage

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/apps/electron"
RUNTIME="$APP_DIR/runtime"
RELEASE="$ROOT/release"
cd "$ROOT"

VERSION="$(sed -n 's/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
[[ -n "$VERSION" ]] || { echo "[workspace.package] version not found" >&2; exit 1; }
PACKAGE_VERSION="$(node -p "require('$APP_DIR/package.json').version")"
if [[ "$PACKAGE_VERSION" != "$VERSION" ]]; then
  echo "apps/electron/package.json is $PACKAGE_VERSION but Cargo.toml is $VERSION" >&2
  echo "Bump both (verify-version.py)." >&2
  exit 1
fi
[[ -d "$APP_DIR/node_modules/electron-builder" ]] || {
  echo "apps/electron dependencies missing. Run: cd apps/electron && npm ci" >&2
  exit 1
}

echo "==> OneAsr Electron pack  version=$VERSION  os=$OS  arch=$ARCH"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "==> cargo build --locked -p oneasr-core --release --bin oneasr-cli"
  cargo build --locked -p oneasr-core --release --bin oneasr-cli
  echo "==> npm run build (electron-vite)"
  (cd "$APP_DIR" && npm run build)
fi

CLI="$ROOT/target/release/oneasr-cli"
[[ -x "$CLI" ]] || { echo "CLI not found: $CLI" >&2; exit 1; }

echo "==> staging runtime (CLI + verified ffmpeg)"
rm -rf "$RUNTIME"
mkdir -p "$RUNTIME/bin"
cp "$CLI" "$RUNTIME/oneasr-cli"
chmod +x "$RUNTIME/oneasr-cli"
node "$APP_DIR/scripts/fetch-ffmpeg.mjs" --os "$OS" --arch "$ARCH" --dest "$RUNTIME/bin/ffmpeg"
chmod +x "$RUNTIME/bin/ffmpeg"

BUILD_OUT="$APP_DIR/dist/build-$(date +%Y%m%d%H%M%S)"
if [[ "$OS" == "linux" ]]; then
  echo "==> electron-builder (linux deb + tar.gz)"
  (cd "$APP_DIR" && npx --no-install electron-builder --linux deb tar.gz "--$ARCH" --config.directories.output="$BUILD_OUT")
  DIST="$BUILD_OUT"
  DEB="$(find "$DIST" -maxdepth 1 -name '*.deb' -type f | head -n 1)"
  TAR="$(find "$DIST" -maxdepth 1 -name '*.tar.gz' -type f | head -n 1)"
  [[ -n "$DEB" && -n "$TAR" ]] || { echo "missing deb/tar.gz in $DIST" >&2; exit 1; }
  mkdir -p "$RELEASE"
  cp "$DEB" "$RELEASE/OneAsr_${VERSION}_linux_${ARCH}.deb"
  cp "$TAR" "$RELEASE/OneAsr_${VERSION}_linux_${ARCH}.tar.gz"
else
  echo "==> electron-builder (mac dir)"
  (cd "$APP_DIR" && CSC_IDENTITY_AUTO_DISCOVERY=false npx --no-install electron-builder --mac dir "--$ARCH" --config.directories.output="$BUILD_OUT")
  # macOS 自带的 find 不支持 -maxdepth/-quit；用 head 截断即可。
  APP="$(find "$BUILD_OUT" -name 'OneAsr.app' -type d | head -n 1)"
  [[ -n "$APP" ]] || { echo "OneAsr.app not found under apps/electron/dist" >&2; exit 1; }
  [[ -x "$APP/Contents/Resources/runtime/oneasr-cli" ]] || {
    echo "runtime/oneasr-cli missing from $APP" >&2; exit 1
  }
  [[ -x "$APP/Contents/Resources/runtime/bin/ffmpeg" ]] || {
    echo "runtime/bin/ffmpeg missing from $APP" >&2; exit 1
  }
  chmod +x "$APP/Contents/Resources/runtime/oneasr-cli" \n    "$APP/Contents/Resources/runtime/bin/ffmpeg"
  MACOS_ASSETS="$ROOT/installer/macos"
  [[ -f "$MACOS_ASSETS/install-notes.txt" && -f "$MACOS_ASSETS/fix-and-open.command" ]] || {
    echo "missing installer/macos assets" >&2; exit 1
  }
  DMG_DIR="$ROOT/dist/dmg"
  rm -rf "$DMG_DIR"
  mkdir -p "$DMG_DIR"
  cp -R "$APP" "$DMG_DIR/OneAsr.app"
  ln -s /Applications "$DMG_DIR/Applications"
  install -m 0644 "$MACOS_ASSETS/install-notes.txt" "$DMG_DIR/安装说明.txt"
  install -m 0755 "$MACOS_ASSETS/fix-and-open.command" "$DMG_DIR/安装 OneAsr.command"
  DMG="$RELEASE/OneAsr_${VERSION}_macos.dmg"
  mkdir -p "$RELEASE"
  rm -f "$DMG"
  echo "==> $DMG"
  hdiutil create -volname "OneAsr $VERSION" -srcfolder "$DMG_DIR" -ov -format UDZO "$DMG"
fi

echo "==> Done."
ls -lh "$RELEASE"
