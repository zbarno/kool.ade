# Project runtime capability contract

Kool.ad/e records project targets and recipes independently of the source
language. Discovery reads repository metadata only; it does not run project
scripts, a `Dockerfile`, or Compose configuration. A detected manifest is
evidence for a candidate, not proof that a build or runtime works.

## Recipe boundary

`ProjectCapabilityManifestV1` is tied to a workspace, repository, source SHA,
and revision. Each `AppTargetV1` selects a surface (`web`, `linux_desktop`,
`headless_service`, or `unsupported_target`) and references build, test, start,
readiness, and artifact policies. Surfaces describe the application that a
driver interacts with; they do not imply a particular language or framework.

`discover_manifest` performs bounded, read-only inspection of common project
metadata (including Cargo, package.json, Python, Go, Java, .NET, CMake, Flutter,
and build/Compose metadata). It takes workspace, repository, task, and source
identity from its caller; it never guesses those from files. Results are
deterministic candidate targets with evidence paths. Discovery does not infer
commands, install tools, run repository scripts, or mark a target ready. An
unrecognized project can still be described with a manually supplied target;
metadata inspection is open ended and is not a closed language allowlist.

`ProjectRuntimeStore::discover_and_save` persists candidate manifests with a
revision fence. `apply_user_recipe_override` and
`ProjectRuntimeStore::approve_recipe_and_save` bind a validated recipe to one
target and operation only when its source is explicitly `user_approved`.
Approval evidence is recorded on that recipe. These controller-facing APIs
preserve task/source identity and do not themselves grant Docker or
dependency-broker capabilities.
`add_user_target_override` remains a controller-level proposal helper.
`ProjectRuntimeStore::add_user_target_and_save` accepts a user `GateReceipt`
whose subject hash covers the target, recipes, readiness probe, task,
workspace, repository, and source identity. The store writes the receipt and
manifest atomically with revision fences, while a separate operator-local
record binds the same proposal and receipt to the current user. Execution
requires both records and fails closed if the local approval is absent, stale,
or revoked; shared planning JSON alone cannot authorize a `user-target-*`.
A caller-provided approval string alone cannot authorize a custom target. The
readiness-aware variant also persists the matching probe atomically. Task
Details exposes a custom-target form for selecting Web, Linux desktop, or
Headless service and a repository-relative app root. Build and test recipes
can be omitted independently, but at least one complete recipe is required.
A Start recipe requires a Build recipe because Start consumes verified build
artifacts. Each recipe needs a logical executable ID and discrete argv, and is
validated and saved with the same user-approval evidence and revision fence as
recipes for detected targets. Build artifact paths are relative to the app
root. Start recipes also save a surface-appropriate readiness probe (HTTP for
Web, local health for services, process alive for Linux desktop). The new
target remains `ReadyButUntested`; approval is not QA evidence. Target
selection remains an explicit Task Details action when the manifest has
multiple targets.

`ExecutionRecipeV1` stores a logical executor tool ID and discrete arguments.
It rejects shell-evaluation switches, absolute or parent-traversing paths,
protected environment variables, unbounded timeouts, and Git metadata output.
Recipe working directories and output paths are relative to the selected
target's project root.
`ProjectRuntimeExecutor` resolves the selected target and recipe from the saved
manifest, checks the task and source SHA against the assigned workspace, rejects
metadata-only recipes, and dispatches discrete argv through a Docker
`ExecutionSession`. The schema alone does not authorize execution. User
approval and prerequisite-broker integration still belong to the application
lifecycle and must happen before the executor is called.

Example recipe payload:

```json
{
  "schema_version": 1,
  "recipe_id": "web-build",
  "source": "user_approved",
  "working_directory_relative": ".",
  "executable_id": "npm",
  "argv": ["run", "build"],
  "allowed_env_keys": [],
  "allowed_output_paths": ["dist"],
  "timeout_seconds": 300,
  "expected_exit_codes": [0],
  "expected_artifacts": ["dist/index.html"],
  "cache_scope": null,
  "prerequisite_tool_ids": ["node", "npm"],
  "network_requirement": "denied",
  "evidence_refs": ["apps/site/package.json", "user-approval:decision-uuid"]
}
```

Build and test attempts use separate `RuntimeReceiptV1` records tied to the
task, target, and immutable source SHA. A passing build or test requires an
approved exit code and all declared artifacts. The Docker backend collects only
the exact recipe or target artifact paths selected by the application. It parses
the bounded returned workspace archive, rejects missing or unsafe files, and
records each artifact's size, media type, and SHA-256 before synchronization.
Runtime receipt paths are relative to the repository clone so monorepo targets
remain addressable by both Start and QA.
Queued build and test recipes use the generic executor. Its image selection
matches each approved recipe's logical tool requirements against pinned image
inventory instead of inferring a language from executable names. A custom tool
can be recorded without a Kool.ad/e code change; execution still fails closed
unless an approved image advertises that tool. Missing tools currently produce
an unavailable-image result; they are not yet converted into Man.ager broker
requests.

The generic start executor requires a passing Build receipt for the same task,
target, source SHA, and recipe. It launches the approved process in the Docker
workspace, captures the exact build artifact paths again, and requires their
hashes to match before recording readiness. The bounded in-session process is
then terminated and reaped. The task queue advances approved Build, Test, and
Start recipes in order, retains receipts for every attempt, and refuses to
start until the matching build artifacts are verified.
HTTP readiness uses an application-generated loopback request through a declared
Node or Python standard-library prerequisite in the selected executor image;
it never downloads or invokes curl. Targets without that approved prerequisite
fail closed for HTTP readiness. Process health alone does not mean functional
or visual QA passed. Capability assessment uses an
explicit inventory of toolchains, dependencies, and surface drivers from the
approved execution environment and task activity; it never consults host
`PATH`. The queued QA prerequisite gate applies this assessment to the selected
task target and reviewed source SHA using toolchain and driver IDs from the
pinned image lock. A target's `required_dependencies` entries identify
dependency-broker request IDs. QA considers one available only when the same
task's persisted activity records that exact request as `Prepared`, its
preparation telemetry reports `Prepared`, and its retained cache snapshot is
retrievable and passes manifest and archive integrity checks. Authorization,
another task's request, missing telemetry, a
missing or corrupt snapshot, or a failed preparation never marks a dependency
available. Toolchain gaps are still reported as unavailable-image results and
are not yet converted into Man.ager broker requests.
Unsupported native mobile, Windows/macOS desktop, and hardware targets must be
reported as unsupported until their execution and QA drivers exist.

## Current implementation boundary

The bundled executor image lock currently provides Rust 1.98.1, Node 24.21.0,
Python 3.14.7, and Go 1.26.8 profiles for Linux amd64 and arm64, plus the general
shell profile. These profiles provide toolchains for recipes; they do not prove
that a particular project builds or starts. Java, .NET, CMake, Flutter, and
other detected stacks remain `MissingToolchain` unless their required tools are
available through a separately approved capability inventory. Web projects can
remain `DriverUnavailable` while the browser QA profile is disabled; Linux
desktop visual QA likewise requires the separately approved desktop driver.

The deterministic fixture matrix covers Rust desktop, TypeScript/Node web,
Python web, Go service, and a two-target TypeScript/Python monorepo. Discovery
only reads metadata and leaves all candidates `ready_but_untested`; the web
fixture's build script is never executed. The typed contracts, bounded
metadata-only discovery, revision-fenced manifest and receipt persistence,
explicit task target selection and recipe/readiness approval in Task Details,
capability assessment, Docker-backed build/test/start dispatch, and a bounded
start/readiness executor are present. The task queue dispatches Start only
after the exact Build and any declared Test receipts pass, and requires a passed
start/readiness receipt before creating the visual QA request. Each receipt is
bound to the selected target, task, source SHA, and approved recipe. Recipe
approval records discrete argv and an explicit user-approval reference in the
shared manifest plus an app-owned local approval record bound to the complete
recipe and target identity; a shared marker by itself cannot authorize dispatch.
Approval does not launch the recipe. Bounded Docker artifact collection feeds Build/Test and
Start/readiness receipts. The existing QA artifact snapshot verifies those
hashes before uploading only the selected files to web preview or desktop sessions.
Task-scoped npm and Cargo cache snapshots from successful broker preparations
are retained with private permissions, integrity-checked against the exact
request, and bound into the immutable Runtime QA request identity. The retained
store is capped at 512 snapshots and 1 GiB; it evicts least-recently-used
snapshots to admit new entries within those limits and removes abandoned staging
directories before the next new snapshot is admitted. Access is refreshed when
a snapshot is read or an identical immutable snapshot is written. Web QA
transfers those archives into the isolated service container and configures the
matching package manager for offline cache use. This makes prepared cache data
available to an approved startup recipe; it does not install dependencies or
prove the recipe consumed them. A recipe must explicitly perform any required
offline install, and uncached dependencies fail in the no-egress QA network.
Desktop QA does not receive package caches. Headless service evidence also
remains unintegrated.
Until released QA image digests and the remaining headless and application-level
runtime evidence paths land, the application must not present a detected
candidate as a verified runnable target or claim runtime QA coverage.
