#!/usr/bin/env bash
set -euo pipefail

if (($# != 2)); then
    echo "Usage: scripts/package-linux-deb.sh <target-triple> <version>" >&2
    exit 2
fi

target="$1"
version="$2"
case "$target" in
    x86_64-unknown-linux-gnu) architecture=amd64 ;;
    aarch64-unknown-linux-gnu) architecture=arm64 ;;
    *) echo "Unsupported Debian target: $target" >&2; exit 2 ;;
esac
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$ ]]; then
    echo "Invalid package version: $version" >&2
    exit 2
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
package_root="$repo_root/dist/deb-root"
artifact_root="$repo_root/dist/native"
rm -rf -- "$package_root"
mkdir -p "$repo_root/dist"
dependency_work="$(mktemp -d "$repo_root/dist/deb-dependencies.XXXXXX")"
trap 'rm -rf -- "$dependency_work"' EXIT
mkdir -p "$dependency_work/debian"
cat > "$dependency_work/debian/control" <<'EOF'
Source: koolade
Section: devel
Priority: optional
Maintainer: Kool.ad/e maintainers <5166344+zbarno@users.noreply.github.com>
Standards-Version: 4.6.2

Package: koolade
Architecture: any
Description: Kool.ad/e desktop application
EOF
dependencies="$(cd -- "$dependency_work" && dpkg-shlibdeps -O -e "$repo_root/target/release/koolade" | sed -n 's/^shlibs:Depends=//p')"
if [[ -z "$dependencies" ]]; then
    echo "error: could not resolve native shared-library dependencies" >&2
    exit 1
fi
install -d "$package_root/DEBIAN" \
    "$package_root/usr/bin" \
    "$package_root/usr/share/applications" \
    "$package_root/usr/share/icons/hicolor/scalable/apps" \
    "$artifact_root"
install -m 0755 "$repo_root/target/release/koolade" "$package_root/usr/bin/koolade"
install -m 0644 "$repo_root/assets/brand/mark.svg" \
    "$package_root/usr/share/icons/hicolor/scalable/apps/koolade.svg"
cat > "$package_root/DEBIAN/control" <<EOF
Package: koolade
Version: $version
Section: devel
Priority: optional
Architecture: $architecture
Depends: $dependencies
Maintainer: Kool.ad/e maintainers <5166344+zbarno@users.noreply.github.com>
Description: Conversational project planning and implementation desktop application
 Kool.ad/e is an experimental native desktop application for planning software
 projects through conversation and coordinating Git-based implementation.
EOF
cat > "$package_root/usr/share/applications/koolade.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Kool.ad/e
Comment=Plan software projects through conversation
Exec=/usr/bin/koolade
Icon=koolade
Terminal=false
Categories=Development;ProjectManagement;
StartupNotify=true
EOF

artifact="$artifact_root/koolade-${version}-${architecture}.deb"
dpkg-deb --build --root-owner-group "$package_root" "$artifact"
dpkg-deb --info "$artifact" > "$artifact_root/package-inspection.txt"
dpkg-deb --contents "$artifact" >> "$artifact_root/package-inspection.txt"
rm -rf -- "$package_root"
