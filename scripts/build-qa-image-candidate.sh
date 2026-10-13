#!/usr/bin/env bash
set -euo pipefail

profile="${1:?profile required}"
platform="${2:?platform required}"
base_image="${3:?locked base image required}"
build_mode="${4:-candidate}"
if [[ "$build_mode" != candidate && "$build_mode" != publish ]]; then
  echo "build mode must be candidate or publish" >&2
  exit 2
fi
architecture="${platform#linux/}"
test "$platform" = linux/amd64
[[ "$base_image" =~ @sha256:[0-9a-f]{64}$ ]]
docker info --format '{{json .SecurityOptions}}' \
  | jq -e 'any(.[]; contains("name=rootless"))' >/dev/null

output_directory="$RUNNER_TEMP/executor-image-candidate-${profile}-${architecture}"
metadata_file="$RUNNER_TEMP/qa-image-build.json"
tag="koolade-runtime-qa:${profile}-candidate"
build_output=--load
if [[ "$build_mode" == publish ]]; then
  case "$profile" in
    browser_qa) tag="ghcr.io/zbarno/koolade-runtime-qa-browser:trusted-$GITHUB_SHA" ;;
    desktop_qa) tag="ghcr.io/zbarno/koolade-runtime-qa-desktop:trusted-$GITHUB_SHA" ;;
  esac
fi
mkdir -p "$output_directory"

case "$profile" in
  browser_qa)
    build_context=containers/runtime-qa/browser
    repository=ghcr.io/zbarno/koolade-runtime-qa-browser
    recipe_files=(
      containers/runtime-qa/browser/Dockerfile
      containers/runtime-qa/browser/driver.mjs
      containers/runtime-qa/browser/relay.mjs
      containers/runtime-qa/browser/package.json
      containers/runtime-qa/browser/package-lock.json
      containers/runtime-qa/browser/vendor/*.tgz
    )
    recipe_sha256="sha256:$(sha256sum "${recipe_files[@]}" | awk '{printf "%s  %s\n", $1, $2}' | sha256sum | awk '{print $1}')"
    docker buildx create --name qa-browser-candidate --driver docker-container --use
    docker buildx inspect --bootstrap
    docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
      --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
      --metadata-file "$metadata_file" "$build_output" --tag "$tag" containers/runtime-qa/browser
    test "$(docker image inspect "$tag" --format '{{.Os}}/{{.Architecture}} {{.Config.User}}')" \
      = "linux/amd64 10001:10001"
    smoke="$RUNNER_TEMP/qa-browser-smoke.mjs"
    cat > "$smoke" <<'SMOKE'
import { chromium } from "playwright";
import { createRequire } from "node:module";
import dns from "node:dns/promises";
import net from "node:net";
const require = createRequire(import.meta.url);
async function denied(host, port) {
  await new Promise((resolve, reject) => {
    const socket = net.createConnection({ host, port });
    const timer = setTimeout(() => socket.destroy(new Error("probe timed out")), 1200);
    socket.once("connect", () => { clearTimeout(timer); socket.destroy(); reject(new Error(`unexpected connection ${host}:${port}`)); });
    socket.once("error", () => { clearTimeout(timer); resolve(); });
  });
}
try { await dns.lookup("example.com"); throw new Error("unexpected DNS resolution"); }
catch (error) { if (error.message === "unexpected DNS resolution") throw error; }
await denied("1.1.1.1", 80);
await denied("172.17.0.1", 2375);
await denied("127.0.0.1", 2375);
const browser = await chromium.launch({ headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-background-networking", "--disable-extensions", "--renderer-process-limit=2"] });
const page = await browser.newPage();
await page.setContent("<title>runtime-qa-smoke</title>");
if (await page.title() !== "runtime-qa-smoke") throw new Error("browser smoke failed");
const versions = { node: process.versions.node, npm: process.env.EXPECTED_NPM,
  python3: process.env.EXPECTED_PYTHON3,
  playwright: require("playwright/package.json").version, chromium: await browser.version(),
  driver: "koolade-runtime-qa-browser-driver-1.0.0", base: process.env.BASE_IMAGE };
await browser.close();
process.stdout.write(JSON.stringify(versions));
SMOKE
    npm_version="$(docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --tmpfs /tmp:rw,nosuid,nodev,size=256m --entrypoint npm "$tag" --version)"
    python_version="$(docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --tmpfs /tmp:rw,nosuid,nodev,size=256m --entrypoint python3 "$tag" --version)"
    [[ "$python_version" =~ ^Python[[:space:]][0-9]+\.[0-9]+\.[0-9]+$ ]]
    tool_versions="$(docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --pids-limit 256 --memory 1g --cpus 2 --tmpfs /tmp:rw,nosuid,nodev,size=256m \
      --env EXPECTED_NPM="$npm_version" --env EXPECTED_PYTHON3="$python_version" --env BASE_IMAGE="$base_image" \
      --mount "type=bind,src=$smoke,dst=/tmp/qa-smoke.mjs,readonly" \
      --entrypoint node "$tag" /tmp/qa-smoke.mjs)"
    tool_versions="$(jq -c . <<< "$tool_versions")"
    scripts/test-browser-qa-fixture.py "$tag"
    ;;
  desktop_qa)
    repository=ghcr.io/zbarno/koolade-runtime-qa-desktop
    export SOURCE_DATE_EPOCH=0 CARGO_INCREMENTAL=0
    cargo +1.98.1 build --locked --release --bin koolade-qa-desktop
    binary="$RUNNER_TEMP/koolade-qa-desktop"
    cp target/release/koolade-qa-desktop "$binary"
    cp "$binary" "$output_directory/koolade-qa-desktop"
    recipe_sha256="$(python3 - "$binary" <<'PY'
import hashlib, pathlib, sys
files = [
    (pathlib.Path("containers/runtime-qa/desktop/Dockerfile"), "containers/runtime-qa/desktop/Dockerfile"),
    (pathlib.Path(sys.argv[1]), "containers/runtime-qa/desktop/koolade-qa-desktop"),
]
payload = "".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {name}\n" for path, name in files).encode()
print("sha256:" + hashlib.sha256(payload).hexdigest())
PY
    )"
    context="$RUNNER_TEMP/qa-desktop-context"
    mkdir -p "$context"
    cp containers/runtime-qa/desktop/Dockerfile "$context/Dockerfile"
    cp "$binary" "$context/koolade-qa-desktop"
    build_context="$context"
    docker buildx create --name qa-desktop-candidate --driver docker-container --use
    docker buildx inspect --bootstrap
    docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
      --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
      --metadata-file "$metadata_file" "$build_output" --tag "$tag" "$context"
    test "$(docker image inspect "$tag" --format '{{.Os}}/{{.Architecture}} {{.Config.User}}')" \
      = "linux/amd64 10001:10001"
    docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --pids-limit 64 --memory 1g --cpus 2 --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
      --tmpfs /run:rw,nosuid,nodev,noexec,size=16m "$tag" --smoke \
      | jq -e '.schema_version == 1 and .status == "ready" and .display_width == 1440 and .display_height == 900 and .screenshot_bytes > 0'
    fixture_evidence="$output_directory/desktop-fixture.json"
    docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --pids-limit 64 --memory 1g --cpus 2 --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
      --tmpfs /run:rw,nosuid,nodev,noexec,size=16m "$tag" --fixture-smoke \
      | tee "$fixture_evidence" \
      | jq -e '.status == "passed" and
          .interaction_case.click_assertion == "passed" and
          .interaction_case.keyboard_assertion == "passed" and
          .interaction_case.screenshot_hashes_distinct == true and
          ([.interaction_case.actions[].action] | index("mouse_click") != null) and
          ([.interaction_case.actions[].action] | index("keyboard_type") != null) and
          .seeded_defect_case.click_assertion == "failed" and
          .seeded_defect_case.defect_detected == true and
          .seeded_defect_case.actions[0].status == "failed"'
    python3 - "$fixture_evidence" "$output_directory" <<'PY'
import base64, hashlib, json, pathlib, sys
evidence = json.loads(pathlib.Path(sys.argv[1]).read_text())
directory = pathlib.Path(sys.argv[2])
for case in ("interaction_case", "seeded_defect_case"):
    for screenshot in evidence[case]["screenshots"]:
        payload = base64.b64decode(screenshot["bytes_base64"], validate=True)
        if hashlib.sha256(payload).hexdigest() != screenshot["sha256"] or not payload.startswith(b"\x89PNG\r\n\x1a\n"):
            raise SystemExit("desktop fixture screenshot evidence failed integrity validation")
        (directory / f"desktop-{case}-{screenshot['name']}.png").write_bytes(payload)
PY
    docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --entrypoint node "$tag" -e '
        const dns = require("node:dns").promises;
        const net = require("node:net");
        async function denied(host, port) {
          await new Promise((resolve, reject) => {
            const socket = net.createConnection({host, port});
            const timer = setTimeout(() => socket.destroy(new Error("probe timed out")), 1200);
            socket.once("connect", () => { clearTimeout(timer); socket.destroy(); reject(new Error(`unexpected connection ${host}:${port}`)); });
            socket.once("error", () => { clearTimeout(timer); resolve(); });
          });
        }
        (async () => {
          try { await dns.lookup("example.com"); throw new Error("unexpected DNS resolution"); }
          catch (error) { if (error.message === "unexpected DNS resolution") throw error; }
          await denied("1.1.1.1", 80);
          await denied("172.17.0.1", 2375);
          await denied("127.0.0.1", 2375);
        })().catch(error => { console.error(error); process.exit(1); });
      '
    xvfb_version="$(docker run --rm --network none --read-only --cap-drop ALL --entrypoint dpkg-query "$tag" -W -f='${Version}' xvfb)"
    tool_versions="$(jq -cn --arg driver linux-x11-driver/v1 --arg xvfb "$xvfb_version" --arg base "$base_image" '{driver:$driver,xvfb:$xvfb,base:$base}')"
    ;;
  *)
    echo "unsupported QA image profile: $profile" >&2
    exit 2
    ;;
esac

# Exercise teardown through the isolated rootless daemon, including auto-remove.
probe_id="$(docker run --rm --detach --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges --pids-limit 8 --memory 64m \
  --entrypoint /bin/sh "$tag" -c 'sleep 60')"
cleanup_probe() {
  if [[ -n "${probe_id:-}" ]]; then docker rm --force "$probe_id" >/dev/null 2>&1 || true; fi
}
trap cleanup_probe EXIT
docker stop --time 1 "$probe_id" >/dev/null
for attempt in {1..20}; do
  if ! docker inspect "$probe_id" >/dev/null 2>&1; then break; fi
  sleep 0.1
done
if docker inspect "$probe_id" >/dev/null 2>&1; then
  echo "rootless Docker did not remove stopped QA probe container" >&2
  exit 1
fi
probe_id=""
trap - EXIT

if [[ "$build_mode" == publish ]]; then
  publish_metadata="$RUNNER_TEMP/qa-image-publish.json"
  docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
    --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
    --metadata-file "$publish_metadata" --push --tag "$tag" "$build_context"
  metadata_file="$publish_metadata"
fi

image_digest="$(jq -er '."containerimage.digest"' "$metadata_file")"
[[ "$image_digest" =~ ^sha256:[0-9a-f]{64}$ ]]
image_reference="$repository@$image_digest"
scan_reference="$tag"
if [[ "$build_mode" == publish ]]; then
  docker pull --platform "$platform" "$image_reference"
  scan_reference="$image_reference"
fi
"$SYFT" "docker:$scan_reference" -o spdx-json="$output_directory/executor-sbom.spdx.json"
sbom_sha256="sha256:$(sha256sum "$output_directory/executor-sbom.spdx.json" | awk '{print $1}')"
jq -n \
  --arg profile "$profile" --arg platform "$platform" --arg digest "$image_digest" \
  --arg reference "$image_reference" --arg base_image "$base_image" \
  --arg source_revision "$GITHUB_SHA" --arg recipe "$recipe_sha256" \
  --arg sbom_sha256 "$sbom_sha256" --argjson tool_versions "$tool_versions" \
  '{profile:$profile,platform:$platform,digest:$digest,reference:$reference,base_image:$base_image,source_revision:$source_revision,build_recipe_sha256:$recipe,sbom_sha256:$sbom_sha256,tool_versions:$tool_versions}' \
  > "$output_directory/executor-image.json"
jq -n --slurpfile candidate "$output_directory/executor-image.json" \
  --arg profile "$profile" --arg platform "$platform" --arg digest "$image_digest" \
  --arg reference "$image_reference" --arg source_revision "$GITHUB_SHA" \
  --arg sbom_sha256 "$sbom_sha256" \
  '{subject:{profile:$profile,platform:$platform,reference:$reference,digest:$digest},source_revision:$source_revision,sbom_sha256:$sbom_sha256,base_image:$candidate[0].base_image,build_recipe_sha256:$candidate[0].build_recipe_sha256}' \
  > "$output_directory/executor-provenance.json"
provenance_sha256="sha256:$(sha256sum "$output_directory/executor-provenance.json" | awk '{print $1}')"
jq --arg provenance "$provenance_sha256" '. + {provenance_sha256:$provenance}' \
  "$output_directory/executor-image.json" > "$output_directory/executor-image.tmp.json"
mv "$output_directory/executor-image.tmp.json" "$output_directory/executor-image.json"
