# Project-aware Docker runtime composition

Implementation tracking: [#529](https://github.com/zbarno/kool.ade/issues/529).

## Current behavior and safety boundary

Kool.ad/e currently executes through an explicitly approved, digest-locked
Linux runtime image. The five general-purpose executor profiles are `general`,
`rust`, `node`, `python`, and `go`, each locked separately for `linux/amd64`
and `linux/arm64`. Browser and desktop QA images are separate policy-gated
drivers, **not** selectable components for arbitrary implementation tasks.

An approved recipe can need more than one toolchain. Selecting a single
language profile fails even when all of the required tools appear across
several approved images. The image plan added by #529 recognizes that
difference:

1. Use the existing approved image when it already provides *all* required
   logical tool IDs. This path retains the existing smoke-receipt requirement.
2. Otherwise, require a digest-locked general foundation matching the Docker
   daemon's Linux architecture.
3. Select the smallest currently known set of approved Rust/Node/Python/Go
   component sources that covers the requested tools. Unknown tool IDs fail
   closed. No agent-supplied registry, Dockerfile, URL or host binary is
   consulted by the planner.
4. Canonicalize duplicate and unordered tool IDs. Compute a stable SHA-256
   **composition recipe key** from exact foundation and source digests,
   their declared versions, target platform, and canonical tool IDs.
5. Report an actionable composite-runtime requirement. **Do not run a task
   using this candidate.**

The computed recipe key is a proposal identity for future cache lookup. It
is **not** a Docker image digest, an executable artifact, an authorization
receipt, or evidence that combining the component images is safe. Changing the
trusted builder's implementation or security policy must change the recipe
identity version. A project's source SHA and approved recipe identity remain
separate execution-authorization inputs; they must not be discarded or shared
merely because two projects have identical tool requirements.

## Still required before enabling dynamic execution

The next implementation stages for #529 must provide all of these:

- An app-owned composition builder that produces an actual Linux image from
  approved, immutable, architecture-matched components. Image sources are
  *inputs*, not automatically compatible layer sets. For example, copying
  binaries from a language image without their native libraries can create
  a broken or misleading runtime.
- Fixed component adapters and exact version checks (not simply inherited
  `tool_versions` metadata), with rootless, bounded, network-isolated build
  and verification. Build instructions may not come from task content.
- A private content-addressed cache of immutable, verified results with
  locking, provenance, policy-version invalidation, and bounded cleanup.
- Explicit linkage from approved target recipes and dependency-broker inputs
  to preparation, with task-scoped approvals and source/revision fences.
- Runtime image registration and independent validation before any
  `ExecutionSession` may mount task content into the new image.
- Automated negative security tests, rootless integration tests, and CI gates
  before replacing the existing language-specific profile selection.

The existing signed Debian package broker and derived-image capability should
be reused where appropriate. It already verifies a restricted system package
closure, but it is not a general multi-language composition backend today.

## Operating policy

Until those stages are complete, keep the existing locked profile images and
their smoke tests in service. A composite recipe is a **blocked preparation
requirement**, never a reason to bypass sandbox policy or execute on the host.

Docker Desktop/Windows/macOS host integration and computer-use VM images are
outside this scope. The Docker executor continues to target Linux containers
and select architecture from the validated daemon, not the host OS name.
