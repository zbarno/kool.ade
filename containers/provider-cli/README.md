# Provider CLI runtime image

This image packages the official Linux Codex and Claude Code native binaries
for amd64 or arm64. `packages.lock.json` pins each npm tarball by exact URL,
version, and SHA-512 integrity. `prepare.py` downloads only those tarballs,
verifies integrity before reading them, and extracts only the expected regular
binary file. It rejects links, unexpected paths, oversized archives, and
unexpected output files.

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
