#!/usr/bin/env bash
set -euo pipefail

if (($# != 2)); then
    echo "Usage: scripts/package-macos-dmg.sh <target-triple> <version>" >&2
    exit 2
fi

target="$1"
version="$2"
case "$target" in
    x86_64-apple-darwin) architecture=x86_64 ;;
    aarch64-apple-darwin) architecture=arm64 ;;
    *) echo "Unsupported macOS target: $target" >&2; exit 2 ;;
esac
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$ ]]; then
    echo "Invalid package version: $version" >&2
    exit 2
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
artifact_root="$repo_root/dist/native"
app_root="$repo_root/dist/Koolade.app"
contents="$app_root/Contents"
resources="$contents/Resources"
icon_work="$repo_root/dist/icon-work"
rm -rf -- "$app_root" "$icon_work"
install -d "$contents/MacOS" "$resources" "$icon_work" "$artifact_root"
install -m 0755 "$repo_root/target/release/koolade" "$contents/MacOS/koolade"

for size in 16 32 128 256 512; do
    install -d "$icon_work/icon.iconset"
    sips -s format png -z "$size" "$size" "$repo_root/assets/brand/app-icon.png" \
        --out "$icon_work/icon.iconset/icon_${size}x${size}.png" >/dev/null
    if (( size <= 256 )); then
        retina=$((size * 2))
        sips -s format png -z "$retina" "$retina" "$repo_root/assets/brand/app-icon.png" \
            --out "$icon_work/icon.iconset/icon_${size}x${size}@2x.png" >/dev/null
    fi
done
iconutil --convert icns --output "$resources/Koolade.icns" "$icon_work/icon.iconset"
cat > "$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Kool.ad/e</string>
  <key>CFBundleDisplayName</key><string>Kool.ad/e</string>
  <key>CFBundleIdentifier</key><string>com.koolade.desktop</string>
  <key>CFBundleExecutable</key><string>koolade</string>
  <key>CFBundleIconFile</key><string>Koolade</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
plutil -lint "$contents/Info.plist"
artifact="$artifact_root/koolade-${version}-macos-${architecture}.dmg"
hdiutil create -quiet -volname "Kool.ad/e" -srcfolder "$app_root" -format UDZO "$artifact"
hdiutil verify "$artifact" > "$artifact_root/package-inspection.txt"
rm -rf -- "$app_root" "$icon_work"
