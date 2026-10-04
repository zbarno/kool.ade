#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Install Kool.ad/e from this source checkout.

Usage: scripts/install.sh [--prefix DIR]

By default, installs into ~/.local. Cargo and the Linux build dependencies
needed by this checkout must already be installed.
EOF
}

prefix="${HOME:?HOME must be set}/.local"
while (($#)); do
    case "$1" in
        --prefix)
            if (($# < 2)) || [[ -z "$2" ]]; then
                echo "error: --prefix requires a directory" >&2
                exit 2
            fi
            prefix="$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$(uname -s)" != Linux ]]; then
    echo "error: Kool.ad/e currently supports Linux x86_64 desktop use" >&2
    exit 1
fi
if [[ "$(uname -m)" != x86_64 ]]; then
    echo "error: Kool.ad/e currently supports Linux x86_64 desktop use" >&2
    exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "error: Cargo was not found; install the Rust toolchain first" >&2
    exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
mkdir -p -- "$prefix"
prefix="$(cd -- "$prefix" && pwd)"

cargo install --locked --path "$repo_root" --root "$prefix" --force

applications_dir="$prefix/share/applications"
icons_dir="$prefix/share/icons/hicolor/scalable/apps"
mkdir -p -- "$applications_dir" "$icons_dir"
install -m 0644 "$repo_root/assets/brand/mark.svg" "$icons_dir/koolade.svg"

# Quote the executable path according to the Desktop Entry Exec field rules.
desktop_exec="$(printf '%s' "$prefix/bin/koolade" | sed 's/\\/\\\\/g; s/"/\\"/g')"
cat > "$applications_dir/koolade.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Kool.ad/e
Comment=Plan software projects through conversation
Exec="$desktop_exec"
Icon=koolade
Terminal=false
Categories=Development;ProjectManagement;
StartupNotify=true
EOF

echo "Kool.ad/e installed to $prefix/bin/koolade"
echo "Desktop entry installed to $applications_dir/koolade.desktop"
if [[ "$prefix" != "$HOME/.local" ]]; then
    echo "Add $prefix/bin to PATH and make $prefix/share/applications discoverable by your desktop environment."
fi
