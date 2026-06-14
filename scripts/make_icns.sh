#!/usr/bin/env bash
# Regenerate crates/app/resources/rspace.icns from the square app-icon.png:
# round to the macOS icon shape, then build the iconset. Run when the art changes.
set -euo pipefail
cd "$(dirname "$0")/.."

src="crates/app/resources/app-icon.png"
out="crates/app/resources/rspace.icns"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

swift scripts/round_icon.swift "$src" "$tmp/rounded.png"

mkdir "$tmp/rspace.iconset"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$tmp/rounded.png" --out "$tmp/rspace.iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$tmp/rounded.png" --out "$tmp/rspace.iconset/icon_${s}x${s}@2x.png" >/dev/null
done

iconutil -c icns "$tmp/rspace.iconset" -o "$out"
echo "wrote $out"
