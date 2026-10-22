# Docker Engine prerequisites

Repository commands use the application-owned Docker `ExecutionSession`. This
document describes the local engine and image checks made before a session is
created. A failed check denies repository tools; the application does not run
commands on the host as a fallback. The current application execution route is
Linux-only. Docker Desktop support on macOS and Windows is not production
verified.

## Supported local engine

- Native Linux must use rootless Docker Engine. Being in the `docker` group or
  running a task process as a non-root UID does not make a rootful daemon
  rootless.
- Docker Desktop on macOS or Windows is not enabled as a supported application
  execution target until path staging and cleanup are validated on those hosts.
- TCP, HTTP, SSH Docker endpoints, and unverified Docker contexts are rejected.
  Remote task execution will use the authenticated Kool.ad/e SSH worker; it will
  not forward a Docker API endpoint.
- Discovery checks daemon/API versions, Linux mode, architecture, rootless
  security options, seccomp, CPU/memory/PID limits, Docker image storage, and an
  application-owned workspace staging path. It performs no pulls, container
  creation, installation, daemon changes, or task execution.
- Readiness is cached for at most 15 seconds. Call `EngineDiscovery::invalidate`
  after a context switch or when a daemon restart is observed.

Failure is closed and categorized. Missing sockets, stopped daemons, permission
denials, rootful Linux, Windows containers, unsupported API/architecture,
missing limits/seccomp, insufficient disk, unknown profiles, and image digest or
platform mismatches do not fall back to host commands.

## Approved image profiles

`containers/images.lock.json` locks architecture-specific SHA-256 manifests
for `general`, `rust`, `node`, `python`, and `go`. Production selection accepts a profile and
the daemon architecture; it never accepts a repository or image name from task
text. Browser and desktop QA profiles remain disabled pending their separate
runtime lifecycle approval. Pulls are separate from health checks and can only
be requested through the bundled lock.
The runtime user is numeric UID/GID `10001:10001`.

Local implementation stages bounded task content into the container and
synchronizes validated changes back through the application-owned workspace
resolver. Git metadata remains separately protected. Docker-backed integration
tests assert the container's non-root identity, network isolation, resource
limits, host-secret denial, and task-clone synchronization. The CI gate runs
these tests against a pinned rootless Docker Engine.

## Saved task workspace compatibility

Task metadata remains separate from execution records. Legacy linked-worktree
tasks are migrated through a journaled workspace snapshot that preserves the
original worktree, branch, base commit, staged and unstaged changes, untracked
files, and symlinks. Interrupted migrations can resume only after validating
the saved snapshot and destination; conflicts leave both source and destination
available for review. An execution process is never resumed from serialized
command arguments. After a controller restart, unfinished Docker records are
marked interrupted by the execution recovery path, and a later task resume
creates a new execution attempt. See `src/core/implementation/execution/task_state/legacy_workspace.rs`,
`src/core/implementation/tests/task_repositories/legacy_migration.rs`, and
`src/execution/docker/backend/lifecycle/recovery/interrupt.rs`.

The operator's existing ignored `koolade/runtime-config.json` project grant
keeps its validated format. Local Docker execution may mount its approved
files read-only; remote execution requires a separate worker-bound grant. The
grant and file contents remain outside task commits and execution journals.
This avoids treating an old local grant as permission to transmit files to a
worker.

`containers/executor/Dockerfile` is a trusted recipe with a fixed build context.
It is never built from an agent repository. The `Trusted executor images` workflow
checks ten locked executor profile/platform combinations plus the two amd64 QA
image candidates on pull requests; after a change
reaches `main`, it publishes digest-addressed GHCR candidates with BuildKit SBOM and
SLSA provenance attestations and retains digest records as workflow artifacts. Review
the artifact and attestations before promoting any derived digest into
`containers/images.lock.json`. Runtime still uses the currently reviewed source
manifest pins until that promotion is committed; build tags are never trusted by the
runtime. See `containers/executor/README.md` for rebuild and promotion steps.

Docker Desktop's VM disk capacity cannot be inferred from the host's free space.
Until the engine reports a verifiable storage capacity, discovery fails closed
instead of treating host staging space as a VM capacity measurement.
