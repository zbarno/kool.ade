---
packet-task: {"schemaVersion":1,"uid":"e5bdfc69-4e44-489d-8800-0ed90e08bb60","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":["cc582e2e-e762-45cc-bc5c-87b811421c41","f7f19f0b-d39b-45fa-9d00-89bbb7e00bbd","2aa220ea-bf9a-4137-a005-ce35a20d7282","223d9292-5a9a-4a79-92d6-26007acdc815","35d88288-7a47-498b-aba9-b877dbec6dd8"]}
---

# F7-TASK-verify-tracker-isolation-and-enforce-the-regression-quality-bar — Verify tracker isolation and enforce the regression quality bar
<!-- packet-artifact-id:v1 {"uid":"e5bdfc69-4e44-489d-8800-0ed90e08bb60","displayId":"F7-TASK-verify-tracker-isolation-and-enforce-the-regression-quality-bar","title":"Verify tracker isolation and enforce the regression quality bar","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Tasks 2-6 added ledger reads/writes adjacent to hot paths (accrual hook in the app loop, totals folded into the board ViewModel, shutdown flush, month-report writes). Nothing currently proves those additions leave planning turns, queue progression, and board rendering undisturbed (R6/AC4), nor pins the clippy baseline NFR-8/D-34 requires. Without this, F7 ships unverified against AC3/AC4/AC5.

## Ticket goal — what changes when done

cargo test and cargo clippy pass clean, and new regression tests demonstrate concurrent tracker activity leaves turn completion, queue advancement, and board paints unchanged, with a recorded clippy baseline file committed.

## User story

As the operator-biller I want proof that F7 never disturbs my planning or my queue and that the crate stays warning-clean so my totals arrive trusted, not corrupted by collateral regressions.

## Purpose

Extend the regression suite proving continuous tracker operation leaves planning turns, queue progression, and board rendering undisturbed with no deadlocks or stalls, reconcile mixed-workspace interval sums independently, and pin a clippy baseline keeping the suite green with zero new warnings at the pinned toolchain.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified: single crate packet (edition 2024); src/core/turn.rs orchestrates turns off the UI thread behind TurnController; src/core/implementation_queue.rs exposes next_ticket over Queue; board ViewModel lives in src/ui/planning_board.rs (task 4 extended it). Tests are inline #[cfg(test)] modules and #[path="..."] siblings (see core/implementation_queue.rs). No clippy baseline file exists in-tree; discovery of any CI script under .github/ or scripts/ is required before choosing the recording location. chrono 0.4.45 available for synthetic interval fixtures.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 6: Isolation: ledger writes never disturb planning turns, queue progression, board rendering, or network activity (R6)
- Success criterion 3: A mixed week across at least two workspaces matches an independent re-sum of recorded intervals, and the month-end markdown report's per-day, per-feature lines sum exactly to the on-screen workspace totals with identical regeneration from the ledger (AC3)
- Success criterion 4: Operator-wait hold windows (unanswered question or held approval) accrue zero minutes with no backfill after release, and one workspace's total grows at no faster than one times wall rate even with concurrent workers (AC6)
- Success criterion 5: Full regression suite green with no new clippy warnings versus the recorded baseline at the pinned toolchain (AC5, D-34)

## Dependencies

- [F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals](F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals.md) must be complete.
- [F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals](F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals.md) must be complete.
- [F7-TASK-surface-day-and-week-time-totals-on-existing-board-and-detail-surfaces](F7-TASK-surface-day-and-week-time-totals-on-existing-board-and-detail-surfaces.md) must be complete.
- [F7-TASK-generate-the-completed-month-markdown-report-from-the-ledger](F7-TASK-generate-the-completed-month-markdown-report-from-the-ledger.md) must be complete.
- [F7-TASK-integrate-ledger-writes-on-coarse-grains-with-session-lifecycle-and-app-shutdown](F7-TASK-integrate-ledger-writes-on-coarse-grains-with-session-lifecycle-and-app-shutdown.md) must be complete.

## Affected files and components

- src/core/time_tracker_regression.rs (new: integration test exercising ledger reads/writes, accrual hooks, totals, and month report alongside a synthesized turn and a queue advance, asserting no behavioral drift)
- src/ui/planning_board.rs test module (extend with a regression case ensuring the totals field population does not alter existing board invariants when ledger is absent or present)
- clippy_baseline.txt (new: recorded warnings from pinned toolchain; exact path decided in step 2 by locating or creating the scripts/ or docs/ convention)

## Implementation steps

1. Discovery: locate the pinned toolchain file (rust-toolchain.toml) and any existing clippy invocation under CI/scripts; record exact cargo invocations used elsewhere so the baseline uses identical flags. Confirm no pre-existing regression suite under tests/ or benches/ that should host these tests instead.
2. Author src/core/time_tracker_regression.rs with inline #[cfg(test)] tests: (a) synthesize IntervalRows for two workspaces across a mixed week, call core::time_totals, and independently re-sum raw merged spans to assert equality (AC3). (b) Run TurnController-style pipeline with accrual hooks subscribed for a workspace; assert the turn reaches Applied and queue.next_ticket progresses as before, with no additional delay beyond baseline jitter.
3. Author a board-viewmodel regression test asserting (i) absence of ledger yields None totals and identical rendered output, (ii) presence of ledger totals does not reorder existing ViewModel fields.
4. Pin clippy baseline: run cargo clippy --workspace --tests and stash the output into clippy_baseline.txt (location confirmed in step 1). Add a small note file explaining regeneration.
5. Wire the baseline diff into the standard verification command so a reviewer comparing against it fails loudly on any new warning.

## Acceptance criteria

- Mixed-workspace fixture: per-workspace day and ISO-week totals from time_totals equal an independent merge-and-sum of the same IntervalRows.
- With a workspace meter open and accumulating, a full planning turn completes and next_ticket advances; no deadlock, no hang, and no change to turn timeout constants.
- Board view-model construction with and without ledger rows both succeed and preserve existing field ordering.
- Running cargo clippy --workspace --tests against the pinned toolchain produces zero new diagnostics relative to clippy_baseline.txt.

## Test plan

1. Fixture of 3 synthetic rows spanning two workspaces, one deliberately overlapping pair: assert per-workspace/day totals equal an independent merged-span sum.
2. Drive TurnController with accrual hooks wired to a fake workspace; assert Applied status and next_ticket() progression identical to a run without hooks.
3. Call the board-view-model assembler twice: once with empty ledger, once with populated; assert identical non-time-related serialization between the two runs.
4. Snapshot clippy output before/after landing; assert the diff introduces no new lint hits.

## Verification commands and expected evidence

1. cargo test -p packet (full suite green)
2. cargo clippy --workspace --tests (compared to clippy_baseline.txt: zero new warnings)
3. cargo test -p packet time_tracker_regression (targeted suite for new tests)

## Edge cases and failure handling

- Absence of ledger on a fresh project must not slow down ViewModel construction measurably or introduce any new allocation-heavy path in steady state.
- Clippy baseline must exclude known third-party crate warnings, otherwise trivially noisy diffs will mask genuine regression signals.

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

- New regression tests committed and passing; clippy baseline checked in; full suite green with zero new clippy warnings versus the pinned toolchain baseline.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
