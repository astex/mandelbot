#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 --binary <path> --version <ver> --arch <deb-arch> --output <dir>"
  exit 1
}

BINARY="" VERSION="" ARCH="" OUTPUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)  BINARY="$2";  shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --arch)    ARCH="$2";    shift 2 ;;
    --output)  OUTPUT="$2";  shift 2 ;;
    *) usage ;;
  esac
done

[[ -z "$BINARY" || -z "$VERSION" || -z "$ARCH" || -z "$OUTPUT" ]] && usage

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_NAME="mandelbot_${VERSION}_${ARCH}"
PKG_DIR="$OUTPUT/$PKG_NAME"

mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/share/applications"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/32x32/apps"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/128x128/apps"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/scalable/apps"

cp "$BINARY" "$PKG_DIR/usr/bin/mandelbot"
chmod +x "$PKG_DIR/usr/bin/mandelbot"

cp "$REPO_ROOT/assets/mandelbot.desktop" "$PKG_DIR/usr/share/applications/mandelbot.desktop"
rsvg-convert -w 32 -h 32 "$REPO_ROOT/assets/icons/logo-sm.svg" -o "$PKG_DIR/usr/share/icons/hicolor/32x32/apps/mandelbot.png"
rsvg-convert -w 128 -h 128 "$REPO_ROOT/assets/icons/logo-md.svg" -o "$PKG_DIR/usr/share/icons/hicolor/128x128/apps/mandelbot.png"
cp "$REPO_ROOT/assets/icons/logo.svg" "$PKG_DIR/usr/share/icons/hicolor/scalable/apps/mandelbot.svg"

cat > "$PKG_DIR/DEBIAN/control" <<CONTROL
Package: mandelbot
Version: ${VERSION}
Architecture: ${ARCH}
Maintainer: astex <astex@users.noreply.github.com>
Description: A fractal agent tree for agentic development
 Terminal emulator and IDE-like environment for agentic development.
Homepage: https://github.com/astex/mandelbot
License: GPL-3.0-only
Section: devel
Priority: optional
CONTROL

cat > "$PKG_DIR/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
fi
POSTINST
chmod 755 "$PKG_DIR/DEBIAN/postinst"

dpkg-deb --build "$PKG_DIR" "$OUTPUT/${PKG_NAME}.deb"
echo "Created $OUTPUT/${PKG_NAME}.deb"
