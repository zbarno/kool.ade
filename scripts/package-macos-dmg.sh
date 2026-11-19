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

# Optional fail-closed gate for SHIPPED bundled runtime resources (e.g. the
# per-architecture QEMU executable plus its signed runtime manifest).
# KOOADE_BUNDLED_RUNTIME_DIR must mirror the app Resources layout: the fixed-
# name manifest at its top level beside the qemu/ subtree it describes. Left
# unset, packaging proceeds exactly as before (no bundled runtime shipped);
# set but failing the application-pinned trust gate, packaging aborts.
bundled_runtime_dir="${KOOADE_BUNDLED_RUNTIME_DIR:-}"
bundled_runtime_verify_log=""
if [[ -n "$bundled_runtime_dir" ]]; then
    bundled_runtime_dir="$(cd -- "$bundled_runtime_dir" && pwd)"
    ops_binary="$repo_root/target/release/computer-use-runtime-manifest"
    if [[ ! -x "$ops_binary" ]]; then
        echo "Missing $ops_binary." >&2
        echo "Build it first: cargo build --release --bin computer-use-runtime-manifest" >&2
        exit 2
    fi
    bundled_runtime_verify_log="$repo_root/dist/bundled-runtime-verify.log"
    if ! "$ops_binary" verify-pinned "$bundled_runtime_dir" > "$bundled_runtime_verify_log"; then
        cat "$bundled_runtime_verify_log" >&2
        rm -f -- "$bundled_runtime_verify_log"
        echo "Refusing to package: bundled runtime resources failed the application trust gate." >&2
        exit 3
    fi
    # Admitted: stage the verified subtree (manifest plus qemu/) into Resources.
    cp -R -- "$bundled_runtime_dir/." "$resources/"
fi

artifact="$artifact_root/koolade-${version}-macos-${architecture}.dmg"
hdiutil create -quiet -volname "Kool.ad/e" -srcfolder "$app_root" -format UDZO "$artifact"
hdiutil verify "$artifact" > "$artifact_root/package-inspection.txt"
if [[ -n "$bundled_runtime_verify_log" ]]; then
    { printf '\nbundled runtime manifest application trust verification:\n'
cat "$bundled_runtime_verify_log"; } >> "$artifact_root/package-inspection.txt"
    rm -f -- "$bundled_runtime_verify_log"
fi
rm -rf -- "$app_root" "$icon_work"
