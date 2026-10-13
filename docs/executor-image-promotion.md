# Executor image digest promotion

Executor digests are production policy: selecting one permits its contents to
run inside a bounded task container. A mutable tag or a build artifact without
provenance is not a promotion candidate.

For each supported profile and architecture, CI must retain a record containing
the exact trusted repository digest, locked base image, source revision, recipe
digest, tool versions, SBOM digest, and provenance digest. Reviewers inspect the
candidate evidence before updating `containers/images.lock.json`.

The pull request build creates and rootlessly executes each candidate locally,
then uploads the image record, SPDX SBOM, and provenance record as a retained
workflow artifact. It does not publish candidate images from an untrusted pull
request. This first review unit validates the evidence and makes it available
for a later lock-update pull request. Trusted-main publication stays skipped
until the lock contains the promoted derived-image digests. The lock-update PR
must rerun the builds and compare each result with the reviewed digest before
it can merge.

QA candidates also probe DNS, external TCP egress, loopback and bridge Docker
API addresses while attached to `--network none`, then start and stop a
short-lived container through the rootless daemon and require automatic
removal. This validates the configured isolation and container teardown path;
hosted rootless job results remain the execution evidence for each candidate.

The verifier uploads a combined manifest as `executor-image-evidence-manifest`
and retains the per-image artifact directories named
`executor-image-candidate-<profile>-<architecture>`. Download both artifact
groups from the same successful workflow run. The promotion tool verifies the
sidecar file hashes and that each provenance record binds its candidate digest,
source, base, recipe, and SBOM.

The same rootless candidate workflow also builds `browser_qa` and
`desktop_qa` for linux/amd64. Their records live beside the executor records in
`executor-image-candidate-browser_qa-amd64` and
`executor-image-candidate-desktop_qa-amd64`. The desktop artifact includes the
exact `koolade-qa-desktop` binary used in its recipe hash. These QA profiles
remain absent from the runtime lock until both have real candidate evidence and
are promoted together in a reviewed lock update. Promotion sets
`runtime_enabled: false`; a reviewed image digest does not by itself approve
runtime QA. Trusted-main publication then rebuilds under rootless Docker,
compares against the reviewed digest, and attaches GitHub provenance and SPDX
SBOM attestations while the runtime remains disabled. Enable a QA profile only
after the separate #171 runtime-QA release gate passes and its result has been
reviewed. No workflow publishes an untrusted PR candidate.

```sh
gh run download <run-id> --dir candidate-artifacts
```

The combined JSON document has this shape:

```json
{
  "schema_version": 1,
  "images": [
    {
      "profile": "general",
      "platform": "linux/amd64",
      "reference": "ghcr.io/zbarno/koolade-executor@sha256:<64 lowercase hex characters>",
      "digest": "sha256:<same 64 lowercase hex characters>",
      "base_image": "docker.io/library/debian@sha256:<locked base digest>",
      "source_revision": "<40 lowercase hex characters>",
      "build_recipe_sha256": "sha256:<recipe digest>",
      "sbom_sha256": "sha256:<evidence digest>",
      "provenance_sha256": "sha256:<evidence digest>",
      "tool_versions": {"shell": "POSIX /bin/sh", "base": "Debian bookworm-slim"}
    }
  ]
}
```

Validate a candidate without changing the lock, then prepare the reviewed lock
update:

```sh
python3 scripts/promote-image-digests.py candidate-artifacts/executor-image-evidence-manifest/executor-image-candidates.json --evidence-root candidate-artifacts --source-revision <full-git-revision> --require-profiles general rust node python go browser_qa desktop_qa
python3 scripts/promote-image-digests.py candidate-artifacts/executor-image-evidence-manifest/executor-image-candidates.json --evidence-root candidate-artifacts --source-revision <full-git-revision> --require-profiles general rust node python go browser_qa desktop_qa --promote
git diff -- containers/images.lock.json
```

After applying the reviewed digest update, CI rebuilds the same deterministic
image and verifies every produced digest against the lock. QA runtime enablement
is a separate reviewed change after the #171 release gate:

```sh
python3 scripts/promote-image-digests.py candidate-artifacts/executor-image-evidence-manifest/executor-image-candidates.json --evidence-root candidate-artifacts --require-profiles general rust node python go browser_qa desktop_qa --check-lock
```

The validator rejects missing/duplicate profile-platform pairs, untrusted
registries, mismatched base images, recipe drift, sidecar hash mismatches,
provenance that does not bind the image and evidence, incomplete tool metadata,
and Rust versions other than the pinned toolchain.
Promotion only edits the lock; it does not publish images or make the review
decision. The resulting manifest diff must be reviewed with the retained CI
SBOM and provenance evidence before it is merged.
