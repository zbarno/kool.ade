#!/usr/bin/env bash
set -euo pipefail

profile="${1:?profile required}"
platform="${2:?platform required}"
base_image="${3:?locked base image required}"
architecture="${platform#linux/}"
test "$platform" = linux/amd64
[[ "$base_image" =~ @sha256:[0-9a-f]{64}$ ]]
[[ "${DOCKER_CONFIG:-}" == "$RUNNER_TEMP/qa-image-docker-config" ]]

docker info --format '{{json .SecurityOptions}}' \
  | jq -e 'any(.[]; contains("name=rootless"))' >/dev/null

output_directory="$RUNNER_TEMP/executor-image-candidate-${profile}-${architecture}"
record="$output_directory/executor-image.json"
test -s "$record"
test "$(jq -er '.profile' "$record")" = "$profile"
test "$(jq -er '.platform' "$record")" = "$platform"
test "$(jq -er '.source_revision' "$record")" = "$GITHUB_SHA"

buildkit_image="moby/buildkit@sha256:b059f8d7226d0b326bc871af489a5dddf65e36af4d20d14251b072974d9ee87c"
builder="qa-${profile}-publisher"
tag="koolade-runtime-qa:${profile}-candidate"
case "$profile" in
  browser_qa)
    build_context=containers/runtime-qa/browser
    repository=ghcr.io/zbarno/koolade-runtime-qa-browser
    ;;
  desktop_qa)
    repository=ghcr.io/zbarno/koolade-runtime-qa-desktop
    build_context="$RUNNER_TEMP/qa-desktop-publish-context"
    mkdir -p "$build_context"
    cp containers/runtime-qa/desktop/Dockerfile "$build_context/Dockerfile"
    cp "$output_directory/koolade-qa-desktop" "$build_context/koolade-qa-desktop"
    ;;
  *)
    echo "unsupported QA image profile: $profile" >&2
    exit 2
    ;;
esac

find "$build_context" -exec touch -h -d '@0' {} +
docker buildx create --name "$builder" --driver docker-container \
  --driver-opt "image=$buildkit_image" --use
docker buildx inspect --bootstrap

tag="${repository}:trusted-${GITHUB_SHA}"
metadata_file="$RUNNER_TEMP/qa-image-publish.json"
docker buildx build --platform "$platform" --network=none --provenance=false --sbom=false \
  --build-arg SOURCE_DATE_EPOCH=0 --build-arg "BASE_IMAGE=$base_image" \
  --metadata-file "$metadata_file" \
  --output "type=image,name=$tag,push=true,rewrite-timestamp=true" "$build_context"

digest="$(jq -er '."containerimage.digest"' "$metadata_file")"
[[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]
expected_digest="$(jq -er --arg profile "$profile" \
  '.profiles[$profile][] | select(.platform == "linux/amd64") | .digest' containers/images.lock.json)"
test "$digest" = "$expected_digest"
image_reference="$repository@$digest"
docker pull --platform "$platform" "$image_reference"

test -s "$output_directory/executor-sbom.spdx.json"
sbom_sha256="sha256:$(sha256sum "$output_directory/executor-sbom.spdx.json" | awk '{print $1}')"
jq --arg digest "$digest" --arg reference "$image_reference" \
  --arg sbom "$sbom_sha256" '. + {digest:$digest,reference:$reference,sbom_sha256:$sbom}' \
  "$record" > "$record.tmp.json"
mv "$record.tmp.json" "$record"

jq -n --slurpfile candidate "$record" \
  '{subject:{profile:$candidate[0].profile,platform:$candidate[0].platform,reference:$candidate[0].reference,digest:$candidate[0].digest},source_revision:$candidate[0].source_revision,sbom_sha256:$candidate[0].sbom_sha256,base_image:$candidate[0].base_image,build_recipe_sha256:$candidate[0].build_recipe_sha256}' \
  > "$output_directory/executor-provenance.json"
provenance_sha256="sha256:$(sha256sum "$output_directory/executor-provenance.json" | awk '{print $1}')"
jq --arg provenance "$provenance_sha256" '. + {provenance_sha256:$provenance}' \
  "$record" > "$record.tmp.json"
mv "$record.tmp.json" "$record"

printf 'Published QA image digest %s for %s.\n' "$digest" "$profile" >> "$GITHUB_STEP_SUMMARY"
