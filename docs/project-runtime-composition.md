# Project-specific verified Docker runtimes

Implementation issue: [#529](https://github.com/zbarno/kool.ade/issues/529).

## Overview

Kool.ad/e runs approved project recipes in isolated Linux containers. It
retains the existing `general`, `rust`, `node`, `python`, and `go`
digest-locked images as the trusted component catalog for `linux/amd64` and
`linux/arm64`. Browser and desktop QA drivers are separate images, with
their own release gates.

A single-language recipe still selects one approved image. When a recipe needs
multiple supported toolchains and no single image can provide them, the local
execution controller creates a **verified composite runtime**. The project
does not specify a Dockerfile, image name, registry URL, build instruction,
privileged setting, or host command.

### Trust and authorization

1. Requirements originate from the application-selected, approved recipe.
   Project metadata alone does not authorize an executable recipe.
2. The planner resolves every logical tool to a component in the bundled,
   architecture-matched, immutable source-image lock. Unknown tools, disabled
   QA profiles, and mismatched architectures fail closed.
3. For each component, the app requires the **same exact image inspection and
   operator-approved smoke receipt** as normal runtime image preparation.
   Missing images go through the existing machine setup task instead of
   an unapproved automatic pull.
4. The application generates its own fixed Dockerfile. It starts with an
   approved toolchain image and copies selected pinned toolchain files from
   other approved component images. Docker BuildKit receives no project
   content, secret provider, host socket, or agent-generated instructions.
   `RUN` is not used; Docker BuildKit's network mode is `none` for build
   instructions. Approved digest references may still require registry
   resolution by Docker if not already present locally.
5. Docker returns an immutable image ID, which the app re-inspects for exact
   content/base labels, Linux OS and architecture. A separate bounded,
   non-root, read-only, no-network container executes fixed commands and
   verifies **actual installed tool versions**, not just claimed metadata.
   A failing probe blocks task execution.
6. A verified image ID is registered in the existing `DockerExecutionBackend`
   for the task's `ExecutionSession`. Every normal workspace, authorization,
   network, process, resource, and secret isolation check still applies.

A composite image does not give another task permission to use the originating
task's clone, credentials, approvals, or output. Every task creates a new
session scoped to its own source and controller authorization.

### Cache behavior

The canonical recipe SHA-256 key includes the target Linux architecture,
foundation and component source digests, exact advertised tool versions, and
canonical logical requirements. A Docker tag under
`koolade-derived:<recipe-hash>` persists in the Docker daemon across
application restarts. Cached image tags are **hints only**. A later task
rechecks locked sources, image labels, image ID/architecture, and installed
tool versions before registering and using the cached result.

The image *recipe* hash is not the OCI image ID. The Docker image ID is
obtained only from the successful build and validation. There is no fallback
to host tools or user-provided images. Toolchain source updates change the
recipe hash, so the old cache entry is no longer selected. Future changes to
the builder and security policy must also invalidate recipe identity.

Composite builds are serialized within an application process and across
processes that share the same private state root. Waiting for the cross-process
lock is nonblocking and bounded, so cancelling a preparation does not hold up
the executor indefinitely. The private cache receipt records the exact recipe,
OCI image ID, architecture, source digests, tool versions, daemon fingerprint,
and last successful verification timestamp.

After each verified preparation, the app enforces a conservative cache budget:
at most **8 unused/retained composite images** or **8 GiB of estimated image
size**, whichever limit is reached first. Eviction prioritizes the oldest
verified receipts, retains the image currently being prepared, and only targets
images with the exact app-owned composition label, matching digest-derived
single tag, and no attached containers. Deletion uses Docker without
`force`. If a container still uses an image, cleanup skips it rather than
breaking the active task, so pressure can temporarily exceed the target
budget. Accounting uses full image sizes and deliberately overestimates
storage when layers are shared. Neither locked base images, system package
extensions, arbitrary user images, nor browser/desktop QA images are eligible
for this cleanup.

An obsolete image never becomes executable because a stale receipt exists:
every lookup revalidates exact image labels, architecture, binary versions,
and the approved image lock. Source or builder/security policy changes
invalidate the content identity and therefore bypass obsolete cached images.
Dependency snapshots remain in the existing separate, broker-verified cache;
they are not copied into the composite image on each source revision.

### Limitations and rollout

- This initial builder covers the four existing approved toolchain sources.
  Unrecognized languages or tools still require a reviewed source adapter or
  broker capability. No arbitrary registry download or unapproved automatic
  system package installation is permitted.
- Source distributions share a Debian Bookworm lineage, but mixing native
  libraries can still produce incompatibilities. A composite must pass the
  runtime version checks and real build/test recipes; success is not inferred
  merely from a generated Dockerfile.
- The signed Debian system-package extension is a separate, task-authorized
  path. It must not be silently conflated with automatic toolchain composition.
- Docker Desktop on Windows/macOS remains separately gated; these are Linux
  container images, not native Windows or macOS images.
- Browser/desktop QA release flags are unchanged. Image composition is not
  independent visual QA evidence.
- Keep the old locked profiles available for existing and in-flight tasks.
  If provisioning fails, show the missing/invalid setup requirement, never
  launch in an unverified container.
