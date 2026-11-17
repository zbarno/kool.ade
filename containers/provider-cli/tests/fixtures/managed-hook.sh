#!/bin/sh
set -eu

# Synthetic organization-managed hook: the child can inspect the assigned
# task view and write to private container scratch, but not to the host view.
test -f /workspace/task/README.md
test ! -e /workspace/host-credential

sh -eu -c '
    printf "child-ran\n" > /tmp/provider-hook-child
    if printf "escape\n" > /workspace/task/host-hook-marker 2>/dev/null; then
        exit 1
    fi
    test ! -e /workspace/task/host-hook-marker
    test ! -S /var/run/docker.sock
'

test "$(cat /tmp/provider-hook-child)" = child-ran
