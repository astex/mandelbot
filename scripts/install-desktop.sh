#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Find assets relative to the script or the repo root
if [[ -f "$SCRIPT_DIR/../assets/mandelbot.desktop" ]]; then
  ASSETS="$SCRIPT_DIR/../assets"
elif [[ -f "$SCRIPT_DIR/assets/mandelbot.desktop" ]]; then
  ASSETS="$SCRIPT_DIR/assets"
else
  echo "Could not find assets directory" >&2
  exit 1
fi

APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"

mkdir -p "$APPS_DIR"
cp "$ASSETS/mandelbot.desktop" "$APPS_DIR/"
echo "Installed mandelbot.desktop to $APPS_DIR"

for png in "$ASSETS"/icons/hicolor/mandelbot-*.png; do
  size=$(basename "$png" | sed 's/mandelbot-\([0-9]*\)\.png/\1/')
  dest="$ICONS_DIR/${size}x${size}/apps"
  mkdir -p "$dest"
  cp "$png" "$dest/mandelbot.png"
done
echo "Installed icons to $ICONS_DIR"

if command -v update-desktop-database &>/dev/null; then
  update-desktop-database "$APPS_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache &>/dev/null; then
  gtk-update-icon-cache -f -t "$ICONS_DIR" 2>/dev/null || true
fi
