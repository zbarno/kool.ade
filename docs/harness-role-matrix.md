# Harness role matrix

Kool.ad/e runs external CLIs under an application-selected **execution
profile**. A CLI's presence, authentication, model catalog, or ACP transport is
not proof that it is eligible for a role. Discovery records provider-specific
profile IDs bound to the exact CLI version and authentication-source class
(`Environment` or `ProviderProfile`). Evidence expires with the capability
snapshot and is checked again against the installed CLI version and current auth
class before a routed call. Stale or unknown evidence is refreshed and cannot
route work. Normal routing never substitutes another provider silently.

## Authority and profiles

The Manager is the control plane. Kool.ad/e owns its instructions, context,
read-only tools, workflow state, and action authorization. Manager output is
advisory: deterministic application checks and required human approval gates
authorize consequential transitions, dispatch, review, repair, or publication.
Manager tools cannot write source, approve its own work, bypass QA/review, merge,
or deploy.

Workers are the execution plane. Their provider-native behavior is allowed only
when specifically configured and confined to the assigned task, clone, Docker
execution boundary, credentials/network policy, and review lifecycle. All
repository and command work uses the app-owned MCP tools and existing execution
session; provider identity cannot add tool authority.

| Provider CLI | Manager profile | Worker profile | Eligibility condition or blocker |
| --- | --- | --- | --- |
| Pi | Conditional | Conditional | Both require current profile/authentication, version, model, and mode evidence. The worker may receive one app-owned task skill during bounded implementation (see below). |
| Codex CLI | Blocked | Blocked | Account-managed hooks/policy cannot be inspected or contained. |
| Claude Code | Blocked | Blocked | Server-managed hooks/policy cannot be inspected or contained; Claude is additionally blocked under WSL where Windows-managed policy is not inspectable. |
| OpenCode | Blocked | Conditional | Manager remains blocked until installed-CLI behavior proves project instructions and plugins are suppressed. Worker requires current evidence and route/mode prerequisites. |
| GitHub Copilot CLI | Blocked | Conditional | Manager remains blocked until effective managed instruction and extension behavior is verified. Worker requires current evidence and route/mode prerequisites. |
| Antigravity CLI | Blocked | Conditional | Manager is blocked because ambient instruction/managed-policy suppression is not verified. Worker use requires current evidence and sandbox/tool prerequisites. |

“Conditional” means a current capability snapshot must confirm the exact CLI
version, authentication, model catalog, invocation support, profile identity,
and role/mode. Tool-enabled modes also require the app MCP bridge and the
appropriate live execution target. It does not claim every installed provider
is ready. Pi is currently the only Manager-eligible CLI. Codex and Claude fail
closed before launch for both profiles; Antigravity, OpenCode, and Copilot fail
closed for Manager. No authentication or transport result overrides those blockers.

Only one provider-native worker capability is currently enabled: Pi receives a
single application-owned `koolade-task` `SKILL.md` for bounded implementation.
Pi still disables ambient skills, extensions, context files, sessions, and
built-in tools; all tools come from the app MCP allowlist. No provider-native
subagents are enabled or claimed. Other provider-native skills, subagents,
hooks, plugins, and instructions remain disabled or unsupported unless a
separate verified capability is added. A worker skill or output cannot grant
permissions, approve work, or alter task scope.

## Roles and authority

`ExecutionMode` fixes the app tool boundary for an operation. The selected
provider and role profile cannot change it.

| User-facing role or phase | Execution mode | App-supplied tool access | Expected result |
| --- | --- | --- | --- |
| Man.ager planning and dispatch | `Manager` | Read-only | Structured proposals; dispatch and state transitions remain application actions |
| Man.ager explanation of a decision | `DecisionExplanation` | None | Structured explanation from supplied context; no repository tools |
| Specification and feature planning | `Planning` | Read-only | Specification proposal through the application response schema |
| Context-only analysis | `ReadOnlyAnalysis` | None | Analysis from supplied context; Docker readiness is not required |
| Task generation | `TaskGeneration` | Read-only | Task drafts parsed through the application schema |
| Investigation | `Investigation` | Read-only | Findings tied to app-mediated repository reads |
| Coding and documentation changes | `Implementation` | Bounded implementation | Proposed file changes in the assigned application-owned task clone |
| Independent QA inspection | `ReadOnlyAnalysis` | None | Findings from provided evidence, without repository access |
| Independent QA command phase | `Implementation` with verification subphase | Bounded implementation | Bounded containerized checks and recorded exit evidence |
| Reconciliation | `Reconciliation` | None | Structured conflict decision; publication stays app-owned |

## Configurable role routes

Coding tools Settings exposes provider/model routes for Manager, general
planning, specification, feature planning, task generation, investigation,
independent plan review, implementation, code QA, documentation,
reconciliation, decision explanation, repair work, and Runtime QA review.
Project Settings can override these routes for one project. Route precedence is
task override, project route, global role route, selected default, then the
legacy Codex setting. Manager has its own `Manager` execution mode and profile;
task overrides cannot select or replace Manager authority. Other routes retain
their corresponding worker mode/profile. Runtime QA review routing does not
satisfy the separate approved driver/runtime policy gate.

The QA command phase uses the existing `Implementation` mode with its
verification subphase and Docker `Verification` policy. It must not gain write
authority from the provider or QA role name. Runtime and visual QA require
their approved drivers and `RuntimeQa` policy before they can be offered.

## Worker mode adapter eligibility

This table describes generic **worker-mode adapter** support, not a claim that a
provider is currently authenticated or that a Docker target is ready. `P` means
the app has an adapter/profile contract for that mode; runtime discovery and
route checks still have to pass. `—` means the provider is blocked by
uncontainable managed hooks for that profile.

| Provider CLI | Planning | Read-only analysis | Task generation | Investigation | Implementation | Reconciliation | Decision explanation | Runtime QA visual review |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Pi | P | P | P | P | P | P | P | P |
| Codex CLI | — | — | — | — | — | — | — | — |
| Claude Code | — | — | — | — | — | — | — | — |
| OpenCode | P | P | P | P | P | P | P | P |
| GitHub Copilot CLI | P | P | P | P | P | P | P | P |
| Antigravity CLI | P | P | P | P | P | P | P | P |

All modes preserve their app-defined tool authority and response contract.
Adapters normalize streamed text, cancellation, structured output, and errors
into `HarnessOutcome`; model and reasoning selection remain validated and are
recorded with the task attempt. Context-only worker modes do not require Docker.
Repository-tool modes require the app-owned Docker MCP bridge and the target
required by their mode. The separate Manager table above applies the stricter
Manager profile check in addition to mode and snapshot readiness.

## Evidence and deferred validation

Discovery creates a `HarnessCapabilitySnapshotV1` with readiness for every mode
and role. The snapshot includes the profile IDs and authentication-source class;
`Unknown`, stale profile IDs, or an authentication-source change fail closed
and trigger refresh. Role selection checks the profile blocker, current
snapshot, selected role/mode, authentication, model, and execution target as
applicable. A failed route gives a diagnostic and does not silently substitute
another provider.

Automated fake-CLI tests check profile arguments/environment, app MCP authority,
mode tool access, output normalization, cancellation, and blocked-provider
rejection. They verify the application's generated configuration and invocation
boundary, not how a vendor CLI behaves when authenticated. Authenticated
provider smoke validation remains deferred to integrated release validation;
no real-provider behavior is claimed here. Record provider/version/auth-source
class, role/profile, effective instructions and tools, cancellation, and
observed result for each supported Manager provider. Verify the enabled Pi
worker skill inside its assigned Docker task and confirm unauthorized files,
tools, MCP calls, and actions remain unavailable. Do not mark these scenarios
passed from mocks or compilation alone.
