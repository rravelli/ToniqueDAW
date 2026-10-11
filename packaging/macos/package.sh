#!/usr/bin/env bash
# Package release builds for both Mac architectures as a universal
# Tonique.app in dist/tonique-<version>-macos-universal.dmg. Runs on macOS.
# Usage: packaging/macos/package.sh <version>
set -euo pipefail
version=$1
app=dist/Tonique.app
iconset=dist/tonique.iconset
staging=dist/dmg

rm -rf "$app" "$iconset" "$staging"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iconset" "$staging"

lipo -create -output "$app/Contents/MacOS/tonique" \
    target/aarch64-apple-darwin/release/tonique_daw \
    target/x86_64-apple-darwin/release/tonique_daw
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# Every size iconutil expects, scaled from the logo.
for size in 16 32 128 256 512; do
    sips -z $size $size images/logo-256.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) images/logo-256.png \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/tonique.icns"

# Ad-hoc signature: Gatekeeper still warns (that needs a Developer ID), but
# Apple Silicon refuses to run a bundle without any signature.
codesign --force --deep --sign - "$app"

cp -R "$app" LICENSE-MIT LICENSE-APACHE "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname Tonique -srcfolder "$staging" -format UDZO -ov \
    "dist/tonique-$version-macos-universal.dmg"
echo "dist/tonique-$version-macos-universal.dmg"
