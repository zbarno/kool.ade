# Docker execution security matrix

This matrix describes the security contract and current support status for the
Docker migration. It is a release checklist as well as a design reference;
rows marked **not verified** must not be described as production support.
The dated hosted evidence and remaining release gaps are tracked in the
[Docker release evidence matrix](docker-release-evidence.md).

## Provider and role parity

Pi, Codex, Claude Code, OpenCode, Copilot, and Antigravity are provider
frontends. Each provider runs on the controller for its account and model
connection. Repository tools are exposed through Kool.ad/e's application MCP
bridge and a mode-bound `ExecutionSession`; selecting a provider cannot widen
the mode policy. The bridge must fail closed when there is no session. Provider
native repository tools and local shell features stay disabled. This policy
applies to Man.ager, specification/planning, task generation, investigation,
implementation, verification/QA, documentation, and reconciliation roles.
Per-role readiness is still reported independently from whether a Docker
target is available.

| Provider | Controller-side adapter | Repository access | Credential boundary | Parity evidence required |
| --- | --- | --- | --- | --- |
| Pi | `providers/pi.rs` MCP extension | Application tools only | Provider auth remains on controller | Fake-provider tests for each mode and role |
| Codex | `providers/codex.rs` | Application MCP only; host code mode disabled | Provider auth remains on controller | Fake-provider tests for each mode and role |
| Claude Code | `providers/claude.rs` | Application MCP only; unsafe managed host hooks/settings rejected | Provider auth remains on controller | Fake-provider tests for each mode and role |
| OpenCode | `providers/opencode.rs` | Application MCP only | Provider auth remains on controller | Fake-provider tests for each mode and role |
| Copilot | `providers/copilot.rs` | Application MCP only; unsafe machine hooks rejected | Provider auth remains on controller | Fake-provider tests for each mode and role |
| Antigravity | `providers/antigravity.rs` | Application MCP only | Provider auth remains on controller | Fake-provider tests for each mode and role |

## Execution modes

| Mode | Workspace and Git | Network | Process and container boundary | Credentials and broker | Result authority |
| --- | --- | --- | --- | --- | --- |
| `PlanningReadOnly` | Selected project/task content read-only; `.git` read-only | Disabled | Non-root, no added capabilities, no privileged mode, readonly root filesystem, bounded CPU/memory/PIDs/temp/output | No provider credentials in container; repository tools are app-owned | No Git writes; bounded evidence only |
| `Implementation` | Assigned task clone writable; `.git` metadata separately mounted read-only | Disabled | Same common container policy and per-request deadline/cancellation | Dependency/resource requests are mediated by the controller; no host secrets | Controller verifies and applies returned changes to the task clone; worker cannot commit, push, or merge |
| `Verification` | Assigned task clone writable for build/test outputs; `.git` read-only | Disabled | Same common container policy and bounded output/artifacts | No provider credentials; dependency access only through explicit broker capabilities | Controller records bounded logs/results; verification cannot publish |
| `RuntimeQa` | Workspace read-only; evidence/artifact path writable | Disabled unless the application explicitly grants named service ports | Same common container policy; service reachability is limited to authorized QA ports | No provider credentials or private configuration by default | Controller validates bounded evidence and records the result; QA cannot publish |

Unknown modes, policy fields, command capabilities, environment names, image
digests, or target capabilities are denied. Request budgets are bounded by
the worker's immutable policy ceiling. Container cleanup must be scoped to
the application-owned execution/task labels.

## Target support matrix

The mode rules above are identical for every target. A remote worker does not
receive broader mounts, network, credentials, or Git rights than a local
worker.

| Target | Engine and identity | Workspace transfer | Network and credentials | Status |
| --- | --- | --- | --- | --- |
| Native Linux local | Trusted local Engine API; require rootless Docker, compatible Linux mode, supported limits, and approved image digest | Resolve only the app-owned task clone; stage through bounded workspace handling | Container network disabled by default; controller credentials remain outside container | CI must prove this against a real rootless daemon; local rootful Docker is rejected |
| SSH remote Linux | Explicitly enrolled SSH alias and pinned host-key fingerprint; worker must report compatible version, policy, image digest, and rootless Docker | Bounded, hashed, source-bound stage-in/out; reject `.git`, private paths, unsafe symlinks, oversized archives, and stale results | SSH carries typed protocol messages; dependency metadata is relayed to the controller broker, with approved cache data transferred separately; no unrestricted Docker TCP, provider token, or implicit private-file forwarding | Durable broker relay and claim recovery ([#190](https://github.com/zbarno/kool.ade/pull/190)), global claim authority ([#283](https://github.com/zbarno/kool.ade/pull/283)), confirmed-loss retry/fencing ([#278](https://github.com/zbarno/kool.ade/pull/278)), transfer and execution telemetry ([#286](https://github.com/zbarno/kool.ade/pull/286), [#294](https://github.com/zbarno/kool.ade/pull/294), [#300](https://github.com/zbarno/kool.ade/pull/300)), private-file denial and output suppression ([#292](https://github.com/zbarno/kool.ade/pull/292), [#296](https://github.com/zbarno/kool.ade/pull/296)) have focused regression coverage. Worker health retains GPU and isolated-display observations, but device provisioning is not implemented ([#290](https://github.com/zbarno/kool.ade/pull/290)). Per-worker telemetry includes transfer size/time, Start RPC duration, execution wall/CPU/memory, image digest, typed result failure, harness, and requested model. Remaining #141 gates include two-controller/two-worker dependency-broker authorization and recovery acceptance, queue delay, separate dependency/image preparation, and GPU/display provisioning. |
| Docker Desktop on macOS | Local Desktop Linux VM transport and host-path/staging behavior must be verified | Must use archive/staging, never host clone paths interpreted inside the VM | Same mode policy; VM trust limitations must be documented | Not production-verified |
| Docker Desktop on Windows | Linux-container backend and WSL2/host path behavior must be verified; Windows containers are unsupported | Must preserve Unicode/case-collision, executable, symlink, and line-ending behavior through archive/staging | Same mode policy; no shell interpolation for SSH or paths | Not production-verified |

Remote telemetry identifies the enrolled worker by its stable worker ID and
keeps SSH aliases, credentials, and private host configuration out of shared
project records. A recorded Start RPC duration is the end-to-end controller
request time; it must not be interpreted as an isolated image-preparation or
queue-delay measurement.

When a user-approved private file is mounted for a remote execution, the worker
suppresses all command output and discards workspace changes and artifacts for
that run. This prevents copied or transformed secret contents from entering
controller or worker journals, logs, or durable task results. It also means
private-file runs cannot return build outputs; rerun without private files when
durable outputs are needed.

## Authority and resource controls

| Boundary | Required control | Release evidence |
| --- | --- | --- |
| Filesystem | Only the selected task workspace is mounted/staged; protect `.git`; reject traversal, symlink escape, host-home/socket mounts, and agent-selected Docker settings | Docker-backed attempts to read sibling/private paths and mutate Git metadata are denied |
| Process | Non-root UID, drop all capabilities, `no-new-privileges`, no devices, no host namespaces, readonly root, bounded tmpfs, CPU/memory/PID/deadline/output limits | Real Engine inspection plus adversarial process/resource tests |
| Network | No ambient container egress; only explicit Runtime QA service ports may be reachable | DNS, HTTP, loopback-to-host, and unapproved registry access attempts fail |
| Secrets | Provider/model credentials and host secret files remain on the controller; remote private-file transfer requires a separate destination-scoped, expiring approval | Tests prove denial by default and redaction from journals/logs/evidence |
| Dependencies | Controller applies existing dependency authorization policy; worker receives only approved bounded artifacts or image data | Acceptance/denial, hash mismatch, expiry, and stale-claim tests |
| Git | Worker has no merge/push credentials; result application and publication remain controller-owned and claim-fenced | Competing-controller takeover cannot apply or publish a stale result |
| Lifecycle | Match owner, task, execution, lease/auth generation, and cancellation generation before start, result collection, and apply | Disconnect, restart, cancellation, and stale replay tests |

## Current validation status

Unit and fake-Engine tests are necessary but do not prove runtime isolation.
The mandatory CI job must start a real rootless Linux Docker Engine, assert
that the daemon reports rootless mode, and run the Docker integration tests
without an opt-out skip. Local runs against rootful Docker are not evidence for
the rootless policy. The exact-head hosted quality and trusted-image checks on
PR [#296](https://github.com/zbarno/kool.ade/pull/296) passed on 2026-10-22
([quality run](https://github.com/zbarno/kool.ade/actions/runs/39779969714),
[image run](https://github.com/zbarno/kool.ade/actions/runs/39779969708)) and has
since merged. Those checks provide evidence for private-file output suppression
and the reviewed images.

### Hosted evidence snapshot: PR #302

The following additional evidence is from PR [#302](https://github.com/zbarno/kool.ade/pull/302)
at head `74946359390980ea30e5a392e4478be23b8d4a37`, against `main` at
`2e60c0f758dc6a8b73252f2e104f7903897683a5`. The hosted
[quality workflow](https://github.com/zbarno/kool.ade/actions/runs/39841917236)
completed on 2026-10-22. Its `tests and Docker runtime`, `Docker-only runtime`,
format, documentation, and Clippy jobs passed on that head.
The separate [trusted-image run](https://github.com/zbarno/kool.ade/actions/runs/39841917107)
also passed for the same head, including both Linux rootless QA candidate smoke
tests described below.

| Area | Evidence from the hosted run | Status and limit |
| --- | --- | --- |
| Linux rootless Docker | The [`tests and Docker runtime` job](../.github/workflows/ci.yml#L28) starts Docker Engine v29.9.0 in rootless mode, asserts Linux and the `name=rootless` security option, then runs Nextest with `KOOLADE_DOCKER_BACKEND_INTEGRATION=1`. | **Verified in hosted Linux CI.** This does not establish Docker Desktop support. |
| Authenticated SSH worker E2E | The [same job](../.github/workflows/ci.yml#L50) requires `sshd`, sets `KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION=1` and `KOOLADE_REQUIRE_SSHD_E2E=1`, and runs the suite. Coverage includes [`remote_docker_worker_restores_authorized_npm_cache_without_container_network`](../src/harness/resource_bridge/tests/end_to_end/remote/cache_restore.rs), [`ssh_worker_applies_verified_remote_file_change_to_the_task_clone`](../src/harness/resource_bridge/tests/end_to_end/remote/task_roundtrip.rs), [`ssh_worker_restart_reconciles_started_exec_and_rejects_partial_result`](../src/harness/resource_bridge/tests/end_to_end/remote/worker_restart.rs), and [`git_claim_takeover_fences_the_stale_remote_worker_result`](../src/harness/resource_bridge/tests/end_to_end/remote/claim_takeover.rs). | **Verified against loopback SSH and the same rootless daemon.** This is not a separate-host run. |
| Provider roles and app-owned MCP | [`fake_provider_invocations_cover_all_modes_and_launch_profiles`](../src/harness/execution_security/application_boundary/providers/tests/invocation_matrix.rs) and [`every_provider_role_calls_only_its_application_owned_mcp_authority`](../src/harness/execution_security/application_boundary/providers/tests/provider_mcp_e2e.rs) use fake provider executables. [`every_mode_has_equal_repository_tool_authority_across_all_providers`](../src/harness/execution_security/application_boundary/providers/tests/role_authority.rs) and [`every_user_role_keeps_its_intended_mode_and_provider_authority`](../src/harness/execution_security/application_boundary/providers/tests/role_authority.rs) compare generated configuration. | **Mock-verified.** No vendor CLI prompt or provider API was used; vendor-specific runtime interpretation remains **Unavailable / unverified**. |
| Network and private-file denial | [`authorized_npm_restore_blocks_malicious_install_egress_and_host_secrets`](../src/harness/resource_bridge/tests/end_to_end/npm_malicious.rs) runs in the rootless Docker integration suite. | **Verified for this fixture and path.** It is not a complete adversarial image, network, and filesystem matrix. |
| Docker-only static assertion | The `Docker-only runtime` job runs [`scripts/assert-docker-only.sh`](../scripts/assert-docker-only.sh). | **Static assertion passed.** It does not prove legacy persisted task-state migration. |
| Concurrent controller sessions and worker recovery | [`concurrent_ssh_controllers_share_worker_and_reject_revoked_result`](../src/harness/resource_bridge/tests/end_to_end/remote/two_controllers.rs), [`ssh_worker_restart_reconciles_started_exec_and_rejects_partial_result`](../src/harness/resource_bridge/tests/end_to_end/remote/worker_restart.rs), and [`git_claim_takeover_fences_the_stale_remote_worker_result`](../src/harness/resource_bridge/tests/end_to_end/remote/claim_takeover.rs) ran in the loopback SSH suite. | **Verified for those fixtures.** Separate controller processes, two separate workers, resource contention, and two-host loss recovery remain **unverified**. |
| Cancellation and bounded container cleanup | [`docker_engine_create_exec_cancel_and_cleanup_are_owned_and_bounded`](../src/execution/docker/backend/tests/docker_engine.rs) ran in the rootless integration suite. | **Verified for the covered lifecycle.** |
| Linux QA candidate image lifecycle | The [trusted-image workflow](../.github/workflows/executor-images.yml#L307) ran [`rootless_app_owned_qa_driver_uploads_runs_reads_and_cleans_candidate`](../src/execution/docker/qa_container/tests/desktop_rootless.rs) for `desktop_qa` and [`rootless_app_owned_browser_preview_and_driver_produce_evidence_and_clean_up`](../src/execution/docker/qa_container/tests/browser_rootless/run.rs) for `browser_qa` on rootless Linux. | **Passed for these Linux image-level smoke tests.** They do not establish user-facing Runtime QA availability or production UI behavior; Runtime QA remains unavailable in product support claims. |
| Legacy runtime migration and platform support | The static assertion and this hosted full-suite pass do not prove preservation of legacy execution records, dirty clones, approvals, and evidence during Docker migration. macOS and Windows Docker Desktop E2E are absent. | **Unverified.** Keep these capabilities out of production support claims until dedicated evidence is recorded. |

The workflow and test names above are reproducible from the linked run and
source paths in this repository. A green test suite is not release approval:
the Docker cutover checklist remains blocked until each applicable unchecked
item has its own current evidence, including the separate-host, provider,
migration, and platform gaps listed here. Rootful local Docker does not count
as evidence for the rootless policy.

The executor is a practical isolation boundary, not a claim of perfect
containment for hostile code. Docker daemon access and host mounts are powerful
capabilities; the application therefore keeps daemon access in the trusted
controller/worker process and does not mount the Docker socket into task
containers.
