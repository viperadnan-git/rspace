#!/usr/bin/env bash
# Build rspace.app (macOS) with the embedded icon and version metadata.
# Ad-hoc signs the finished bundle: the linker signs only the binary, leaving a
# signature that promises a _CodeSignature the bundle lacks, which macOS reports
# as "damaged". Unsigned for distribution (no Developer ID), so recipients still
# clear the quarantine flag (`xattr -dr com.apple.quarantine rspace.app`).
# Usage: scripts/bundle_macos.sh [--release]
set -euo pipefail

cd "$(dirname "$0")/.."
if [[ "${1:-}" == "--release" ]]; then
  profile="release"
  cargo build --bin rspace --release
else
  profile="debug"
  cargo build --bin rspace
fi

version=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')

app="target/rspace.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/rspace" "$app/Contents/MacOS/rspace"
cp crates/app/resources/rspace.icns "$app/Contents/Resources/rspace.icns"

cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>rspace</string>
  <key>CFBundleDisplayName</key><string>rspace</string>
  <key>CFBundleIdentifier</key><string>com.viperadnan.rspace</string>
  <key>CFBundleExecutable</key><string>rspace</string>
  <key>CFBundleIconFile</key><string>rspace</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Sign last: any change to the bundle after this invalidates the signature.
codesign --force --deep --sign - "$app"

echo "built $app ($version, $profile)"
