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
`add_user_target_override` and `ProjectRuntimeStore::add_user_target_and_save`
let an authenticated controller add an explicitly selected target for an
unknown stack. The target can select an app surface and project-relative root;
its recipes must cite the controller's approval reference. A generic logical
tool ID and discrete argv are allowed, so no Rust/npm-specific adapter is
required to describe an unfamiliar project. Task Details does not yet expose
the custom-target form; it currently exposes discovery, selection, and recipe
approval for detected targets.

`ExecutionRecipeV1` stores a logical executor tool ID and discrete arguments.
It rejects shell-evaluation switches, absolute or parent-traversing paths,
protected environment variables, unbounded timeouts, and Git metadata output.
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
  "working_directory_relative": "apps/site",
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
then terminated and reaped. Start is not yet dispatched by the normal task
queue.
HTTP readiness uses an application-generated loopback request through a declared
Node or Python standard-library prerequisite in the selected executor image;
it never downloads or invokes curl. Targets without that approved prerequisite
fail closed for HTTP readiness. Process health alone does not mean functional
or visual QA passed. Capability assessment uses an
explicit inventory of toolchains, dependencies, and surface drivers from the
approved execution environment; it never consults host `PATH`.
The queued QA prerequisite gate applies this assessment to the selected task
target and reviewed source SHA using toolchain and driver IDs from the pinned
image lock. Until dependency-broker inventory is wired into this gate, declared
dependencies remain unavailable and block QA; they are never assumed present.
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
hashes before uploading only the selected files to web preview or desktop sessions. Broker-backed toolchain
provisioning and inventory, and headless service evidence remain unintegrated.
Until those paths land, the application must not present a detected candidate
as a verified runnable target or claim runtime QA coverage.
