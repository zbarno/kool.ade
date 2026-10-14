#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Linux ]]; then
  echo "Rootless Docker systemd setup requires Linux." >&2
  exit 1
fi

uid="$(id -u)"
runtime_dir="/run/user/$uid"

# GitHub-hosted jobs do not always start a PAM-backed systemd user session.
# Rootless Docker needs that user manager to create its delegated container
# scopes when cgroup resource limits are requested.
sudo loginctl enable-linger "$USER"
sudo systemctl start "user@$uid.service"

for _ in $(seq 1 20); do
  [[ -S "$runtime_dir/bus" ]] && break
  sleep 0.25
done
if [[ ! -S "$runtime_dir/bus" ]]; then
  echo "systemd did not create a D-Bus socket for user $uid at $runtime_dir/bus." >&2
  exit 1
fi

export XDG_RUNTIME_DIR="$runtime_dir"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime_dir/bus"
systemctl --user show-environment >/dev/null

# Check that the user manager can create resource-limited cgroups before
# starting Docker. Rootless Docker ignores these limits without cgroup v2 and
# delegated systemd controllers, so a successful Docker info check is not
# sufficient evidence.
# Keep the probe body literal so the child reads its own cgroup path.
# shellcheck disable=SC2016
systemd-run --user --scope \
  --property=MemoryMax=64M --property=TasksMax=32 \
  bash -ceu '
    cgroup="$(cut -d: -f3 /proc/self/cgroup)"
    test "$(cat "/sys/fs/cgroup$cgroup/memory.max")" = 67108864
    test "$(cat "/sys/fs/cgroup$cgroup/pids.max")" = 32
  '

if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'XDG_RUNTIME_DIR=%s\nDBUS_SESSION_BUS_ADDRESS=%s\n' \
    "$XDG_RUNTIME_DIR" "$DBUS_SESSION_BUS_ADDRESS" >>"$GITHUB_ENV"
fi
