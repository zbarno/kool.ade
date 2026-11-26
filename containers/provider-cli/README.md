# Provider CLI runtime image

This image packages the official Linux Codex and Claude Code native binaries
for amd64 or arm64. `packages.lock.json` pins each npm tarball by exact URL,
version, and SHA-512 integrity. `prepare.py` downloads only those tarballs,
verifies integrity before reading them, and extracts only the expected regular
binary file. It rejects links, unexpected paths, oversized archives, and
unexpected output files.

The candidate workflow runs a separate rootless-container fixture with a
synthetic managed hook and child process. It verifies the configured task view
is read-only, a sibling host credential directory and Docker socket are absent,
container scratch remains writable, outbound networking is denied, and the
container has bounded CPU, memory, and process count. This validates those
Docker settings for the candidate image only. It does not prove that the
application owns provider-container lifecycle, that Codex or Claude invokes a
managed hook inside it, or that model authentication and transport are safely
mediated. Both providers remain blocked until those application boundaries are
implemented and verified.

Prepare a local build context on a network-connected machine:

```sh
python3 containers/provider-cli/prepare.py \
  --architecture x64 \
  --output containers/provider-cli/binaries
docker build --network=none \
  --build-arg BASE_IMAGE='docker.io/library/node@sha256:<reviewed-digest>' \
  --build-arg CODEX_VERSION=0.167.1 \
  --build-arg CLAUDE_VERSION=2.1.322 \
  --tag koolade-provider-cli:local containers/provider-cli
```

The generated binaries are transient build inputs and must not be committed.
The image is only a binary-delivery artifact. It does not configure auth,
network access, provider readiness, or task execution; Codex and Claude remain
blocked until their process containment and model/auth transport are enforced.

The candidate workflow exports a single-platform OCI image for each architecture
and checks the loaded image configuration against that manifest before running
the smoke tests. Each successful job retains the manifest bytes, an SPDX SBOM,
and a candidate record in the corresponding `provider-cli-evidence-*` artifact.
For promotion review, collect both artifacts from the same successful run and
attempt on the intended source revision. Verify the manifest and SBOM SHA-256
digests against each candidate record. The smoke checks cover CLI startup and
synthetic child-process containment. Signed-in task execution requires separate
application and credential-mediation evidence.

### Promote a tested candidate

Dispatch `provider-cli-image.yml` on `main`. Once both architecture jobs have
passed, download the evidence before its 30-day retention period expires.
Only a main dispatch retains the OCI layers needed for promotion. To prepare
the release record in a topic branch, set `RUN_ID` to the successful run ID
and execute the following commands from the repository root with Bash and jq.

```bash
set -euo pipefail
evidence="$(mktemp -d)"
gh run download "$RUN_ID" --pattern 'provider-cli-evidence-*' --dir "$evidence"
gh api "repos/zbarno/kool.ade/actions/runs/$RUN_ID" > "$evidence/run.json"
platforms='{}'
for arch in x64 arm64; do
  candidate="$evidence/provider-cli-evidence-$arch/candidate.json"
  hash="sha256:$(sha256sum "$candidate" | cut -d' ' -f1)"
  platforms="$(jq --argjson prev "$platforms" --arg hash "$hash" \
    '$prev + {(.platform): {manifestDigest, candidateDigest: $hash}}' "$candidate")"
done
jq --argjson platforms "$platforms" \
  '{schemaVersion:1,sourceRevision:.head_sha,runId:.id,runAttempt:.run_attempt,$platforms}' \
  "$evidence/run.json" > "$evidence/release.json"
for arch in amd64 arm64; do
  suffix="$arch"
  if [[ "$arch" == amd64 ]]; then suffix=x64; fi
  python3 containers/provider-cli/verify-release.py "$evidence/release.json" \
    "$evidence/run.json" "$evidence/provider-cli-evidence-$suffix" "$arch"
done
cp "$evidence/release.json" containers/provider-cli-release.json
```

Submit the release record for review. Its pull request verifies both retained
images against the record. After the reviewed record lands on main, the
publication workflow copies those images to GHCR by verified digest and
attests their provenance and SBOM. App admission still requires review of the
published image, attestations, and application containment evidence.
