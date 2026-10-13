#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
matches_file="$(mktemp)"
trap 'rm -f "$matches_file"' EXIT

set +e
rg -n -i \
  --glob '!assert-docker-only.sh' \
  '(bubblewrap|bwrap|KOOLADE_BWRAP_BIN|pi_sandbox)' \
  "$root/src" "$root/docs" "$root/README.md" "$root/SECURITY.md" \
  "$root/.github" "$root/containers" "$root/site" "$root/tests" \
  "$root/Cargo.toml" "$root/Cargo.lock" \
  >"$matches_file"
scan_status=$?
set -e

if [[ "$scan_status" -eq 0 ]]; then
  cat "$matches_file" >&2
  echo "Legacy Bubblewrap runtime or configuration references must be removed." >&2
  exit 1
fi
if [[ "$scan_status" -ne 1 ]]; then
  echo "The Docker-only source scan failed with status $scan_status." >&2
  exit "$scan_status"
fi

echo "No legacy Bubblewrap runtime or configuration references found."
