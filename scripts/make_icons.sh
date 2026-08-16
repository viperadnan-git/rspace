#!/usr/bin/env bash
# Regenerate the OS app-icons from `app-icon.svg` (a pre-rounded squircle):
#   → rspace.icns (macOS), icon.ico (Windows), app-icon.png (Linux + README).
# In-app rendering uses `logo.svg`, the bare mark, which this does not touch.
# Needs: rsvg-convert (librsvg), iconutil (macOS), magick (ImageMagick).
set -euo pipefail
cd "$(dirname "$0")/.."

res="crates/app/resources"
src="$res/app-icon.svg"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# macOS .icns — the standard iconset sizes (@1x and @2x).
mkdir "$tmp/rspace.iconset"
for s in 16 32 128 256 512; do
  rsvg-convert -w "$s" -h "$s" "$src" -o "$tmp/rspace.iconset/icon_${s}x${s}.png"
  d=$((s * 2))
  rsvg-convert -w "$d" -h "$d" "$src" -o "$tmp/rspace.iconset/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$tmp/rspace.iconset" -o "$res/rspace.icns"

# Windows .ico — multi-resolution.
for s in 16 32 48 64 128 256; do rsvg-convert -w "$s" -h "$s" "$src" -o "$tmp/$s.png"; done
magick "$tmp/16.png" "$tmp/32.png" "$tmp/48.png" "$tmp/64.png" "$tmp/128.png" "$tmp/256.png" "$res/icon.ico"

# Linux icon (transparent squircle); also the README preview.
rsvg-convert -w 1024 -h 1024 "$src" -o "$res/app-icon.png"

echo "regenerated from $src: rspace.icns, icon.ico, app-icon.png"
