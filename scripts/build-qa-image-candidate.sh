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
buildkit_image="moby/buildkit@sha256:b059f8d7226d0b326bc871af489a5dddf65e36af4d20d14251b072974d9ee87c"
docker info --format '{{json .SecurityOptions}}' \
  | jq -e 'any(.[]; contains("name=rootless"))' >/dev/null

output_directory="$RUNNER_TEMP/executor-image-candidate-${profile}-${architecture}"
metadata_file="$RUNNER_TEMP/qa-image-build.json"
tag="koolade-runtime-qa:${profile}-candidate"
build_output=(--output "type=docker,name=$tag,rewrite-timestamp=true")
normalize_build_context() {
  find "$1" -exec touch -h -d '@0' {} +
}
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
      containers/runtime-qa/browser/service.mjs
      containers/runtime-qa/browser/relay.mjs
      containers/runtime-qa/browser/package.json
      containers/runtime-qa/browser/package-lock.json
      containers/runtime-qa/browser/vendor/*.tgz
      scripts/build-qa-image-candidate.sh
    )
    recipe_sha256="sha256:$(sha256sum "${recipe_files[@]}" | awk '{printf "%s  %s\n", $1, $2}' | sha256sum | awk '{print $1}')"
    docker buildx create --name qa-browser-candidate --driver docker-container \
      --driver-opt "image=$buildkit_image" --use
    docker buildx inspect --bootstrap
    normalize_build_context "$build_context"
    docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
      --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
      --metadata-file "$metadata_file" "${build_output[@]}" containers/runtime-qa/browser
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
  driver: "koolade-runtime-qa-browser-driver-1.1.0", base: process.env.BASE_IMAGE };
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
      --mount "type=bind,src=$smoke,dst=/opt/koolade-runtime-qa/qa-smoke.mjs,readonly" \
      --entrypoint node "$tag" /opt/koolade-runtime-qa/qa-smoke.mjs)"
    tool_versions="$(jq -c . <<< "$tool_versions")"
    python3 scripts/test-browser-qa-fixture.py "$tag"
    ;;
  desktop_qa)
    repository=ghcr.io/zbarno/koolade-runtime-qa-desktop
    export SOURCE_DATE_EPOCH=0 CARGO_INCREMENTAL=0
    cargo +1.98.1 build --locked --release --features qa-desktop-driver-image \
      --bin koolade-qa-desktop --bin koolade-qa-desktop-fixture
    binary="$RUNNER_TEMP/koolade-qa-desktop"
    fixture_binary="$RUNNER_TEMP/koolade-qa-desktop-fixture"
    cp target/release/koolade-qa-desktop "$binary"
    cp target/release/koolade-qa-desktop-fixture "$fixture_binary"
    strip --strip-all --remove-section=.note.gnu.build-id "$binary"
    strip --strip-all --remove-section=.note.gnu.build-id "$fixture_binary"
    cp "$binary" "$output_directory/koolade-qa-desktop"
    # Boundary: inspect the linked driver artifact, because unit tests cannot
    # prove the image digest was removed from the shipped executable. External
    # dependency: the pinned Rust toolchain and rootless Docker. Setup cost:
    # one optimized driver build and isolated image smoke below.
    python3 - "$binary" containers/images.lock.json <<'PY'
import json, pathlib, sys
binary = pathlib.Path(sys.argv[1]).read_bytes()
lock = json.loads(pathlib.Path(sys.argv[2]).read_text())
for profile in ("browser_qa", "desktop_qa"):
    for variant in lock["profiles"][profile]:
        if variant["digest"].encode() in binary:
            raise SystemExit("desktop driver binary embeds its own QA image lock digest")
PY
    recipe_sha256="$(python3 - "$binary" <<'PY'
import hashlib, pathlib, sys
files = [
    (pathlib.Path("containers/runtime-qa/desktop/Dockerfile"), "containers/runtime-qa/desktop/Dockerfile"),
    (pathlib.Path(sys.argv[1]), "containers/runtime-qa/desktop/koolade-qa-desktop"),
    (pathlib.Path("scripts/build-qa-image-candidate.sh"), "scripts/build-qa-image-candidate.sh"),
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
    normalize_build_context "$build_context"
    docker buildx create --name qa-desktop-candidate --driver docker-container \
      --driver-opt "image=$buildkit_image" --use
    docker buildx inspect --bootstrap
    docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
      --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
      --metadata-file "$metadata_file" "${build_output[@]}" "$context"
    test "$(docker image inspect "$tag" --format '{{.Os}}/{{.Architecture}} {{.Config.User}}')" \
      = "linux/amd64 10001:10001"
    docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --pids-limit 64 --memory 1g --cpus 2 --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
      --tmpfs /run:rw,nosuid,nodev,noexec,size=16m "$tag" --smoke \
      | jq -e '.schema_version == 1 and .status == "ready" and .display_width == 1440 and .display_height == 900 and .screenshot_bytes > 0'
    fixture_evidence="$output_directory/desktop-fixture.json"
    docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
      --pids-limit 128 --memory 1g --cpus 2 --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
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
          .seeded_defect_case.actions[0].status == "failed" and
          .interaction_case.foreign_window_excluded == true and
          .interaction_case.foreign_window_action_rejected == true and
          (.native_control_visibility.visible | test("^window [0-9]+: region visible$")) and
          (.native_control_visibility.absent | test("^window [0-9]+: region not visible$")) and
          .packaged_non_rust_application.status == "passed" and
          .packaged_non_rust_application.chromium_version == "143.0.7499.4" and
          .packaged_non_rust_application.resize.width == 900 and
          .packaged_non_rust_application.resize.height == 600 and
          .packaged_non_rust_application.screenshot_hashes_distinct == true and
          .egui_application.status == "passed" and
          .egui_application.click_assertion == "passed" and
          .egui_application.keyboard_assertion == "passed" and
          (.egui_application.active_window_post_states | length == 2) and
          (all(.egui_application.active_window_post_states[]; test("^(clicked|focused) window [0-9]+"))) and
          .egui_application.screenshot_hashes_distinct == true and
          ([.egui_application.screenshots[].name] == ["before", "after_click", "after_keyboard"])'
    python3 - "$fixture_evidence" "$output_directory" <<'PY'
import base64, hashlib, json, pathlib, sys
evidence = json.loads(pathlib.Path(sys.argv[1]).read_text())
directory = pathlib.Path(sys.argv[2])
for case in ("interaction_case", "seeded_defect_case", "packaged_non_rust_application", "egui_application"):
    if case == "egui_application" and len(evidence[case]["screenshots"]) != 3:
        raise SystemExit("egui fixture must retain exactly three screenshots")
    for screenshot in evidence[case]["screenshots"]:
        payload = base64.b64decode(screenshot["bytes_base64"], validate=True)
        if hashlib.sha256(payload).hexdigest() != screenshot["sha256"] or not payload.startswith(b"\x89PNG\r\n\x1a\n"):
            raise SystemExit("desktop fixture screenshot evidence failed integrity validation")
        if case == "egui_application" and (int.from_bytes(payload[16:20], "big"), int.from_bytes(payload[20:24], "big")) != (1440, 900):
            raise SystemExit("egui fixture screenshot dimensions differ from the locked virtual display")
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
    dbus_version="$(docker run --rm --network none --read-only --cap-drop ALL --entrypoint dpkg-query "$tag" -W -f='${Version}' dbus-daemon)"
    dbus_run_session="$(docker run --rm --network none --read-only --cap-drop ALL --entrypoint /bin/sh "$tag" -c 'command -v dbus-run-session')"
    [[ "$dbus_run_session" == "/usr/bin/dbus-run-session" ]]
    chromium_version="$(docker run --rm --network none --read-only --cap-drop ALL --entrypoint /bin/sh "$tag" -c 'set -- /ms-playwright/chromium-*/chrome-linux64/chrome; [ "$#" -eq 1 ] && [ -x "$1" ] && "$1" --version' | sed 's/[[:space:]]*$//')"
    [[ "$chromium_version" == "Google Chrome for Testing 143.0.7499.4" ]]
    chromium_version="${chromium_version##* }"
    tool_versions="$(jq -cn --arg driver linux-x11-driver/v1 --arg xvfb "$xvfb_version" --arg chromium "$chromium_version" --arg dbus "$dbus_version" --arg base "$base_image" '{driver:$driver,xvfb:$xvfb,chromium:$chromium,dbus:$dbus,base:$base}')"
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
  echo "rootless Docker did not remove stopped QA probe container after $attempt checks" >&2
  exit 1
fi
probe_id=""
trap - EXIT

# Candidate evidence must use the same registry-format image exporter as
# trusted publication. Docker's local-load manifest digest is not necessarily
# the registry manifest digest. Keep the locally loaded tag for smoke tests.
if [[ "$build_mode" == candidate ]]; then
  registry_metadata="$RUNNER_TEMP/qa-image-registry-candidate.json"
  registry_tag="$repository:candidate-$GITHUB_SHA"
  docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
    --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
    --metadata-file "$registry_metadata" \
    --output "type=image,name=$registry_tag,push=false,rewrite-timestamp=true" "$build_context"
  metadata_file="$registry_metadata"
fi

if [[ "$build_mode" == publish ]]; then
  publish_metadata="$RUNNER_TEMP/qa-image-publish.json"
  docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
    --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
    --metadata-file "$publish_metadata" \
    --output "type=image,name=$tag,push=true,rewrite-timestamp=true" "$build_context"
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
sbom_path="$output_directory/executor-sbom.spdx.json"
# SHA-256 preserves strong file integrity while avoiding redundant SHA-1 file
# metadata entries that can push the SPDX document past GitHub's 16 MiB limit.
SYFT_FILE_METADATA_DIGESTS=sha256 \
  "$SYFT" "docker:$scan_reference" -o "spdx-json=$sbom_path"
sbom_bytes="$(wc -c < "$sbom_path" | tr -d '[:space:]')"
if (( sbom_bytes > 16 * 1024 * 1024 )); then
  printf 'QA image SBOM exceeds the 16 MiB attestation limit: %s bytes\n' \
    "$sbom_bytes" >&2
  exit 1
fi
sbom_sha256="sha256:$(sha256sum "$sbom_path" | awk '{print $1}')"
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
