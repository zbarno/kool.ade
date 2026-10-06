---
koolade-task: {"schemaVersion":1,"uid":"00b3df5f-9bfa-40d8-a8d3-2b748fd2e4f4","batchUid":"ddbb6769-ec92-42c8-9651-7a91475c7109","repositoryId":"root","dependencyUids":[]}
---

# F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage — Define versioned JSONL invocation record and telemetry storage
<!-- koolade-artifact-id:v1 {"uid":"00b3df5f-9bfa-40d8-a8d3-2b748fd2e4f4","displayId":"F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage","title":"Define versioned JSONL invocation record and telemetry storage","parentUid":"ddbb6769-ec92-42c8-9651-7a91475c7109"} -->

Feature: F11 - Durable operator-local implementation telemetry (zbarno/kool.ade issue #23)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

No per-invocation telemetry is persisted: pi's usage collapses to final text and there is no durable record tying tokens, model, or cost to a task. Fix the concrete record contract and store every other task builds on.

## Ticket goal — what changes when done

A version-tagged JSONL invocation record type and a concurrency-safe append store exist under $KOOLADE_HOME/projects/<project>/telemetry/; appending does not touch or dirty the repo.

## User story

As an operator I want durable operator-local telemetry so I can explain a task's real cost in time and model usage.

## Purpose

Fix the concrete durable record contract (schema version tag, attribution identity, token/duration/outcome fields) and the concurrency-safe append store under $KOOLADE_HOME/projects/<project>/telemetry/, establishing the source-of-truth primitive every other task depends on.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Operator-local store already resolves via state_root() honoring KOOLADE_HOME and project_dir(slug) building <root>/<slug> (home.rs); JSONL append and corruption-quarantine precedent in chat_store.rs.

Feature ID: F11
Repository: root

## Approved scope mapping

- Scope 2: Durable invocation records under $KOOLADE_HOME/projects/<project>/telemetry/ - append-oriented, version-tagged JSON Lines, concurrency-safe - attributable to project, session, repository, feature, task batch, task, implementation phase, harness, and provider/API/model (R2, R3).
- Success criterion 3: Each completed model call is counted exactly once; streamed or cumulative usage updates never double-count, and token categories reconcile without double-counting reasoning tokens already included in output (AC10-AC13).

## Dependencies

None. This task can start independently.

## Affected files and components

- src/persistence/home.rs - confirm state_root and project_dir for the project slug path
- src/persistence/telemetry/store.rs - new append-oriented versioned JSONL store under projects/<slug>/telemetry/

## Implementation steps

1. Define InvocationRecord struct with schemaVersion, attribution (project, session, repository, feature, batch, task, phase, harness, provider, api, model), five token fields plus total, duration mono plus wall timestamps, and outcome; serde JSON with null-tolerant optionals
2. Add store.rs opening projects/<slug>/telemetry/invocations.jsonl with O_APPEND, write newline-delimited versioned objects, quarantine unparseable lines
3. Read back records verifying parse-skip and that writing creates nothing in the repo worktree

## Acceptance criteria

- Appending a record yields one parseable line carrying full attribution and token fields without altering the repository worktree status
- Corrupt trailing line is skipped without failing earlier record reads

## Test plan

1. Append two records from separate tasks concurrently into one telemetry dir and assert both present with distinct attribution
2. Append then inject a bad line and assert read skips it and prior records load

## Verification commands and expected evidence

1. Run cargo +1.98.1 test --locked --all-targets -- --test-threads=1 expecting the new telemetry store tests to pass

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

- Compile green, store tests pass, repo untouched

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
