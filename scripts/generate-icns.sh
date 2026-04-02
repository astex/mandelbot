#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ICONS_DIR="$REPO_ROOT/assets/icons"
SVG_SM="$ICONS_DIR/logo-sm.svg"
SVG_MD="$ICONS_DIR/logo-md.svg"
OUTPUT_ICNS="$ICONS_DIR/logo.icns"
OUTPUT_CAR="$ICONS_DIR/Assets.car"

TMPDIR=$(mktemp -d)
ICONSET="$TMPDIR/logo.iconset"
mkdir -p "$ICONSET"

# (filename, pixel_size, dpi, source_svg)
ENTRIES=(
  "icon_16x16.png      16  72  $SVG_SM"
  "icon_16x16@2x.png   32  144 $SVG_SM"
  "icon_32x32.png      32  72  $SVG_SM"
  "icon_32x32@2x.png   64  144 $SVG_SM"
  "icon_128x128.png    128 72  $SVG_MD"
  "icon_128x128@2x.png 256 144 $SVG_MD"
  "icon_256x256.png    256 72  $SVG_MD"
  "icon_256x256@2x.png 512 144 $SVG_MD"
  "icon_512x512.png    512 72  $SVG_MD"
  "icon_512x512@2x.png 1024 144 $SVG_MD"
)

for entry in "${ENTRIES[@]}"; do
  read -r name size dpi svg <<< "$entry"
  rsvg-convert -w "$size" -h "$size" "$svg" -o "$ICONSET/$name"
  sips -s dpiWidth "$dpi" -s dpiHeight "$dpi" "$ICONSET/$name" >/dev/null
done

# Always generate .icns (fallback for older macOS)
iconutil --convert icns "$ICONSET" -o "$OUTPUT_ICNS"
echo "Generated $OUTPUT_ICNS"

# If actool is available (Xcode), also generate Assets.car for native Tahoe squircle masking
if xcrun --find actool &>/dev/null; then
  XCASSETS="$TMPDIR/Assets.xcassets"
  APPICONSET="$XCASSETS/AppIcon.appiconset"
  mkdir -p "$APPICONSET"

  # Copy PNGs into the appiconset
  for entry in "${ENTRIES[@]}"; do
    read -r name _ _ _ <<< "$entry"
    cp "$ICONSET/$name" "$APPICONSET/$name"
  done

  # Generate Contents.json
  cat > "$APPICONSET/Contents.json" << 'CJSON'
{
  "images": [
    { "filename": "icon_16x16.png",      "idiom": "mac", "scale": "1x", "size": "16x16" },
    { "filename": "icon_16x16@2x.png",   "idiom": "mac", "scale": "2x", "size": "16x16" },
    { "filename": "icon_32x32.png",      "idiom": "mac", "scale": "1x", "size": "32x32" },
    { "filename": "icon_32x32@2x.png",   "idiom": "mac", "scale": "2x", "size": "32x32" },
    { "filename": "icon_128x128.png",    "idiom": "mac", "scale": "1x", "size": "128x128" },
    { "filename": "icon_128x128@2x.png", "idiom": "mac", "scale": "2x", "size": "128x128" },
    { "filename": "icon_256x256.png",    "idiom": "mac", "scale": "1x", "size": "256x256" },
    { "filename": "icon_256x256@2x.png", "idiom": "mac", "scale": "2x", "size": "256x256" },
    { "filename": "icon_512x512.png",    "idiom": "mac", "scale": "1x", "size": "512x512" },
    { "filename": "icon_512x512@2x.png", "idiom": "mac", "scale": "2x", "size": "512x512" }
  ],
  "info": { "author": "xcode", "version": 1 }
}
CJSON

  mkdir -p "$TMPDIR/car-output"
  xcrun actool "$XCASSETS" \
    --compile "$TMPDIR/car-output" \
    --platform macosx \
    --minimum-deployment-target 11.0 \
    --app-icon AppIcon \
    --output-partial-info-plist "$TMPDIR/partial.plist" \
    2>/dev/null

  cp "$TMPDIR/car-output/Assets.car" "$OUTPUT_CAR"
  echo "Generated $OUTPUT_CAR"
else
  echo "actool not available (needs Xcode) — skipping Assets.car generation"
fi

rm -rf "$TMPDIR"
