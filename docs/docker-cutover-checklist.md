# Docker cutover and release checklist

**Tracker reconciliation (2026-10-23):** #143 is closed for legacy sandbox
removal and Docker-only source/documentation cutover, **not** for a final
migration/release certification. Preserve the unchecked legacy history,
dirty-clone, authorization and interrupted-run migration requirement here
under #144 until real samples pass. #146 has been reopened for real
provider role/host-code-mode acceptance, while #156 owns clean startup.
Parallel CI changes from #310 need an exact-main reliability check for the
queue-concurrency E2E in draft #311 / tracking issue #312. No checkbox is
implicitly passed by these status updates.


This checklist is the final Docker migration gate. A checked box needs a
current test result, workflow run, or manual sign-off recorded beside it.
Keep unmet items unchecked; a mock or skipped integration test is not a
runtime sign-off.

Current hosted evidence and its limits are recorded in the [Docker execution
security matrix](docker-security-matrix.md) and the [dated release evidence
matrix](docker-release-evidence.md). Neither snapshot checks off
the release gates below.

## Application and provider paths

- [ ] Application starts when Docker is missing and explains the exact blocker.
- [ ] Repository-reading planning, implementation, verification, and QA fail
  closed when no approved Docker target is ready; context-only planning remains
  separate and cannot read a repository.
- [ ] Pi, Codex, Claude Code, OpenCode, Copilot, and Antigravity use the same
  app-owned MCP tools for equivalent execution modes and roles.
- [ ] No provider can invoke host-native repository tools or acquire extra
  authority through model, role, or task selection.
- [ ] Provider discovery isolates probes, handles hangs/crashes/cancellation,
  bounds output and child cleanup, and preserves healthy results after another
  provider fails.

## Docker and image trust

- [ ] Native Linux release validation uses a real rootless Docker Engine and
  asserts daemon rootless status; rootful Linux is rejected.
- [ ] Supported Docker API/engine versions, architecture, cgroups/limits,
  available disk, image platform, and image digest are inspected before use.
- [ ] Images are built from locked sources, pinned by architecture-specific
  digest, inspected before launch, and accompanied by provenance/SBOM and
  documented update/rebuild policy.
- [ ] Runtime rejects task-provided Dockerfiles, mutable tag-only images,
  unverified registries, arbitrary Docker endpoints, privileged settings,
  host namespace/device access, and unsupported limits.
- [ ] CI provisions the approved rootless Engine and runs Docker-backed tests
  as a required quality gate without conditional skips.

## Negative security tests

- [ ] Read attempts against private home files (`.ssh`, `.aws`), sibling
  projects, other task clones, and Docker socket are denied.
- [ ] `.git` writes, symlink/parent traversal, bind-mount escape, host mount
  injection, and unsafe workspace staging are denied.
- [ ] Agent-selected image/Dockerfile, `--privileged`, capability/device
  escalation, unconfined seccomp/AppArmor, host network/PID/IPC, and unapproved
  registry access are denied.
- [ ] DNS, HTTP egress, loopback-to-host, and package install scripts that
  require unapproved network access fail.
- [ ] CPU, memory, process, temp disk, log, artifact, and wall-clock quotas are
  enforced by the Engine/runtime rather than shell `ulimit` alone.
- [ ] Cancellation cannot kill unrelated processes or containers; daemon
  restart/disconnect never becomes a successful result without an exit record.
- [ ] Forged broker capabilities, cache poisoning, stale artifact replay,
  unapproved private configuration, SSH host-key bypass, claim takeover, and
  raw remote Docker TCP are rejected.

## Positive lifecycle and integration tests

- [ ] Rust build/test and Node install/build/test complete through approved
  offline broker/cache flows.
- [ ] Planning is read-only; implementation and verification retain their
  intended task-clone writes while keeping Git metadata protected.
- [ ] Concurrent task clones remain isolated through start, cancellation,
  retry, resume, crash recovery, and cleanup.
- [ ] All six providers have fake structured-output tests across each mode and
  user role; real-provider Docker MCP checks are recorded separately where
  credentials and CI allow.
- [ ] Local browser QA can produce bounded, hash-checked screenshot evidence
  and cannot publish changes.
- [ ] A controlled SSH worker test verifies host-key trust, rootless Docker,
  stage-in/out, worker disconnect/reconnect, cancellation, stale result
  rejection, broker authorization, and task clone preservation.
- [ ] Two-controller/two-worker tests cover the same task, different tasks,
  lease takeover, worker loss, resource contention, and result-apply fencing.
- [ ] A protected or scheduled real two-Linux-host run verifies remote
  rootless Docker, concurrent execution, and disconnection recovery.

## User experience, migration, and support claims

- [ ] Settings identify local Docker readiness, daemon/image details,
  resource capacity, safe setup steps, and a cleanup-scoped smoke test.
- [ ] Remote worker registration pins SSH host identity; trust/revoke,
  enable/disable, limits, health, and active tasks are visible.
- [ ] Task Details displays selected target, transfer/run/cancel state,
  failures, and evidence without exposing private SSH configuration.
- [ ] Private files are never forwarded by default; any supported exception
  has exact worker/task/file scope, expiry, revoke, encryption, and cleanup.
- [ ] Existing task clones, dirty changes, branches, approvals, evidence,
  history, and authorization survive migration. Old runtime attempts become
  interrupted and resume only as a new Docker execution.
- [ ] Linux, macOS Docker Desktop, and Windows Docker Desktop claims match
  platform tests. Unsupported platforms are labeled experimental/unsupported.
- [ ] Threat model explains that Docker is not a perfect hostile-code boundary
  and that daemon/socket/mount authority is powerful.
- [ ] Backup, rollback, troubleshooting, worker operator, and image update
  instructions match actual behavior.

## Required final evidence

- [ ] `cargo +1.98.1 fmt --all --check`
- [ ] `cargo +1.98.1 nextest run --locked --all-targets --no-fail-fast`
- [ ] `cargo +1.98.1 test --locked --doc`
- [ ] `cargo +1.98.1 clippy --locked --all-targets -- -D warnings`
- [ ] Rootless Docker integration workflow and required quality aggregation
  passed on the exact reviewed commit.
- [ ] Independent security and code review completed; blocking findings fixed.
- [ ] Public release notes include verified platform support, migration
  behavior, and known limits.

## Latest CI evidence (2026-10-22)

At commit `08ee5bdd4cbff96b0b499e44b6237fcf93265ce1`, the hosted
[quality workflow](https://github.com/zbarno/kool.ade/actions/runs/39793292401)
passed after rerunning the initially failing test attempt. Its
[tests and Docker runtime job](https://github.com/zbarno/kool.ade/actions/runs/39793292401/job/119886403621)
provisioned Docker Engine 29.9.0 in rootless mode, asserted the Linux engine
version and rootless security option, required the SSH E2E setup, ran the full
nextest suite with Docker and remote-backend integration enabled, and passed
the daemon-restart recovery E2E. The hosted format, documentation, clippy,
Windows process-tree, Docker-only, and required quality aggregation jobs also
passed on that commit.

This records CI evidence for that revision only. It does not complete the
two-Linux-host release run, real-provider checks, platform QA, or the other
unchecked release gates above. The checklist remains blocked pending those
applicable results and a fresh passing run on the final reviewed commit.

**Release decision:** blocked until all applicable items above have linked,
current evidence. Do not infer sign-off from code presence, a passing fake
engine test, or an integration job that skipped.
