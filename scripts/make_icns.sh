#!/usr/bin/env bash
# Regenerate the app icon from logo.svg (the source of truth):
#   logo.svg → app-icon.png (dark-tiled square) → squircle → rspace.icns.
# In-app rendering uses logo.svg directly; these PNG/.icns outputs are only for
# the OS icon. Run when the art changes. Needs a Node toolchain (npx sharp-cli).
set -euo pipefail
cd "$(dirname "$0")/.."

svg="crates/app/resources/logo.svg"
src="crates/app/resources/app-icon.png"
out="crates/app/resources/rspace.icns"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Render the transparent mark at 780px, pad to 1024, flatten onto the brand bg.
bg="#151922"
npx -y sharp-cli --input "$svg" --output "$src" \
  resize 780 780 -- extend 122 122 122 122 --background "$bg" -- flatten "$bg" >/dev/null
echo "rendered $src from $svg"

swift scripts/round_icon.swift "$src" "$tmp/rounded.png"

mkdir "$tmp/rspace.iconset"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$tmp/rounded.png" --out "$tmp/rspace.iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$tmp/rounded.png" --out "$tmp/rspace.iconset/icon_${s}x${s}@2x.png" >/dev/null
done

iconutil -c icns "$tmp/rspace.iconset" -o "$out"
echo "wrote $out"
