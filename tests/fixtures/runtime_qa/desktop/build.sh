#!/bin/sh
# Build the non-Rust packaged desktop QA fixtures (issue #167 delta).
# Usage: build.sh <output-dir>
# Produces:
#   koolade-qa-nonrust-fixture       - C/Xlib fixture, all dependencies resolvable
#   koolade-qa-nonrust-missing-dep   - same source, NEEDED entry that cannot resolve
# Requires: cc and libX11 development headers (Debian/Ubuntu: libx11-dev).
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: build.sh <output-dir>" >&2
    exit 2
fi
outdir="$1"
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$outdir"

if ! command -v cc >/dev/null 2>&1; then
    echo "build.sh: cc is required to build the non-Rust desktop fixtures" >&2
    exit 1
fi
if [ ! -e /usr/include/X11/Xlib.h ] && command -v apt-get >/dev/null 2>&1; then
    # Idempotent CI convenience: only runs where the headers are missing
    # (standard GitHub runners lack libx11-dev by default).
    helper=""
    if [ "$(id -u)" != "0" ] && command -v sudo >/dev/null 2>&1; then
        helper="sudo"
    fi
    $helper apt-get update -qq
    $helper apt-get install -y -qq libx11-dev
fi
if [ ! -e /usr/include/X11/Xlib.h ]; then
    echo "build.sh: libX11 development headers are required (Debian/Ubuntu: apt-get install libx11-dev)" >&2
    exit 1
fi

common="-O2 -Wall -Wextra -Werror -std=c11 -s -D_GNU_SOURCE"

cc $common -o "$outdir/koolade-qa-nonrust-fixture" \
    "$here/non_rust_fixture.c" "$here/action_probe.c" -lX11

stub_objects=$(mktemp -d)
trap 'rm -rf "$stub_objects"' EXIT INT TERM
cc -O2 -fPIC -shared -Wl,-soname,libkoolade_qa_absent_dep.so.1 \
    -o "$stub_objects/libkoolade_qa_absent_dep.so.1" "$here/absent_dep_stub.c"
cc $common -DKOOLADE_QA_ABSENT_DEP \
    -o "$outdir/koolade-qa-nonrust-missing-dep" "$here/non_rust_fixture.c" \
    "$here/action_probe.c" \
    "$stub_objects/libkoolade_qa_absent_dep.so.1" -lX11

# Build-time guard rails proving the dependency mechanism before CI ships
# the fixtures to the rootless lane:
if command -v ldd >/dev/null 2>&1; then
    ldd "$outdir/koolade-qa-nonrust-fixture" | grep -q 'libX11'
    if ! ldd "$outdir/koolade-qa-nonrust-missing-dep" 2>/dev/null | grep -q 'not found'; then
        echo "build.sh: missing-dependency fixture unexpectedly resolved all shared objects" >&2
        ldd "$outdir/koolade-qa-nonrust-missing-dep" >&2 || true
        exit 1
    fi
fi

chmod 0755 "$outdir/koolade-qa-nonrust-fixture" "$outdir/koolade-qa-nonrust-missing-dep"
echo "built non-Rust desktop QA fixtures in $outdir"
