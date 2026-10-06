---
koolade-task: {"schemaVersion":1,"uid":"7d656182-cf2f-4dde-9df5-66ee2071e145","batchUid":"ddbb6769-ec92-42c8-9651-7a91475c7109","repositoryId":"root","dependencyUids":["00b3df5f-9bfa-40d8-a8d3-2b748fd2e4f4","5d1fd5d4-0b9b-4474-9771-ecf52184705d"]}
---

# F11-TASK-guarantee-exactly-once-and-reconciled-token-counting — Guarantee exactly-once and reconciled token counting
<!-- koolade-artifact-id:v1 {"uid":"7d656182-cf2f-4dde-9df5-66ee2071e145","displayId":"F11-TASK-guarantee-exactly-once-and-reconciled-token-counting","title":"Guarantee exactly-once and reconciled token counting","parentUid":"ddbb6769-ec92-42c8-9651-7a91475c7109"} -->

Feature: F11 - Durable operator-local implementation telemetry (zbarno/kool.ade issue #23)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

pi's NDJSON folds only text/activity/stop reason/tool counts (EventFold in pi_events.rs); nothing dedupes streamed or cumulative usage or reconciles the token categories. Without exactly-once, per-call accounting, repeated usage frames would inflate totals and reasoning already in output would double-count, corrupting every downstream cost figure (AC10-AC13).

## Ticket goal — what changes when done

A pure reconciled-per-call computation plus a message-keyed collector yield exactly one model call with reconciled tokens per assistant message; streamed/cumulative frames replace rather than accumulate, and reasoning-in-output is not double-counted.

## User story

As an operator I want each model call counted exactly once with consistent token math, so my reported usage and cost reflect reality and trust in the ledger holds.

## Purpose

Ensure each model call is counted once, streamed or cumulative updates never double-count, and the five token categories plus total reconcile per provider semantics so reasoning already in output is not doubled.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified: pi_events.rs resets message_id on message_start (synthetic u64, monotonically increasing); message_update accumulates _delta/_end content but parses no token fields. Helpers are in pi_events/helpers.rs. Task 2 supplies the raw per-frame usage extraction feeding UsageObservations. No usage or dedup code exists yet; the stable per-call key is message_id scoped to the current invocation.

Feature ID: F11
Repository: root

## Approved scope mapping

- Scope 3: Exactly-once per-call counting with provider-faithful token reconciliation: reasoning is a subset of output where the provider says so (Scope, AC12-AC13).
- Success criterion 3: Each completed model call is counted exactly once; streamed or cumulative usage updates never double-count, and token categories reconcile without double-counting reasoning tokens already included in output (AC10-AC13).

## Dependencies

- [F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage](F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage.md) must be complete.
- [F11-TASK-normalize-pi-harness-usage-into-incremental-records](F11-TASK-normalize-pi-harness-usage-into-incremental-records.md) must be complete.

## Affected files and components

- src/harness/pi_events.rs - per-assistant-message usage accumulator keyed by message_id, fed from fold_line, exposing reconciled final values
- src/harness/pi_events/usage.rs - new: pure token-reconciliation and exactly-once reducer (unit-testable)
- src/harness/pi_events/tests/event_fold.rs - fold-driven dedup and reconciliation fixtures

## Implementation steps

1. Confirm the exact Pi NDJSON usage frame shapes and the per-call stable key/message scoping in docs/json.md referenced at the top of pi_events.rs; align the collector key to the assistant message_id assigned on message_start
2. In usage.rs add a TokenTotals struct (input, output, cache_read, cache_write, reasoning, total) and a pure reconcile(TotalObserved, ProviderConvention) -> ReconciledTokens: when reasoning_includes_output=true subtract reasoning from output once, else add it; recompute total = input + cache_read + cache_write + output_adjacent (+ reasoning only when not already in output); treat a provided provider total as authoritative when consistent, else fall back to the summed form; clamp negatives to None
3. Add a reduce(UsageAccumulator, UsageFrame) that is idempotent per call: frames sharing a call key replace the latest observed snapshot (never add), so cumulative and streamed variants converge on one final count; expose exactly_one_final(key) so each message contributes one counted call
4. Wire the accumulator into EventFold keyed by message_id, resetting on message_start so each assistant message is an independent counted call; emit the reconciled ReconciledTokens per finalized call to pi_harness (which composes the InvocationRecord in task 2)
5. Ensure a mid-stream disconnect (Closed without message_end) still emits the last observed snapshot once, so a partially-reported call is counted exactly once without fabrication

## Acceptance criteria

- One assistant message receiving N cumulative usage frames yields exactly one counted call with tokens equal to the final snapshot, not the sum
- When the provider signals reasoning-in-output, the stored total excludes a second copy of reasoning and matches the provider total where supplied
- Five distinct categories and the total are preserved independently and agree under the active provider convention
- Two concurrent fold states never share call counters; each counts its own messages exactly once

## Test plan

1. feed_line sequence: one message_id with three monotonically growing cumulative frames plus a final message_end asserting one call and final-snapshot tokens
2. Delta variant: interleaved _delta additions for one message converging to the same single count as the cumulative case
3. Reasoning reconciliation: provider flag true with reasoning=r asserts total = input+cache_r+cache_w+output and equals the supplied provider total
4. Disconnect case: truncate the stream after frames but before message_end and assert exactly one reconciled call is emitted once

## Verification commands and expected evidence

1. cargo +1.98.1 test --locked --all-targets -- --test-threads=1 pi_events expecting the new exactly-once and reconciliation unit/integration tests to pass

## Edge cases and failure handling

- Provider reports reasoning already contained in output - must not add again
- Cumulative snapshot regression (a lower-than-previous total from a late frame) - accept latest snapshot rather than max-sum

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

- Exactly-once dedup, replace-not-add reduction, and per-provider reconciliation pass; reasoning-in-output proven not double-counted; three gates green

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
