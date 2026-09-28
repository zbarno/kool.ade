---
packet-task: {"schemaVersion":1,"uid":"223d9292-5a9a-4a79-92d6-26007acdc815","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":["f7f19f0b-d39b-45fa-9d00-89bbb7e00bbd"]}
---

# F7-TASK-generate-the-completed-month-markdown-report-from-the-ledger — Generate the completed-month markdown report from the ledger
<!-- packet-artifact-id:v1 {"uid":"223d9292-5a9a-4a79-92d6-26007acdc815","displayId":"F7-TASK-generate-the-completed-month-markdown-report-from-the-ledger","title":"Generate the completed-month markdown report from the ledger","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Days and ISO weeks are already derived by core::time_totals (task 3), and raw IntervalRows exist, but no completed-month report is produced. Until a monthly hand-off exists, the operator cannot close out a billing cycle and the exported-equals-displayed invariant (AC3) has nothing to tie to. This delivers the export half of the truthtelling rule.

## Ticket goal — what changes when done

Given a completed calendar month on device-local dates, a new core report builder emits one deterministic markdown file per month: month head with per-workspace totals, one section per day listing per-feature-name minute lines, a closing per-workspace totals table — all recomputed from IntervalRows via time_totals so it matches the board byte-for-byte.

## User story

As the seated operator-biller I want a completed month to collapse into a local markdown report split by day and feature name so I can drop it into my invoice without retabulating a spreadsheet.

## Purpose

Detect completed calendar months (1st through last day, device-local) and emit a deterministic markdown file: month head with per-workspace totals, per-day sections labeled by feature name via the ticket Feature:/item-title/General chain (AD-9), workspace qualifiers where needed, and a closing per-workspace totals table. Persist git-backed and guarantee byte-identical regeneration with totals matching the board.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Single crate (package packet, edition 2024); lib.rs exposes core/artifacts etc. chrono 0.4.45, serde/serde_json available — no new deps. Dependency task 3 built src/core/time_totals.rs over IntervalRow(start_epoch_s,end_epoch_s,workspace_id,session_id,...). Tasks 1-2 give the ledger loader and rows. Artifacts use src/artifacts/atomic.rs for temp+rename+dirsync writes. Feature labels come from the durable Feature: ticket line/item title else General (AD-9). Confirm exact module path and Row type once landed; discovery in step 1. Dates device-local, no DST (AD-2). No report code exists yet.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 5: Completed-calendar-month export as a markdown report: month head, one section per day broken down by feature name, per-workspace month totals tying to the board, regenerable from the ledger (R5, AD-7, AD-9)
- Success criterion 3: A mixed week across at least two workspaces matches an independent re-sum of recorded intervals, and the month-end markdown report's per-day, per-feature lines sum exactly to the on-screen workspace totals with identical regeneration from the ledger (AC3)

## Dependencies

- [F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals](F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals.md) must be complete.

## Affected files and components

- src/core/month_report.rs (new: pure builder producing the markdown string for a given year-month from IntervalRows via time_totals; feature-label resolution Feature: -> item title -> General)
- src/core/mod.rs (declare pub mod month_report beside time_totals)

## Implementation steps

1. Step 1 discovery: locate the ledger loader and IntervalRow type from task 1 and the exact time_totals API/functions from src/core/time_totals.rs; confirm whether rows carry an item/feature ref for labeling — if absent, flag that item-title/Feature resolution needs task 1-2 field confirmation before hardcoding.
2. Implement build_month(year: u16, month: u32, rows: &[IntervalRow]) -> String in src/core/month_report.rs. Derive the month range on device-local dates (day 1 through last day) with chrono; filter rows to the month; no DST remap (AD-2).
3. For each device-local day, cluster intervals into buckets by label via the AD-9 chain: planning Feature: name -> worked item title -> "General". Within a day/workspace, reuse time_totals' merge logic so overlap counts once; emit minutes (whole seconds/60) per label.
4. Compose the deterministic markdown: H1 month head with a per-workspace month-total line; one ## Day YYYY-MM-DD section per day containing label lines with minutes; where a label recurs across >1 workspace, add a workspace qualifier; a closing per-workspace totals table. Emit sections in chronological order and labels in sorted order so output is stable.
5. Expose a detect_completed_months helper marking a month closed only when today's date exceeds its last day (device-local); keep the builder pure (String in/out, no IO). Wire the git-backed persist + coarse-commit as task 6's seam: accept an optional output path param, but do the atomic write here via src/artifacts/atomic.rs temp+rename+dirsync, deferring the commit trigger to task 6.
6. Register pub mod month_report in src/core/mod.rs.

## Acceptance criteria

- build_month(ym, rows) returns the identical byte sequence across repeated calls on identical rows (regenerability, AC3).
- Report only produced for a completed month (today past last day); the current in-progress month yields no report.
- Per-day label lines sum to that day's on-screen per-workspace total, and the closing table equals the sum of all day sections (ties to board via time_totals).
- Same feature name in multiple workspaces appears with a workspace qualifier; the unit of account stays the workspace (AD-6/AD-9).
- Zero-new-clippy-warning build; no network calls; output deterministic regardless of insertion order of rows.

## Test plan

1. Construct synthetic IntervalRows spanning 2 workspaces across a full month incl. a cross-day and an overlapping interval; assert the rendered string is deterministic (call twice, compare bytes).
2. Independently re-sum the day's intervals per workspace and assert each day's line-column total matches the closing per-workspace table cell (mixed-workspace AC3).
3. Assert the current in-progress month is not reported, and a just-closed prior month is.
4. Assert a shared feature label appearing in two workspaces gets a workspace qualifier and no double-count within one workspace/day.

## Verification commands and expected evidence

1. cargo test -p packet core::month_report (expect all pass)
2. cargo clippy -p packet -- -Dwarnings vs recorded baseline (zero new warnings)

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

- month_report module compiles into the crate; targeted tests pass; clippy adds no warnings vs pinned baseline; builder documented as pure and fed only by time_totals/IntervalRows.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
