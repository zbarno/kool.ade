# Provider runtime containment v1

Status: security design in progress for issue [#591](https://github.com/zbarno/kool.ade/issues/591).
This document records the strategy evaluation and required trust boundaries;
it does not claim that Codex or Claude execution is contained or supported.

## Current boundary and decision

Today, provider CLIs run on the controller so they can use their supported
sign-in and model transports. Their repository tools are disabled and
application MCP operations go through the existing `ExecutionSession`. That
session protects brokered repository commands, but does not contain provider
hooks, helpers, plugins, or descendants. Private homes, CLI flags, and MCP
configuration do not close that gap.

The implementation direction is to run the provider process tree in a
task-scoped rootless Docker runtime, reusing the existing rootless Docker
substrate, execution policy, task clone, MCP authorization, and cancellation
lifecycle. The provider runtime image and CLI delivery must be separately
selected, pinned, and verified; this design does not assume the current
repository execution image contains either provider CLI. This is a
necessary boundary, not sufficient proof of safety. Provider model transport
and authentication must also be mediated so arbitrary code in the container
cannot read reusable credentials or send them to an arbitrary destination.
Until that mediation and the process boundary are implemented and tested,
Codex and Claude remain unavailable for both profiles.

The ordinary repository `ExecutionSession` cannot simply host the CLI as-is:
it intentionally has no provider credentials and ambient network access is
disabled. A provider runtime therefore needs a separate, task-owned lifecycle
with the same trusted Docker substrate and cancellation semantics, plus narrow
bridges to the controller. It must not add a host-process fallback or expose
the Docker API to the container.

## Strategy evaluation

| Strategy | Assessment |
| --- | --- |
| Isolated `HOME`, rewritten config, or CLI disable flags | Rejected: cloud/account policy and descendants still execute as host processes. |
| App-owned MCP while CLI stays on controller | Rejected: contains repository tools only, not hooks or arbitrary subprocesses. |
| ACP or another CLI protocol | Rejected as a containment boundary: protocol choice does not confine the CLI process tree. |
| New parallel sandbox or mandatory sidecar | Rejected: duplicates execution authority and adds a new privileged boundary. |
| Task-scoped rootless Docker provider runtime | Selected for implementation evaluation: reuses the trusted execution substrate and can contain the CLI and descendants, provided mounts, process controls, network, and credentials are independently verified. |

The selected direction is conditional. A provider/authentication combination
must remain unavailable if its model transport cannot be mediated without
exposing reusable credentials to hooks or allowing arbitrary egress.

## Trust boundaries and threat matrix

| Threat path | Required boundary and evidence |
| --- | --- |
| Managed hook, startup script, plugin, skill, subagent, MCP helper, or shell child | All provider descendants stay in one unprivileged task container; no host PID, privileged mode, added capabilities, Docker socket, or host executable mounts. Require `no-new-privileges`, a reviewed seccomp profile, and the host's supported AppArmor/SELinux confinement. Synthetic managed-hook tests must prove a child command runs inside the container and cannot affect a host marker outside an explicit allowed view. |
| Malicious repository content or cross-task access | Mount only the assigned task snapshot and task-private scratch. Do not mount sibling clones, controller home, other task state, or writable Git publication credentials. Validate symlinks and use the existing workspace identity/revision checks. |
| Controller credentials and provider auth | Never mount controller credential stores or pass reusable tokens in command arguments, environment, task files, logs, artifacts, or MCP data. Use a controller-owned auth/transport broker only if it can authenticate supported provider requests without returning reusable secrets to the CLI. Otherwise fail closed for that auth mode. |
| Network exfiltration or unauthorized destination | No direct container egress. Any model connection must traverse an authenticated, destination- and protocol-constrained controller broker. Repository dependency/resource access remains separately authorized by existing brokers. Verify denied destinations and prevent the provider from reaching controller-local services. |
| App MCP misuse or stale task identity | Expose only the task-scoped bridge with short-lived capability, role/mode, task, revision, and cancellation binding. Revalidate authorization on every request and revoke on cancellation or session release. Provider output cannot mint capabilities or perform publication/approval actions. |
| Cancellation, timeout, crash, or lost controller | Terminate the complete container/process group, revoke bridge and transport grants, and clean task resources by application-owned identity. Verify no descendant or active grant survives terminal cleanup. |
| Manager prompt or tool authority escalation | Assess Manager independently. Keep it blocked unless effective instructions/configuration and proposal-only authority are verified; Worker containment evidence alone never creates Manager eligibility. |
| Engine or platform mismatch or container escape | Require current evidence for Linux rootless Docker, image digest, engine capabilities, and launch policy. Reject rootful/unknown/unsupported engines and retain existing platform fail-closed behavior. Adversarial validation must attempt namespace and container escapes, including proc/sys mounts, device access, kernel interfaces, and unsafe runtime options; reject the target if required controls are unavailable. |

## Runtime contract to implement

The provider runtime must be created and released by application code for one
task execution. Its request is bound to provider/version, auth strategy,
project/task identity, pinned workspace revision, role/profile, execution
mode, image digest, policy version, and cancellation generation. It uses an
unprivileged identity, read-only root filesystem, explicit writable task and
scratch mounts, bounded CPU/memory/process/temp/output resources,
`no-new-privileges`, reviewed seccomp and host LSM profiles, no host
network/PID/IPC namespaces, no added capabilities, and no Docker socket. The
container receives no ambient inherited environment; values are explicit and
reviewed against the selected auth strategy.

The controller remains the owner of user consent, task claims, authorization,
QA, Git publication, approvals, merge, and deployment. The runtime can return
bounded output and proposed workspace changes only. The controller validates
the task/revision fence before applying those changes. Container output and
model-visible data are untrusted.

Readiness evidence must distinguish binary detected, authenticated, model
usable, transport ready, Worker-contained, Manager-eligible, and unavailable
with an actionable reason. Evidence is versioned and invalidated when the
binary path/version, auth source, runtime image/engine, policy, role, or launch
configuration changes. A configured or locally observed hook list is not
evidence that cloud-managed policy is absent.

## Validation required before removing blockers

The following proofs are required for each supported provider/version/auth
combination, not inferred from Docker configuration or fake CLI invocation:

- Rootless-container adversarial tests for host filesystem, process, socket,
  sibling-task, secret, network, forbidden MCP access, and attempted namespace
  or container escapes (including proc/sys, devices, kernel interfaces, and
  runtime-option abuse).
- A synthetic organization-managed hook that launches a child process; prove
  the allowed in-container marker works and an out-of-boundary host marker
  cannot be created or read.
- Credential mediation tests for saved-profile/OAuth, refresh, and supported
  environment/API-key paths. Any path that requires the provider process to
  hold a reusable secret remains unavailable until an alternate safe transport
  is proven.
- Policy/configuration drift and expired-evidence tests that deny execution.
- Cancellation, timeout, crash, and controller-loss tests proving descendants,
  bridge grants, and runtime resources are revoked.
- Separate Manager tests proving managed instructions cannot authorize tools,
  approvals, ownership changes, QA bypass, or publication.
- Authenticated native Codex and Claude smoke plus managed-hook containment in
  integrated release validation #144/#156. Fixture tests do not count as real
  provider evidence.

Existing Rust quality gates and hosted Docker/security workflows remain
required. No provider blocker should be removed until the replacement runtime,
transport mediation, role-specific evidence, adversarial tests, and applicable
hosted checks all pass.
