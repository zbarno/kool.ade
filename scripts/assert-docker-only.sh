#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
matches_file="$(mktemp)"
trap 'rm -f "$matches_file"' EXIT

set +e
git -C "$root" grep -n -i -E \
  -e '(bubblewrap|bwrap|KOOLADE_BWRAP_BIN|pi_sandbox)' -- \
  src docs README.md SECURITY.md .github containers site tests Cargo.toml Cargo.lock \
  ':!scripts/assert-docker-only.sh' \
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
