# F11: Implement zbarno/kool.ade issue #23
<!-- koolade-artifact-id:v1 {"uid":"8b0cc126-4ef4-4a5b-9a2d-7452e96c2cae","displayId":"F11","title":"Implement zbarno/kool.ade issue #23"} -->
<!-- koolade-change:v1 {"schemaVersion":1,"uid":"8b0cc126-4ef4-4a5b-9a2d-7452e96c2cae","displayId":"F11","status":"ready"} -->

**Status:** Ready — issue #23 content transcribed from the operator paste delivered in the CLR-011 thread; no blocking open items.

## Intent

Implement zbarno/kool.ade issue [#23](https://github.com/zbarno/kool.ade/issues/23): add durable, user-local implementation telemetry so Kool.ad/e can explain what a task or feature actually cost to implement in both **time** and **model usage**. After a task is implemented, the operator should be able to see: total elapsed time, active implementation time, AI/harness execution time, model-call count, full token usage breakdown, harness(es) used, provider/API/model(s) used, reasoning/thinking level where available, estimated API cost, breakdown by harness/model, breakdown by implementation phase, and a rollup from individual tasks to the containing feature.

Provenance: the planning sandbox is network-isolated and no repository artifact mirrors the issue, so the issue body was transcribed from the operator-supplied paste in the CLR-011 thread. The paste opens at the issue's Goal section and carries no separate title line; the Goal lead sentence ('Add durable, user-local implementation telemetry so Kool.ad/e can explain what a task or feature actually cost to implement') stands in as the working title until the exact title is supplied (cosmetic residue, not blocking). The issue's Why, Desired report, Requirements, Implementation notes, and acceptance checklist are folded into Current Behavior, Desired Behavior, Requirements, and Acceptance Criteria below.

## Current Behavior

- Workspace-scoped time ledger: `.koolade-packet/state/time-ledger.log` (header `time-ledger:v1`; rows `T:<repo>:<workspace>:<session>:<pid>:<start>:<end>:<item_uid>:<feature_ref>:<end_status>`) is appended atomically and is Git-ignored (verified in `.gitignore`). Rows are keyed by (repo, workspace) with a count-vs-discard rule for interrupted intervals: a 'how busy was this repository' accrual, not per-invocation accounting; concurrent tasks in one workspace commingle on a shared workspace clock.
- Operator-local persistence already exists outside Git: `$KOOLADE_HOME` (default `~/.koolade-packet`)/projects/<slug> holds per-operator runtime files keyed by a stable repository slug, with JSONL chat-store precedent — the same home shape the issue proposes for telemetry.
- Harness boundary: pi's NDJSON event stream is folded into final text, live activity, stop reason, error class, and tool-execution counts; the fold and its consumers expose no normalized token, model, or cost usage, and no per-invocation usage record is persisted anywhere.
- Execution spans around implementation, reconciliation, and verification harness calls provide the interception points the issue names, and task/batch metadata already expose stable identities with the batch carrying the feature id.

## Desired Behavior

The source of truth shifts from wall-clock accrual to **raw invocation-level telemetry**; wall-clock and aggregate figures are derived from it.

- Telemetry is operator-local: records live under `$KOOLADE_HOME/projects/<project>/telemetry/` (the issue's preferred location, adopted because it coincides with the existing operator-local project-directory layout), never become shared project truth, and never require a Git commit. The existing Git-ignored ledger stays ignored.
- Every AI/harness invocation is its own record, attributable to project, session, repository, feature, task batch, task, implementation phase, harness, and provider/API/model — including when multiple tasks execute concurrently against the same repository.
- The harness boundary exposes normalized usage without making higher-level code Pi-specific (field list in R4), emitted incrementally where the backing harness provides it; missing provider fields degrade gracefully instead of blocking the rest of the invocation's telemetry.
- Incurred usage survives failure: tokens consumed before a failure, timeout, cancellation, or retry still count; retries append rather than overwrite.
- Cost is stable: displayed as 'Estimated API cost' unless Kool.ad/e has authoritative billing data; a run's reported cost stays attached to that historical execution and is never recomputed from later pricing.
- Reporting presents the task metric below — presentation may evolve, but it must clearly separate the three times — and rolls the same shape up across all tasks of a feature:

```text
Implementation metrics
  Elapsed                        18m 42s
  Active implementation time     14m 08s
  AI execution time              11m 51s
  Model calls                      17
  Tokens   Input                   842,119
           Cache read              691,440
           Cache write              38,212
           Output                   29,841
           Reasoning                18,104
           Total                    910,172
  Estimated API cost             $1.84
Breakdown
  Pi - Claude Sonnet 4.5     9m 14s - 13 calls - 721k tokens - $1.41
  Pi - GPT-5.6               2m 37s -  4 calls - 189k tokens - $0.43
By phase
  Reconciliation         1m 12s - $0.09
  Implementation         8m 46s - $1.49
  Repair / retry         1m 53s - $0.26
```

Time definitions: **Elapsed** runs first implementation start through completion; **Active implementation time** is time Kool.ad/e was actively working, excluding human/operator waits; **AI execution time** is actual harness/model execution. Existing day/week and workspace-union timing may remain as derived views where useful but must not suppress task-level invocation data. The telemetry model stays transport-neutral so future harnesses (Codex, Claude Code, Copilot CLI, OpenCode, ...) can populate it without changing task-report code.

## Scope

Included:
- Normalized usage capture at the harness boundary (harness identity/version, provider, API/backend, requested vs actual model, reasoning/thinking level, model-call count, input/output/cache-read/cache-write/reasoning/total tokens, reported or estimated cost, duration, outcome/stop reason), emitted incrementally during invocations.
- Durable invocation records under `$KOOLADE_HOME/projects/<project>/telemetry/` — append-oriented, versioned, concurrency-safe — carrying full attribution identity.
- Exactly-once per-call counting; no double counting from streamed or cumulative usage updates; provider-faithful token reconciliation (reasoning is a subset of output where the provider says so).
- Retention of failed/timeout/cancelled/retried usage; retries append new records.
- Stable cost semantics per R6, including the fallback estimation policy tracked on the board.
- Task and feature reporting: rollups, harness/model and phase breakdowns, token-category totals, estimated-cost totals; existing timing views demoted to derived.
- Ledger continuity: unchanged v1 format, still Git-ignored; any migration or retirement explicit and safe, no backfill.
- Tests per AC27 under the standing three-gate landing rule (R9).

Excluded:
- Authoritative billing or invoicing integration; live pricing feeds.
- Populating any harness other than Pi (the neutral model admits them later).
- Backfilling work performed before telemetry exists.
- Modifying shared planning-state artifacts; committing telemetry to Git is prohibited outright.
- UI redesign beyond presenting the task/feature reports.
- Reopening F10's historic baseline.

## Affected Product Areas

- **Architecture and Constraints** — a new operator-local persistence lane (telemetry under the project directory) and a normalized usage contract on the harness boundary; the time ledger's relation to reporting is fixed as a derived view.
- **Current Capabilities** — gains observability of implementation cost (times, model calls, tokens, estimated cost) per task and per feature.
- **Quality and Acceptance** — extended test and gate duties: same-repository concurrency, retry/failure accounting, token deduplication, cost persistence, task/feature rollups, Git cleanliness.

## Requirements

- R1 — Record the canonical issue content in this specification: Goal into Intent/Desired Behavior; the issue's Requirements into R2-R8; the issue's acceptance checklist into AC1-AC27; residue (working-title substitution) noted. *Satisfied by this revision.*
- R2 — User-local telemetry: records persist under `$KOOLADE_HOME/projects/<project>/telemetry/` (preferred-location choice over 'an equivalent user-local location'; tracked on the board), in an append-oriented, versioned form; creating or updating telemetry neither Git-commits nor dirties the project repository; `.koolade-packet/state/time-ledger.log` remains Git-ignored (kept, or safely migrated/retired without ever becoming tracked).
- R3 — Raw invocation-level source of truth: each AI/harness invocation is recorded independently and attributable to project, session, repository, feature, task batch, task, implementation phase, harness, and provider/API/model; workspace-unioned timing is never the source of truth; two concurrent tasks against the same repository each retain complete, individual histories without mutual suppression.
- R4 — Harness-boundary normalization: expose normalized execution usage without making higher-level application code Pi-specific: harness identity/version, provider, API/backend, requested model, actual/response model where distinguishable, reasoning/thinking level, model-call count, input/output/cache-read/cache-write/reasoning/total tokens, reported or estimated cost, execution duration (monotonic where practical, plus wall-clock timestamps for timeline placement), and outcome/stop reason; emit incrementally where the harness streams usage; missing provider-specific fields degrade gracefully and never prevent the remainder of the invocation's telemetry.
- R5 — Failed and interrupted work: usage incurred before a harness failure, timeout, cancellation, or retry is retained; a retry appends new telemetry without overwriting or duplicating earlier attempts.
- R6 — Cost semantics: display 'Estimated API cost' unless Kool.ad/e has authoritative billing data; a harness/provider-reported cost stays attached to that historical execution; estimates may use the fallback policy (vendor-independent bundled per-model price table; unknown price renders n/a — tracked on the board); old runs are never recomputed from later pricing or configuration.
- R7 — Derived reporting: task reports and feature rollups share one shape; harness/model breakdowns and phase breakdowns; token-category totals; estimated-cost totals; the three-time definitions are honored; task-level aggregates reconcile with invocation records and feature-level aggregates reconcile with the sum of task-level metrics.
- R8 — Operational resilience: the existing live elapsed-time UI keeps functioning while a task runs; recorded metrics remain available independently of the original application session; telemetry persistence failures surface to the user but do not corrupt or abort otherwise valid implementation work; missing optional provider fields do not fail the implementation.
- R9 — Landing gates: fmt, test, and clippy on Rust 1.98.1 all pass before any resulting commit, and the AC27 test matrix is in place.

## Decisions and Assumptions

- Provenance: content is the operator's transcript from the CLR-011 thread; the paste carried no separate title line, so the Goal lead sentence is the working title; correct on receipt of the exact title (cosmetic only).
- Location: adopt the issue's preferred `$KOOLADE_HOME/projects/<project>/telemetry/`; it coincides with the existing operator-local project-directory layout (slug-keyed under `state_root()`). Filed as a non-blocking Agent item on the board.
- Format: append-oriented, version-tagged JSON Lines (one record per line, per-record schema version), following the existing JSONL persistence precedent and the issue's guidance that this evolves more easily than extending the colon-delimited ledger; reversible before rollout. Filed as a non-blocking Agent item on the board.
- Cost fallback: where no provider-reported cost exists, estimate from a vendor-independent, versioned per-model price table bundled with Kool.ad/e; calls without a known price render n/a and contribute nothing; no fabricated precision; table updates never reprice historical runs. Filed as a non-blocking Agent item on the board.
- Token semantics: keep the five categories plus total independently stored; honor provider conventions so totals reconcile without double-counting reasoning tokens already included in output.
- Durations: monotonic measurement where practical; wall-clock timestamps retained for timeline placement.
- Ledger: remains functional and Git-ignored; its v1 format is not extended for this feature; any retirement is a separate, explicit, safe migration.
- Neutrality: the telemetry model is transport-neutral; Pi populates it initially and task-report code stays harness-free.
- Earlier interim assumption retired: '#23 is distinct from archived F8/F9/F10 work' — confirmed by the transcribed content, which overlaps none of them.
- Lifecycle: Ready; implementation still requires the ordinary explicit feature approval and task generation.

## Acceptance Criteria

- AC1 — Completed tasks expose a durable implementation metrics report with elapsed time, active implementation time, AI execution time, model-call count, token breakdown, harness/model breakdown, phase breakdown, and estimated API cost.
- AC2 — Completed feature reporting can roll up the same metrics across all tasks belonging to that feature.
- AC3 — Telemetry is stored in operator-local persistence and is not committed to Git.
- AC4 — Creating and updating telemetry does not make the project repository dirty.
- AC5 — The existing ignored `.koolade-packet/state/time-ledger.log` remains ignored or is safely migrated/retired without becoming tracked.
- AC6 — Every implementation-related harness invocation can be attributed to a stable task identity.
- AC7 — Telemetry can also associate an invocation with its task batch and feature when those identities are available.
- AC8 — Two tasks running concurrently against the same repository both retain their complete individual invocation history, time, token usage, and cost.
- AC9 — Concurrent work does not cause one task's telemetry to be suppressed because another task already has an active workspace span.
- AC10 — Each completed model call is counted exactly once.
- AC11 — Streaming or cumulative usage updates do not cause token usage to be double-counted.
- AC12 — Input, output, cache-read, cache-write, reasoning, and total token fields are preserved independently when supplied by the harness/provider.
- AC13 — Token totals and displayed category breakdowns reconcile according to the semantics of the backing provider and do not double-count reasoning tokens where reasoning is already included in output.
- AC14 — Harness identity/version is captured for recorded executions.
- AC15 — Provider/API/model identity is captured when available.
- AC16 — Requested model and actual response model can be distinguished when the harness exposes both.
- AC17 — Reasoning/thinking level is captured when available.
- AC18 — Usage already incurred before a harness failure, timeout, cancellation, or retry is retained.
- AC19 — Retrying an implementation adds new telemetry without overwriting or duplicating the usage from earlier attempts.
- AC20 — Historical estimated cost remains stable if model pricing or configuration changes later.
- AC21 — Missing optional provider-specific telemetry does not cause the implementation itself to fail.
- AC22 — Telemetry persistence failures are surfaced to the user but do not corrupt or abort otherwise valid implementation work.
- AC23 — Task-level aggregate metrics reconcile with the underlying invocation/model-call records.
- AC24 — Feature-level aggregate metrics reconcile with the sum of its task-level metrics.
- AC25 — Existing live elapsed-time UI continues to function while a task is running.
- AC26 — Once a task is complete, durable recorded metrics are available independently of the original application session.
- AC27 — Tests cover concurrent tasks in the same repository, retry/failure accounting, token deduplication, cost persistence, task/feature rollups, and Git cleanliness.
