---
packet-task: {"schemaVersion":1,"uid":"cc582e2e-e762-45cc-bc5c-87b811421c41","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":["b12e64c3-317d-41f5-a565-99e539ccd400"]}
---

# F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals — Implement agent-activity observation driving per-workspace accrual intervals
<!-- packet-artifact-id:v1 {"uid":"cc582e2e-e762-45cc-bc5c-87b811421c41","displayId":"F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals","title":"Implement agent-activity observation driving per-workspace accrual intervals","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Time is not captured today; the ledger from task 1 stores rows but nothing opens them. An accrual engine must watch agent execution state so only active-execution minutes become intervals, satisfying AC1/AC3/AC6.

## Ticket goal — what changes when done

When an agent is actively executing on workspace W, an interval row appears in the ledger on transition; on operator-wait, idle, paused, dormant, or app-close the interval closes and no further time accrues. Same-workspace overlap counts once.

## User story

As the operator-biller I want Packet to record minutes only while an agent is really working in a workspace so my totals reflect actual effort, giving me honest daily figures.

## Purpose

Add the core accrual engine watching agent execution state per workspace, opening/closing intervals only during active execution, stopping on operator-wait (unanswered question, held approval), idle, paused, dormant, or app-close. Enforce union-on-overlap per workspace so a single workspace accrues at <=1x wall rate with concurrent workers, and ensure wait-holds backfill zero on release.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Task 1 provides time_ledger append_row/load with IntervalRow (start_epoch_s, end_epoch_s, workspace_id, session_id, end_status). App owns sessions and worker/queue coordination per module 08; agent execution/wait state is surfaced there. Exact state enum and hook point need discovery under src/app; likely an execution-state tracker or queue callback. Union-on-overlap must be enforced at interval-open: refuse to open a second row for a workspace already having an open interval.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 2: Automatic agent-activity capture: meters run only while an agent is actively executing on the workspace and stop on operator-wait, idle, paused, dormant, or app-closed states (AD-4, CLR-029)

- Scope 4: Per-workspace day and ISO-week totals on existing board surfaces, including the day in progress (R4, AD-8)
- Success criterion 1: Minutes accruing while an agent actively works inside workspace W appear in W's day total within at most one minute of rounding; minutes accruing with no workspace-attached agent active appear in no workspace total (AC1)
- Success criterion 3: A mixed week across at least two workspaces matches an independent re-sum of recorded intervals, and the month-end markdown report's per-day, per-feature lines sum exactly to the on-screen workspace totals with identical regeneration from the ledger (AC3)
- Success criterion 4: Operator-wait hold windows (unanswered question or held approval) accrue zero minutes with no backfill after release, and one workspace's total grows at no faster than one times wall rate even with concurrent workers (AC6)

## Dependencies

- [F7-TASK-define-git-backed-time-ledger-artifact-and-atomic-row-persistence](F7-TASK-define-git-backed-time-ledger-artifact-and-atomic-row-persistence.md) must be complete.

## Affected files and components

- src/core/time_accrual.rs (new: per-workspace Accruer holding open intervals, watches execution-state events, emits ledger rows via task-1 append_row)
- src/app/session*.rs or src/app/mod.rs (wire execution-state transitions into Accruer; discovery needed for exact hook site)

## Implementation steps

1. Discover where app tracks per-worker execution phase (active vs waiting/idle/paused/dormant) and how workspace identity attaches to a worker; record the callback site.
2. Create src/core/time_accrual.rs exposing Accruer::{on_active_start(workspace_id, session_id, item_uid, now), on_active_stop(reason, now)} with interior state mapping workspace_id to an OpenSlot {started_at, session_id}.
3. On on_active_start: if a slot exists for that workspace, ignore (union-on-overlap) so concurrent workers on one workspace do not stack time; else insert slot and defer ledger write until close.
4. On on_active_stop: pop slot, compute duration = now - started_at; if duration < 1 second drop it silently to avoid noise; else call ledger.append_row with EndStatus::Ended.
5. On app close or shutdown hook, flush all open slots using the same stopped logic marked EndStatus::InterruptedDiscard to prevent double-count on restart.
6. Wire callbacks at the discovered execution-state transitions; route operator-wait (unanswered question or held approval) and idle to on_active_stop; resume-active passes to on_active_start producing a fresh interval (zero backfill).
7. Attach workspace_id and item_uid from the worker's bound task; attach Feature ref read-time later by task 3, leaving feature_ref None here.

## Acceptance criteria

- Starting an active agent on workspace W for N>=1s produces exactly one ledger row covering approximately N seconds.
- Two concurrent active agents on the same workspace yield at most one open interval and total elapsed never exceeds wall time.
- Triggering operator-wait mid-interval closes the current interval and releases zero additional time; resuming starts a fresh interval (no backfill).
- Idle, paused, dormant, and app-close each stop accrual on the transition tick with no lagged continuation.
- Restart after an unflushed interval sees it as discarded (not re-emitted) because flush happened before exit.

## Test plan

1. Unit: feed synthetic start/stop pairs to Accruer in isolation; assert emitted IntervalRows respect union rule and drop sub-second durations.
2. Integration: simulate two workers opening on the same workspace_id within 1s and assert only one open slot exists.
3. Wait-hold case: open, hold 3 simulated minutes, release; assert exactly one row ending at hold-start with duration < 1 min of pre-hold time and zero additional on release.
4. Shutdown case: leave an open slot, invoke flush, assert row appended with InterruptedDiscard and cleared state.

## Verification commands and expected evidence

1. cargo build -p packet --lib
2. cargo test --lib core::time_accrual
3. cargo clippy --workspace -- -D warnings compared against recorded baseline

## Edge cases and failure handling

- Same session emitting rapid start-stop churn under 1s accumulates no noise rows thanks to the drop threshold.
- Cross-session overlap on one workspace is treated as one active span, honoring AC6 upper bound of 1x wall.

## Constraints

- Durable time records are git-backed project artifacts applied atomically, in a dedicated ledger slot with coarse commit grain (NFR-1/NFR-2, AD-3)
- Network neutrality: no cloud sync or telemetry; the monthly report is a local file (NFR-5, R6)
- Device-local clocks with no DST correction; anomalous clocks are corrected by the operator (AD-2)
- Linux x86_64 desktop from source; no packaging commitment (NFR-7)
- Quality bar: full regression suite green and zero new clippy warnings versus the recorded baseline at the pinned toolchain (NFR-8/D-34, AC5)

## Out of scope

- Rates, tax, invoice emission, and payments - the export hands off to the operator's invoicing tool
- In-app client administration: rosters, labels, renames, merges, and workspace-to-client mapping; multi-workspace client bills compose outside Packet
- Manual start/stop timers and retrospective time entries as the primary capture path (optional layers at most later)
- Multi-operator billing, client portals, cloud sync, remote capture, and client inference from repository metadata
- Windows and macOS builds, and rolling 30-day export windows (calendar months only in v1)

## Definition of done

- Accruer compiled and wired to execution-state transitions; union and backfill-zero behaviors proven by unit/integration tests; clippy diff clean.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
