#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 --binary <path> --version <ver> --output <dir>"
  exit 1
}

BINARY="" VERSION="" OUTPUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)  BINARY="$2";  shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --output)  OUTPUT="$2";  shift 2 ;;
    *) usage ;;
  esac
done

[[ -z "$BINARY" || -z "$VERSION" || -z "$OUTPUT" ]] && usage

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP="$OUTPUT/Mandelbot.app"

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BINARY" "$APP/Contents/MacOS/mandelbot"
chmod +x "$APP/Contents/MacOS/mandelbot"
cp "$REPO_ROOT/assets/icons/logo.icns" "$APP/Contents/Resources/logo.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Mandelbot</string>
  <key>CFBundleIdentifier</key>
  <string>com.astex.mandelbot</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleExecutable</key>
  <string>mandelbot</string>
  <key>CFBundleIconFile</key>
  <string>logo</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

cd "$OUTPUT"
zip -r Mandelbot.app.zip Mandelbot.app
echo "Created $OUTPUT/Mandelbot.app.zip"
