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
unrecognized project can still be described with a manually supplied target.
Node packages with a regular `src/index.html` entry point are also considered
web candidates, and both the package manifest and HTML entry point are recorded
as evidence. Discovery does not follow symlinks when checking that entry point;
metadata inspection is open ended and is not a closed language allowlist.

`ProjectRuntimeStore::discover_and_save` persists candidate manifests with a
revision fence. `apply_user_recipe_override` and
`ProjectRuntimeStore::approve_recipe_and_save` bind a validated recipe to one
target and operation only when its source is explicitly `user_approved`.
Approval evidence is recorded on that recipe. These controller-facing APIs
preserve task/source identity and do not themselves grant Docker or
dependency-broker capabilities.
For detected targets, the Build approval form accepts a bounded list of
expected output paths relative to the app root. The same canonical paths are
stored as the output allowlist and expected artifacts, so the build receipt can
only pass after those files are captured and hashed.
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
artifacts. Build and Test recipes use logical tool IDs and discrete argv. A
Start recipe may use either a logical tool ID or a canonical executable path
relative to its recipe working directory; path-based Start requires explicit
user approval and the executable must be one of the exact files in that
source-bound Build receipt. Arguments are discrete argv items and may be empty.
Build artifact paths are relative to the app root. Start recipes also save a
surface-appropriate readiness probe (HTTP for Web, local health for services,
process alive for Linux desktop). HTTP readiness declares a locked Node,
Python, or Go probe tool independently of the app executable, so a compiled
binary does not need to masquerade as an image tool. The Go probe is built
before the task process starts. The runner copies itself to a write-sealed
Linux memfd, re-executes from it, then verifies its non-dumpable state before
launching the approved app process and probing localhost. Project code cannot
replace the app-owned health check through same-UID procfs access or executable
file writes. Its build uses the locked Go toolchain, an executable workspace
temp directory, and no module or network lookup; unsupported memfd or procfs
execution fails closed. The new target remains
`ReadyButUntested`; approval is not QA evidence. Target selection remains an
explicit Task Details action when the manifest has multiple targets.

For a manual target with a Build recipe, the files entered in the artifact
list are also the exact Build stage-out allowlist. The executor may create their
missing parent directories, but it rejects edits to sibling files in those
directories. Test-only targets omit the artifact spec; Start still requires a
Build recipe and its verified artifact list.

For a discovered target with no recipes, Task Details offers a small trusted
test adapter only when the checked-out metadata identifies Rust, JavaScript,
Python, or Go at that exact target root. Applying it is a separate explicit
user action. The controller rescans the selected task checkout, uses a fixed
discrete argv template with network denied, and asks the runtime store to bind
the recipe to locally recorded discovery provenance. Unknown stacks keep the
manual recipe path. Build and Start stay on that path because their artifact
allowlist and surface-specific readiness require project-specific values. The
adapter action records an approved recipe; it does not execute it or create a
passing build/test receipt.

`ExecutionRecipeV1` stores a logical executor tool ID, or a target-relative
executable path for an explicitly approved Start recipe, plus discrete
arguments. Path-based Start is launched through the application-generated
bounded runtime launcher; it cannot be used as a Build or Test command. The
launcher requires the path to match the exact Build receipt artifact list and
quotes it as one argv value. Recipes reject shell-evaluation switches,
absolute or parent-traversing paths, protected environment variables,
unbounded timeouts, and Git metadata output. Recipe working directories and
output paths are relative to the selected target's project root.
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

A compiled app can use a target-relative Start executable after Build, Test, and
Start approvals are recorded. `target/release/site-server` must also appear in
the target's approved artifact specification, which becomes the exact receipt
and QA upload allowlist:

```json
{
  "schema_version": 1,
  "recipe_id": "rust-web-start",
  "source": "user_approved",
  "working_directory_relative": ".",
  "executable_id": "target/release/site-server",
  "argv": [],
  "allowed_env_keys": ["HOST", "PORT"],
  "allowed_output_paths": [],
  "timeout_seconds": 90,
  "expected_exit_codes": [0],
  "expected_artifacts": [],
  "cache_scope": null,
  "prerequisite_tool_ids": ["python3"],
  "network_requirement": "approved_service_ports",
  "evidence_refs": ["user-approval:decision-uuid"]
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
unless an approved image advertises that tool. Before a queued Build, Test, or
Start attempt opens a Docker session, missing target-declared toolchains are
recorded as stable Man.ager review requests bound to the workspace, target, and
source SHA. The requests identify the tool but contain no install command,
preparation, or authority to execute one. An exact compatible reviewed image
or separately approved broker adapter is still required before execution.

The generic start executor requires a passing Build receipt for the same task,
target, source SHA, and recipe. It launches the approved process in the Docker
workspace, captures the exact build artifact paths again, and requires their
hashes to match before recording readiness. The bounded in-session process is
then terminated and reaped. The task queue advances approved Build, Test, and
Start recipes in order, retains receipts for every attempt, and refuses to
start until the matching build artifacts are verified.
HTTP readiness uses an application-generated loopback request through a declared
Node, Python, or Go standard-library prerequisite in the selected executor
image; it never downloads or invokes curl. Targets without that approved
prerequisite fail closed for HTTP readiness. Process health alone does not mean
functional or visual QA passed. Capability assessment uses an
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
available. Missing target-declared toolchains are routed to the same
source-bound, non-executable Man.ager review queue; QA remains blocked until the
approved runtime inventory can satisfy the target recipes.
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
only reads metadata and leaves all candidates `ready_but_untested`; the
discovery-only web fixture's build script is never executed. Ignored rootless
candidate tests build and test TypeScript, Python, and Rust web targets through
the same browser driver, and run a Go API through the headless service driver.
The polyglot test executes both nested targets, checks all six receipts against
their target and source, and passes each target through production QA request
and artifact preparation before the driver. The TypeScript fixture uses Node
24.21's built-in type stripping for erasable
TypeScript syntax; it does not perform `tsc` type checking ([Node.js 24.21
TypeScript support](https://nodejs.org/download/release/v24.21.0/docs/api/typescript.html)).
Its network-denied Build performs `npm ci --offline` for a synthetic
lockfile-pinned package using the exact selected broker cache snapshot, then
produces artifacts. This proves one cache-backed locked install, not general
npm registry installation. Another ignored rootless test
builds a Rust desktop target in the locked Rust image, validates its Build,
Test, and Start receipts, prepares the request through the production runtime
QA path, and launches the exact artifact with the locked desktop driver. Exact
head hosted results are required before treating these configured tests as
passed. Separate deterministic acceptance fixtures cover the control-plane
workflow on Python-only source: `TurnController` interview and task generation
in a Git workspace with no Cargo or Node manifest, followed by persisted code
review, QA, specification reconciliation, and PR preparation bound to Python
source hashes. They use local fixture harnesses and temporary stores, with no
Docker, network, or model API; they do not publish a GitHub PR. These candidate
tests do not exercise #166's full worker, scenario
synthesis, and independent reviewer persistence path. The typed contracts, bounded
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
Task-scoped npm, Cargo, Pip, and Go cache snapshots from successful broker
preparations are retained with private permissions, integrity-checked against the exact
request, and bound into the immutable Runtime QA request identity. The retained
store is capped at 512 snapshots and 1 GiB; it evicts least-recently-used
snapshots to admit new entries within those limits and removes abandoned staging
directories before the next new snapshot is admitted. Access is refreshed when
a snapshot is read or an identical immutable snapshot is written. Web QA
transfers those archives into the isolated service container and configures the
matching package manager for offline cache use. This makes prepared cache data
available to an approved recipe; each recipe must perform any required offline
install, and uncached dependencies fail in the no-egress QA network. The
TypeScript fixture performs and verifies one such install during Build.
The dependency broker also supports exact-pinned `requirements.txt` restores:
its resolver targets the pinned Python 3.14.7 executor and retrieves SHA-256-
locked universal wheels from the approved PyPI hosts,
verifies them independently, and retains a lock-specific wheelhouse snapshot.
The rootless Pip integration test installs that snapshot in the locked Python
image with package-index access disabled. Go module restores are limited to
exact `go.mod`/`go.sum` existing restores: `h1` checksums are verified through
the approved `proxy.golang.org` and `sum.golang.org` endpoints, with direct VCS
fallback disabled. Replacements, workspaces, vendor trees, private Go settings,
toolchain downloads, and non-exact requirements fail closed. The rootless Go
integration fixture transfers a preverified UUID module cache, runs `go mod
verify` offline, and builds in a network-denied Go runtime. Alternate Python
lock formats, platform-specific wheels, and NuGet
package acquisition remain unsupported and fail closed.
Desktop and headless-service QA receive only the exact request-bound snapshots
selected for the same task, with integrity and the actual executor image
architecture rechecked before staging into read-only-rootfs container tmpfs.
Requests can carry one retained cache per supported architecture; only the
cache matching the selected Docker image is staged. Desktop processes receive
ecosystem cache paths with offline-only settings; uncached dependencies still
fail in the network-disabled container. For a headless service target, the app can create
an explicit visual-QA-not-applicable decision after an independent read-only
reviewer checks the exact active manifest target and its latest source-bound
passing Build and Test receipts. A Manager records the decision as a durable
`NotApplicable` visual-QA status and advances the task to spec reconciliation.
This does not replace service testing. When an approved Start recipe has a
`LocalServiceHealth` probe, the app can also run source-bound API scenarios
through the pinned service driver and the same app-owned preview relay used by
browser QA. The driver records bounded request results as QA evidence, redacts
response bodies, and does not claim browser interaction or screenshot coverage.
Build, Test, Start, API QA, and visual-QA applicability remain separate
receipts. The service driver currently covers the approved API request action;
it does not establish universal integration-test coverage for every service.
The rootless tests establish candidate headless and polyglot paths, not release
certification or the complete #166 worker and reviewer flow. #171 controls QA
release certification. Until those paths and released QA image digests are
validated, the application must not present a detected candidate as a verified
runnable target or claim runtime QA coverage.

When adding a custom target in Task Details, the operator can select dependencies
from that task's successfully prepared broker inventory. The picker exposes only
requests whose task activity and preparation telemetry both report success and
whose request-bound cache snapshot still passes integrity checks for the task,
project, package ecosystem, and lockfile identity. The selected request IDs are
included in the user-approved target proposal; the controller revalidates them
against the current inventory before saving. Free-form IDs, duplicate IDs,
stale preparations, and missing or corrupt snapshots are rejected. The app
revalidates these request-bound snapshots again before transferring them to
desktop QA.
