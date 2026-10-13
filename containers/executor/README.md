# Trusted executor images

`Dockerfile` is the only approved executor recipe. CI reads its architecture-specific
source bases from `containers/base-images.lock.json`; runtime policy reads approved
image references from `containers/images.lock.json`. The build context is this
directory and never a task repository. Base pins are separate so a reviewed derived
image can replace the runtime reference without changing what the recipe builds from.

The source lock pins Docker Official Image manifests for Debian bookworm, Rust
1.98.1, Node 24.21.0, Python 3.14.7 slim-bookworm, and Go 1.26.8 bookworm on
Linux amd64 and arm64. Each architecture has a
separate deterministic candidate digest. The pull request workflow produces and
checks those digests before merge; the trusted `main` workflow publishes the same
bytes and attaches external attestations. Rust entries correspond to platform
manifests from the upstream Rust 1.98.1 bookworm index.

These are the only language toolchains with approved executor profiles. Java,
.NET, CMake, Flutter, and other discovered stacks remain `MissingToolchain` until
an approved image or dependency-broker capability is available. The image recipe
removes inherited `curl`, `wget`, and `git` packages from toolchain profiles
without network access. No profile includes a Docker client or daemon, SSH daemon,
or `sudo`; credentials and host sockets are never copied into the image.

The `Trusted executor images` workflow builds each OCI candidate on a verified
rootless Docker Engine and executes that exact candidate with no network, a
read-only root filesystem, no capabilities, and bounded resources. BuildKit SBOM
and provenance attestations are disabled for the image payload because their
timestamps change the OCI index digest across identical builds. CI retains an
SPDX SBOM and build record as artifacts; a reviewed lock update pins the
deterministic image digest. A push to `main` rebuilds the same digest, checks it
against the lock, publishes it, and attaches external GitHub provenance and SBOM
attestations. Those attestations do not alter the image digest.

Download the combined manifest and twelve per-image evidence artifacts from the
same workflow run. The evidence root must contain directories named
`executor-image-candidate-<profile>-<architecture>`, each with
`executor-image.json`, `executor-sbom.spdx.json`, and
`executor-provenance.json`. The manifest artifact is in
`executor-image-evidence-manifest/executor-image-candidates.json`.
The QA directories are `executor-image-candidate-browser_qa-amd64` and
`executor-image-candidate-desktop_qa-amd64`; desktop evidence also contains the
exact `koolade-qa-desktop` binary included in its recipe hash. Keep these files
in the evidence root when validating or promoting.

```sh
gh run download <run-id> --dir candidate-artifacts
python3 scripts/promote-image-digests.py \
  candidate-artifacts/executor-image-evidence-manifest/executor-image-candidates.json \
  --evidence-root candidate-artifacts \
  --source-revision <full-git-revision> \
  --require-profiles general rust node python go browser_qa desktop_qa
python3 scripts/promote-image-digests.py \
  candidate-artifacts/executor-image-evidence-manifest/executor-image-candidates.json \
  --evidence-root candidate-artifacts \
  --source-revision <full-git-revision> \
  --require-profiles general rust node python go browser_qa desktop_qa --promote
python3 scripts/promote-image-digests.py \
  candidate-artifacts/executor-image-evidence-manifest/executor-image-candidates.json \
  --evidence-root candidate-artifacts \
  --require-profiles general rust node python go browser_qa desktop_qa --check-lock
```

For a local native-architecture candidate from the repository root:

```sh
profile=rust
platform=linux/amd64
base_image="$(jq -er --arg profile "$profile" --arg platform "$platform" \
  '.profiles[$profile][] | select(.platform == $platform) | .reference' \
  containers/base-images.lock.json)"
docker buildx build --platform "$platform" --network=none \
  --provenance=false --sbom=false --build-arg SOURCE_DATE_EPOCH=0 \
  --build-arg "BASE_IMAGE=$base_image" \
  --build-arg "EXECUTOR_PROFILE=$profile" \
  --load \
  --tag koolade-executor:local containers/executor
docker run --rm --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges --entrypoint /bin/sh \
  koolade-executor:local -c 'id; rustc --version; cargo --version'
```

For an arm64 local build, use an arm64 host or a Buildx builder configured for
arm64 emulation; CI uses native amd64 and arm64 runners.

Runtime creation must set UID/GID `10001:10001`, drop every capability, disable
network access and mount only the authorized task workspace. Browser and desktop
QA images use dedicated linux/amd64 profiles. They remain absent from runtime
policy until rootless candidates pass and both digests are promoted through a
reviewed lock update. Trusted-main CI then rebuilds under rootless Docker,
checks exact reviewed digests, and attaches external GitHub provenance and SPDX
SBOM attestations. PR workflows never publish QA candidate images.

## Rebuild and provenance

1. Review base-image updates and refresh both architecture digests in
   `base-images.lock.json`. Do not use `latest` or tag-only inputs.
2. Let the pull request workflow build and run the candidate on rootless Docker,
   checking platform, UID/GID, declared tool versions, absent curl/Docker/SSH-daemon/sudo
   tools, no Docker socket, and a read-only root filesystem under bounded settings.
3. Review the pre-merge OCI digest, SPDX SBOM, and build record. The recipe and
   candidate-generation pull request retains this evidence without publishing
   from untrusted code.
4. Promote the reviewed digests in a follow-up lock-update pull request. Its
   builds must match the lock before the trusted-main publisher is enabled;
   then inspect the GitHub provenance and SBOM attestations.
5. Rebuild on a regular security-update cadence and promptly for critical
   base/toolchain advisories. Retain prior digest records and attestations for
   rollback. Never put credentials in build arguments or layers.

Runtime accepts only the exact reference in `images.lock.json`; commit tags are
publication labels for review and never runtime selectors. Production must use
the trusted derived-image registry after all runtime entries have been promoted.
