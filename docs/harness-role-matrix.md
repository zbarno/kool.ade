# Harness role matrix

This document defines the provider-neutral routing contract for the six
supported CLIs. Repository tools use the application-owned Docker MCP session.
Provider and role parity, remote execution, and platform support remain release
checks; see the evidence requirements below.

## Roles and authority

A role selects the task Kool.ad/e asks a provider to perform. `ExecutionMode`
selects the authority for that particular operation. Provider identity never
changes the mode's tool access.

| User-facing role or phase | Execution mode | App-supplied tool access | Expected result |
| --- | --- | --- | --- |
| Man.ager planning and dispatch | `Planning` | Read-only | Structured plan; dispatch remains an application action |
| Man.ager explanation of a decision | `DecisionExplanation` | None | Structured explanation; no repository tools |
| Specification and feature planning | `Planning` | Read-only | Specification proposal through the application response schema |
| Context-only analysis | `ReadOnlyAnalysis` | None | Analysis from supplied context; Docker readiness is not required |
| Task generation | `TaskGeneration` | Read-only | Task drafts parsed through the application schema |
| Investigation | `Investigation` | Read-only | Findings tied to app-mediated repository reads |
| Coding and documentation changes | `Implementation` | Bounded implementation | Proposed file changes in the application-owned task clone |
| Independent QA inspection | `ReadOnlyAnalysis` | None | Findings from provided evidence, without repository access |
| Independent QA command phase | `Implementation` with a verification subphase | Bounded implementation | Bounded containerized checks and recorded exit evidence |
| Documentation refresh | `Implementation` | Bounded implementation | Proposed documentation changes in the task clone |
| Reconciliation | `Reconciliation` | None | Structured conflict/reconciliation decision; publication stays app-owned |

## Configurable role routes

Coding tools Settings exposes provider/model routes for Manager, general
planning, specification, feature planning, task generation, investigation,
independent plan review, implementation, code QA, documentation,
reconciliation, decision explanation, repair work, and Runtime QA review.
Project Settings can override these routes for one project. Route precedence is
task override, project route, global role route, selected default, then the
legacy Codex setting. Planning/analysis, review, reconciliation, and Runtime
QA review keep their role's existing `ExecutionMode` and tool authority. Task
overrides may choose any supported work category except Manager. Manager's
provider may be selected globally or per project, while its application-owned
authority cannot be replaced by a task override. Runtime QA review routing
does not satisfy the separate approved driver/runtime policy gate.

The QA command phase uses the existing mode contract until the public
`ExecutionMode` is deliberately extended. It must run with the Docker
`Verification` policy and must not gain write authority from the provider or
the QA role name. Runtime and visual QA require their approved drivers and
`RuntimeQa` policy before they can be offered.

## Provider and mode eligibility

`P` below means eligible only when the provider capability snapshot confirms
the binary/version, authentication, selected model, invocation/stream format,
and requested mode. For any mode with tools, the snapshot must also confirm the
same application-managed Docker MCP bridge and an available execution target.
`—` means task execution is disabled because provider-managed hooks cannot be
contained by the current process boundary. An unavailable capability is a
categorized blocker; routing never substitutes another provider or model
silently.

| Provider CLI | Planning | Read-only analysis | Task generation | Investigation | Implementation | Reconciliation | Decision explanation |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Pi | P | P | P | P | P | P | P |
| Codex CLI | — | — | — | — | — | — | — |
| Claude Code | — | — | — | — | — | — | — |
| OpenCode | P | P | P | P | P | P | P |
| GitHub Copilot CLI | P | P | P | P | P | P | P |
| Antigravity CLI | P | P | P | P | P | P | P |

Every cell has the same authority and response contract for its mode. Provider
adapters may use different CLI arguments and wire formats, but must normalize
streamed text, cancellation, structured output, and failures into Kool.ad/e's
`HarnessOutcome`. The selected model and reasoning level are validated by that
provider's capability snapshot and persisted with the task attempt.

## Capability gates

Provider transport readiness is separate from execution-target readiness.
Planning may run from app-supplied context without a repository execution
target; in that case it receives no repository tools. Planning that includes
repository context and all other repository-tool modes require the Docker
bridge and target required by their mode. A remote worker does not need the
provider CLI, model credentials, or SSH credentials.

Before enabling a matrix cell, the implementation must prove all of the
following:

1. Discovery produced a fresh `HarnessCapabilitySnapshotV1` for this binary,
   version, authentication state, model catalog, invocation format, and mode.
2. The role router resolved task, project, global-role, and default routes in
   documented precedence while preserving explicit model selection. Manager's
   global route is configurable; task-level overrides cannot change Manager
   authority.
3. Repository tools are the same app-managed Docker tools for all providers.
   Provider-native host shell, file-edit, and repository tools remain disabled.
4. Fake CLI integration tests cover all six providers across all seven modes,
   including structured output, cancellation, unsupported capabilities, and
   the exact selected provider/model. No silent fallback is allowed.

## Current implementation status

The execution contract and settings store now define and persist
`HarnessCapabilitySnapshotV1` with entries for every mode and role. Discovery
marks the provider's mode and role contract ready only after it confirms
authentication, a fresh model catalog, the exact executable's clean launch
profile, and a supported application adapter. The app MCP adapter contract and
live execution target are recorded separately; tool modes require both at
routing time, while context-only modes do not require Docker. Codex CLI and
Claude Code are currently blocked in every task mode because their managed
account policy can require host-side hooks that Kool.ad/e cannot contain; local
launch flags cannot prove that managed hooks are absent. Other providers remain
subject to fresh authentication, model, and role evidence. Project routes
are stored in the project's planning store and participate in the same central
precedence resolver across planning, task, review, reconciliation, and QA
entry points. The six-by-seven
fake CLI result matrix exercises each task-capable production adapter through
`execute_with_model` and checks normalized final output; Codex and Claude
negative cases verify rejection before provider execution. The separate MCP
authority E2E verifies that provider role calls use only the application-owned
MCP tools. Together these prove invocation and tool configuration, but not
that each real provider interprets every tool policy identically. Full role
workflow E2E coverage, normalized typed provider failures, model/reasoning
validation, and Docker-backed provider smoke tests remain open before treating
every `P` cell as release-ready.

The current probes do not establish fresh account-specific model access for
cloud providers: Pi's credential resolver is not invoked because its auth file
can run a user command, Codex and Claude model catalogs remain stale, and
Copilot and Antigravity authentication remains unknown. Only an explicitly
configured OpenCode loopback model endpoint can currently provide fresh model
evidence without a remote account check. The `P` cells describe conditional
role contracts; they do not mean these providers are ready on a fresh clone.
