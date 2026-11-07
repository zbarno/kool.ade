#!/usr/bin/env bash
set -euo pipefail

for tool in qemu-img qemu-io sha256sum; do
  command -v "$tool" >/dev/null || {
    echo "missing required overlay test tool: $tool" >&2
    exit 2
  }
done

temporary_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
work_dir="$(mktemp -d "$temporary_root/computer-use-overlay-test.XXXXXX")"
cleanup() {
  rm -rf -- "$work_dir"
}
trap cleanup EXIT

base_raw="$work_dir/base.raw"
base_image="$work_dir/base.qcow2"
first_overlay="$work_dir/first.qcow2"
second_overlay="$work_dir/second.qcow2"

truncate -s 16M "$base_raw"
qemu-img convert -f raw -O qcow2 "$base_raw" "$base_image"
qemu-img create -q -f qcow2 -F qcow2 -b "$base_image" "$first_overlay"
qemu-img create -q -f qcow2 -F qcow2 -b "$base_image" "$second_overlay"
base_digest_before="$(sha256sum "$base_image" | cut -d ' ' -f 1)"

qemu-io -f qcow2 -c 'write -P 0x5a 0 4096' "$first_overlay"
qemu-io -f qcow2 -c 'read -P 0x5a 0 4096' "$first_overlay"
qemu-io -f qcow2 -c 'read -P 0x00 0 4096' "$second_overlay"
qemu-io -f qcow2 -c 'read -P 0x00 0 4096' "$base_image"

base_digest_after="$(sha256sum "$base_image" | cut -d ' ' -f 1)"
[[ "$base_digest_after" == "$base_digest_before" ]] || {
  echo "writing one qcow2 overlay changed the immutable base image" >&2
  exit 1
}
qemu-img check "$base_image"
qemu-img check "$first_overlay"
qemu-img check "$second_overlay"
echo "qcow2 overlay isolation passed: sibling overlay and base retained original bytes"
