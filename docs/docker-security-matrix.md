# Docker execution security matrix

This matrix describes the security contract and current support status for the
Docker migration. It is a release checklist as well as a design reference;
rows marked **not verified** must not be described as production support.

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
| SSH remote Linux | Explicitly enrolled SSH alias and pinned host-key fingerprint; worker must report compatible version, policy, image digest, and rootless Docker | Bounded, hashed, source-bound stage-in/out; reject `.git`, private paths, unsafe symlinks, oversized archives, and stale results | SSH carries typed protocol messages; dependency metadata is relayed to the controller broker, with approved cache data transferred separately; no unrestricted Docker TCP, provider token, or implicit private-file forwarding | Scheduling, broker relay, confirmed-loss retry/fencing, and private-file denial are implemented and have regression coverage ([#278](https://github.com/zbarno/kool.ade/pull/278), [#292](https://github.com/zbarno/kool.ade/pull/292)); two-controller authorization/recovery parity, complete per-worker telemetry attribution, and GPU/display resource provisioning remain #141 release gates |
| Docker Desktop on macOS | Local Desktop Linux VM transport and host-path/staging behavior must be verified | Must use archive/staging, never host clone paths interpreted inside the VM | Same mode policy; VM trust limitations must be documented | Not production-verified |
| Docker Desktop on Windows | Linux-container backend and WSL2/host path behavior must be verified; Windows containers are unsupported | Must preserve Unicode/case-collision, executable, symlink, and line-ending behavior through archive/staging | Same mode policy; no shell interpolation for SSH or paths | Not production-verified |

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
the rootless policy. Cross-platform support and two-host remote behavior remain
unverified until their stated smoke tests have run. Update this section with
the exact workflow run, platform, test names, and date before release.

The executor is a practical isolation boundary, not a claim of perfect
containment for hostile code. Docker daemon access and host mounts are powerful
capabilities; the application therefore keeps daemon access in the trusted
controller/worker process and does not mount the Docker socket into task
containers.
