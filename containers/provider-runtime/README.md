# Provider Runtime Image (placeholder)

Recipe staging for the rootless-Docker provider CLI runtime (Issue #591,
`docs/provider-execution-containment-v1.md` phase 2).

**Deliberately not wired.** Until ops publishes a signed image and supplies
registry-authentic digests, `containers/images.lock.json` intentionally does
not contain a provider runtime profile, and
`src/execution/docker/provider_session.rs` refuses any mutation against an
unpinned image (`ImageNotPinned`). The existing fail-closed loader treats
unknown profiles as invalid, so there is no path from this folder to an
authorized runtime today. See `Dockerfile.template` for the promotion flow.
