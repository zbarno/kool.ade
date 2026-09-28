---
packet-task: {"schemaVersion":1,"uid":"2aa220ea-bf9a-4137-a005-ce35a20d7282","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":["f7f19f0b-d39b-45fa-9d00-89bbb7e00bbd"]}
---

# F7-TASK-surface-day-and-week-time-totals-on-existing-board-and-detail-surfaces — Surface day and week time totals on existing board and detail surfaces
<!-- packet-artifact-id:v1 {"uid":"2aa220ea-bf9a-4137-a005-ce35a20d7282","displayId":"F7-TASK-surface-day-and-week-time-totals-on-existing-board-and-detail-surfaces","title":"Surface day and week time totals on existing board and detail surfaces","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Board and detail surfaces today carry no per-workspace time figures: src/ui/planning_board.rs's ViewModel holds only task documents, planning work, open items, and setup state, so the day/week totals task 3 derives are invisible and unused. The operator cannot confirm accrual while working.

## Ticket goal — what changes when done

On the existing board and task-detail surfaces, each workspace shows its in-progress day and current ISO-week time totals, refreshed as the day accumulates, with no change to any existing board interaction.

## User story

As the seated operator-biller I want my connected workspace's day and week hours visible on the board I already watch so I can see billable time accrue live and catch idle meters immediately.

## Purpose

Hook per-workspace day and ISO-week totals onto the existing board/detail views the operator already uses, showing live totals for the day in progress. Additive observer over board rendering, honoring R6 by not disturbing existing board interactions.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Single-crate egui app; board read-model is src/ui/planning_board.rs ViewModel, assembled by the app (src/app/) before paint, consumed by panes incl. src/ui/items_pane.rs and src/ui/layout/task_details.rs. Task 3 delivers pure src/core/time_totals.rs (day_totals/week_totals over IntervalRows, incl. in-progress day). Verify exact assembly site in src/app (likely manager.rs/root.rs) and the workspace-list widget (layout/workspace_repositories.rs); discover in step 1. UI is additive: read-only totals, no new commands.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 4: Per-workspace day and ISO-week totals on existing board surfaces, including the day in progress (R4, AD-8)
- Success criterion 1: Minutes accruing while an agent actively works inside workspace W appear in W's day total within at most one minute of rounding; minutes accruing with no workspace-attached agent active appear in no workspace total (AC1)
- Success criterion 3: A mixed week across at least two workspaces matches an independent re-sum of recorded intervals, and the month-end markdown report's per-day, per-feature lines sum exactly to the on-screen workspace totals with identical regeneration from the ledger (AC3)

## Dependencies

- [F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals](F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals.md) must be complete.

## Affected files and components

- src/ui/planning_board.rs (ViewModel gains an optional per-workspace day/week totals field populated by the app)
- src/ui/planning_board.rs or a new board-pane section (render workspace day + ISO-week totals in hours:mm)
- src/ui/layout/task_details.rs (detail view shows the subject workspace's day and week totals)
- src/app/ (assembly site populating ViewModel from time_totals plus loaded IntervalRows; rerun cadence so the day refreshes within ~1 min)

## Implementation steps

1. Discover where ViewModel is constructed (search for ViewModel usage in src/app/) and how often boards repaint; pick the lightest cadence giving sub-minute freshness (refresh on repaint tick or 60 s).
2. At the assembly point, load ledger rows via task 1's loader and call time_totals::day_totals/week_totals; fold a workspace-keyed day+week seconds pair into ViewModel as a new optional field (None when no ledger, so empty projects are unchanged).
3. Render a compact section on the board listing each workspace with Day h:mm and Week h:mm; render the subject workspace's totals in the task detail pane. Label totals as agent-active time only.
4. Format seconds as hh:mm with no further precision; hide the section entirely when totals are empty/absent rather than drawing zeroes everywhere.
5. Keep the pane purely presentational: no new typed commands dispatched, no writes, so R6 and existing board interactions are untouched.

## Acceptance criteria

- With accrued interval rows present, the board shows the connected workspace's current-day and current-ISO-week totals matching task 3's derivation exactly.
- The in-progress day total increases within one repaint cycle (~60 s) as active work continues; no restart needed.
- All pre-existing board behaviors (item lists, archival, detail navigation) behave identically; with no ledger the board renders as before.
- Displaying totals triggers no disk writes and no new commands to the app dispatcher.

## Test plan

1. Unit/integration: feed synthetic IntervalRows covering today and this ISO week; assemble ViewModel; assert the totals field equals time_totals output.
2. Empty case: no ledger rows/ledger absent; assert ViewModel field is None and rendering skips the section without panic.
3. Determinism: assembling twice from the same rows yields identical display strings.
4. Manual runtime check: with a running accrual, confirm the day figure ticks upward within ~60 s on the board and in task detail.

## Verification commands and expected evidence

1. cargo test -p packet (expect all pass)
2. Run the binary, observe board and task detail totals updating within ~60 s during active work; cargo clippy -p packet with zero new warnings vs baseline

## Edge cases and failure handling

- A project with no ledger yet must not crash or draw a misleading zero-filled panel.
- Minute formatting rounds display without implying finer accuracy than the minute-level accrual (AC1).

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

- Board and detail surfaces render per-workspace day and ISO-week totals sourced solely from time_totals; tests pass; clippy clean vs baseline; existing board flows unaffected.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
