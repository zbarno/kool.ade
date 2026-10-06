---
koolade-task: {"schemaVersion":1,"uid":"5d1fd5d4-0b9b-4474-9771-ecf52184705d","batchUid":"ddbb6769-ec92-42c8-9651-7a91475c7109","repositoryId":"root","dependencyUids":["00b3df5f-9bfa-40d8-a8d3-2b748fd2e4f4"]}
---

# F11-TASK-normalize-pi-harness-usage-into-incremental-records — Normalize Pi harness usage into incremental records
<!-- koolade-artifact-id:v1 {"uid":"5d1fd5d4-0b9b-4474-9771-ecf52184705d","displayId":"F11-TASK-normalize-pi-harness-usage-into-incremental-records","title":"Normalize Pi harness usage into incremental records","parentUid":"ddbb6769-ec92-42c8-9651-7a91475c7109"} -->

Feature: F11 - Durable operator-local implementation telemetry (zbarno/kool.ade issue #23)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Today pi's NDJSON stream folds only into final text, activity, stop reason and tool-execution counts (pi_events.rs EventFold); ActivityTelemetry tracks update counts, not tokens, model, or cost. There is no normalized per-call usage crossing the AiHarness boundary, so the caller cannot attribute tokens, model or duration to a specific invocation. This is the raw-data half of R4.

## Ticket goal — what changes when done

A harness-neutral normalized usage contract fills incrementally from Pi's stream and, at end of invocation, produces a fully-attributed InvocationRecord (attribution passed in by caller, harness/provider/model/tokens/duration/outcome filled by Pi) ready to hand to the store from task 1. Missing fields degrade to None without panicking.

## User story

As an operator driving implementation I want every harness call's normalized usage exposed at the boundary so downstream can persist it per task, keeping app code harness-neutral.

## Purpose

Capture normalized per-invocation usage at the harness boundary from Pi's event stream (identity, provider, model, tokens, cost, duration, outcome), emitted incrementally and degrading gracefully on missing fields, while keeping higher-level code harness-neutral.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified: src/harness.rs defines the AiHarness boundary with submodules pi_events, pi_harness, api.rs; api.rs exports AiHarness, ActivityTelemetry, LiveProgress, PlanningRequest. EventFold accumulates per-message_id state and already keys messages for incremental emission. Task 1 delivers InvocationRecord and the telemetry store. Discovery: exact token/usage fields in Pi NDJSON (docs/json.md referenced at top of pi_events.rs); how reasoning-in-output convention is signaled per provider.

Feature ID: F11
Repository: root

## Approved scope mapping

- Scope 1: Normalized usage capture at the harness boundary: harness identity/version, provider, API/backend, requested vs actual model, reasoning/thinking level, model-call count, input/output/cache-read/cache-write/reasoning/total tokens, reported or estimated cost, duration (monotonic where practical plus wall-clock timestamps), and outcome/stop reason; emitted incrementally where the harness streams usage (R4).
- Success criterion 3: Each completed model call is counted exactly once; streamed or cumulative usage updates never double-count, and token categories reconcile without double-counting reasoning tokens already included in output (AC10-AC13).

## Dependencies

- [F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage](F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage.md) must be complete.

## Affected files and components

- src/harness/api.rs - extend the AiHarness-facing types with a transport-neutral normalized-usage record type (fields per R4)
- src/harness/pi_events.rs - extract token/model/provider/cost/outcome from Pi NDJSON into the fold, emitted per model call
- src/harness/pi_harness.rs - wire fold usage into InvocationRecord at end of call, fill attribution handed in

## Implementation steps

1. Add a NeutralInvocationUsage struct in api.rs mirroring R4: harness id/version, provider, api/backend, requested vs actual model, reasoning level, per-call token counts (input/output/cache_read/cache_write/reasoning/total), duration_mono_ms + wall timestamps, outcome/stop reason; all Options
2. Parse Pi usage/token fields in pi_events.rs during per-message folds; accumulate exactly-once per model_id (dedupe logic owned by task 3; here just forward raw values)
3. Emit one UsageObservation per model call into a caller-provided callback so callers can incrementally build records
4. In pi_harness.rs, take caller-supplied attribution (project/session/repo/feature/batch/task/phase) and combine with the folded usage to produce an InvocationRecord matching task 1's schema, returned alongside HarnessOutcome; never block or alter the outcome if a field is missing

## Acceptance criteria

- A successful Pi turn produces an InvocationRecord with tokens, model, provider, duration and stop reason populated, attribution copied verbatim from the request
- A stream missing optional fields (no cost field from Pi, missing reasoning level) still yields a well-formed record with those fields None
- Concurrent invocations produce independent records; no shared mutable Pi state bleeds identity between calls

## Test plan

1. Unit-test pi_events fold on a synthesized NDJSON with two model calls: assert two UsageObservations, correct token sums, and correct ordering
2. Feeder test: run Pi harness against a scripted stdout fixture lacking cost field; assert produced record has cost=None and other fields set
3. Feed cumulative/streamed token deltas across several lines for one message_id; observe that pi layer emits observations (task 3 will finalize dedup assertions)

## Verification commands and expected evidence

1. cargo +1.98.1 test --locked --all-targets -- --test-threads=1 h:: harness pi_events to exercise the fold unit tests

## Constraints

- Telemetry lives exclusively in operator-local persistence under $KOOLADE_HOME/projects/<project>/telemetry/; creating or updating it must neither Git-commit nor dirty the project repository (R2, AC3-AC4).
- .koolade-packet/state/time-ledger.log keeps its v1 format and Git-ignored treatment; no backfill of pre-telemetry work (R2).
- Cost estimation is deterministic for identical inputs (tokens, model, table version) and historical runs are never recomputed from later pricing; stored estimates and table versions drive all later display (R6, AC20).
- Higher-level application code must stay harness-neutral: the normalized usage contract is filled by Pi first and must admit future harnesses without changing task-report code (R4, Scope).
- Workspace-unioned timing may remain only as a derived view; it is never the source of truth for task-level analytics (R3).
- Landing gates: fmt, test, and clippy on Rust 1.98.1 all pass before any resulting commit, with the AC27 test matrix in place (R9).

## Out of scope

- Authoritative billing or invoicing integration; live pricing feeds.
- Populating any harness other than Pi (the transport-neutral model admits Codex, Claude Code, Copilot CLI, Opencode, and others later).
- Backfilling work performed before telemetry exists.
- Modifying shared planning-state artifacts; committing telemetry to Git is prohibited outright.
- UI redesign beyond presenting the task and feature reports.
- Reopening F10's historic baseline.

## Definition of done

- Neutral usage type compiles in api.rs; pi_events extracts observed usage; pi_harness hands off an InvocationRecord to the store hook defined in task 1; three gates green

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
