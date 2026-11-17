#!/usr/bin/env bash
set -euo pipefail

image="${1:?usage: run-rootless-containment.sh IMAGE}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
fixture="${repo_root}/containers/provider-cli/tests/fixtures/managed-hook.sh"
scratch="$(mktemp -d)"
task_view="${scratch}/task-view"
container_name="koolade-provider-containment-${$}"

cleanup() {
  docker rm --force "${container_name}" >/dev/null 2>&1 || true
  rm -rf -- "${scratch}"
}
trap cleanup EXIT

mkdir -m 700 "${task_view}" "${scratch}/host-private"
chmod 755 "${task_view}"
printf 'synthetic task content\n' > "${task_view}/README.md"
cp "${fixture}" "${task_view}/managed-hook.sh"
printf 'synthetic-host-credential\n' > "${scratch}/host-private/credential"
chmod 600 "${scratch}/host-private/credential"

docker create \
  --name "${container_name}" \
  --network none \
  --read-only \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 1g \
  --cpus 1 \
  --pids-limit 64 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=32m \
  --tmpfs /home/executor:rw,nosuid,nodev,noexec,size=32m,uid=10001,gid=10001 \
  --mount "type=bind,src=${task_view},dst=/workspace/task,readonly" \
  --entrypoint /bin/sh \
  "${image}" -c 'sleep 300' >/dev/null
docker start "${container_name}" >/dev/null

docker inspect "${container_name}" | jq -e --arg task_view "${task_view}" '
  .[0] as $container |
  $container.Config.User == "10001:10001" and
  $container.HostConfig.NetworkMode == "none" and
  $container.HostConfig.Privileged == false and
  $container.HostConfig.ReadonlyRootfs == true and
  $container.HostConfig.PidMode == "" and
  $container.HostConfig.IpcMode == "private" and
  $container.HostConfig.PidsLimit == 64 and
  $container.HostConfig.Memory == 1073741824 and
  $container.HostConfig.NanoCpus == 1000000000 and
  ($container.HostConfig.CapDrop | index("ALL")) != null and
  ($container.HostConfig.SecurityOpt | any(. == "no-new-privileges:true")) and
  ($container.Mounts | any(.Source == $task_view and .Destination == "/workspace/task" and .RW == false)) and
  ($container.Mounts | all(.Type != "bind" or (.Source == $task_view and .Destination == "/workspace/task" and .RW == false))) and
  ($container.Config.Env | all((startswith("OPENAI_API_KEY=") or startswith("ANTHROPIC_API_KEY=") or startswith("CLAUDE_CODE_OAUTH_TOKEN=")) | not)) and
  ($container.Mounts | all(.Destination != "/var/run/docker.sock"))
' >/dev/null

docker exec "${container_name}" /bin/sh /workspace/task/managed-hook.sh

# Exercise outbound denial from a process with the provider image's Node
# runtime. A connection or timeout is a failure; the rootless `none` network
# must reject the attempt promptly.
docker exec "${container_name}" node -e '
  const net = require("node:net");
  const socket = net.connect({host: "1.1.1.1", port: 443});
  socket.setTimeout(2000, () => { socket.destroy(); process.exit(1); });
  socket.once("connect", () => process.exit(1));
  socket.once("error", () => process.exit(0));
'

test ! -e "${task_view}/host-hook-marker"
test "$(cat "${scratch}/host-private/credential")" = synthetic-host-credential
printf 'rootless provider containment fixture passed\n'
