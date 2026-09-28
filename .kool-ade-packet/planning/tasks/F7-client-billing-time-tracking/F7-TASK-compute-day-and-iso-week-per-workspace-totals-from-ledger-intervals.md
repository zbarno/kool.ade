---
packet-task: {"schemaVersion":1,"uid":"f7f19f0b-d39b-45fa-9d00-89bbb7e00bbd","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":["cc582e2e-e762-45cc-bc5c-87b811421c41"]}
---

# F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals — Compute day and ISO-week per-workspace totals from ledger intervals
<!-- packet-artifact-id:v1 {"uid":"f7f19f0b-d39b-45fa-9d00-89bbb7e00bbd","displayId":"F7-TASK-compute-day-and-iso-week-per-workspace-totals-from-ledger-intervals","title":"Compute day and ISO-week per-workspace totals from ledger intervals","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Tasks 1 and 2 persist and accrue raw interval rows, but nothing derives period totals: the board shows no per-workspace day/week figures, and export (task 5) has no total source. Without a single read-time derivation, display and export would compute totals differently, breaking the exported-equals-displayed-equals-summed invariant.

## Ticket goal — what changes when done

A new pure-core module exposes per-workspace day and ISO-week totals computed solely from ledger IntervalRows (loaded via task 1's loader), including the in-progress day, such that callers can obtain board-ready figures with no writes and no mutation of ledger state.

## User story

As the seated operator-biller I want trustworthy per-workspace day and ISO-week hour totals derived straight from the recorded intervals so the board and the month report agree with each other.

## Purpose

Provide read-time derivation of per-workspace daily and ISO-8601 weekly totals from raw interval rows, including the in-progress day, on device-local dates with no DST correction. Back both board display and later export so the exported/displayed/summed invariant holds from one source.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Repo is one crate (Cargo.toml: package packet, edition 2024); lib.rs exposes mods app/artifacts/core/domain/harness/persistence. chrono 0.4.45 is a declared dependency, so device-local civil-date and ISO-8601 week math are available without new deps. Verified: no ledger or accrual code exists yet; task 1 defines IntervalRow (start_epoch_s, end_epoch_s, workspace_id, session_id, end_status) with loader, task 2 emits rows. Confirm the exact module path and Row type once landed; discovery belongs in step 1. Weeks are ISO-8601 Monday-Sunday (AD-8); dates are device-local with no DST correction (AD-2); day granularity is minute-integer seconds.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 4: Per-workspace day and ISO-week totals on existing board surfaces, including the day in progress (R4, AD-8)
- Success criterion 1: Minutes accruing while an agent actively works inside workspace W appear in W's day total within at most one minute of rounding; minutes accruing with no workspace-attached agent active appear in no workspace total (AC1)
- Success criterion 3: A mixed week across at least two workspaces matches an independent re-sum of recorded intervals, and the month-end markdown report's per-day, per-feature lines sum exactly to the on-screen workspace totals with identical regeneration from the ledger (AC3)

## Dependencies

- [F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals](F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals.md) must be complete.

## Affected files and components

- src/core/time_totals.rs (new: pure aggregation; loads IntervalRows and returns per-workspace day and ISO-week second totals)
- src/core/mod.rs (declare pub mod time_totals alongside existing core modules)

## Implementation steps

1. Confirm task 1's ledger module path and the exact IntervalRow field names/types; adopt them verbatim.
2. Add src/core/time_totals.rs taking Vec<IntervalRow>; convert epoch instants to device-local naive datetimes (chrono Local), deriving civil date and ISO weekday with no DST adjustment.
3. Merge each workspace's overlapping/clipped intervals to disjoint spans (sort by start, sweep merging overlaps and clipping to the query day/week bounds), so intra-workspace overlap counts once.
4. Sum merged span lengths in whole seconds per workspace and per day and per ISO week (Mon-Sun); expose day_totals(now)->BTreeMap<LocalDate,BTreeMap<ws,seconds>> and week_totals(now)->BTreeMap<ISOWeek, BTreeMap<ws,seconds>>, both including the day in progress.
5. Register pub mod time_totals in src/core/mod.rs; keep the module free of IO writes and network calls so it stays a pure read over loaded rows (supports R6).

## Acceptance criteria

- A workspace with N consecutive active seconds on day D reports exactly N seconds (minute-rounded integer) in D's total; the current/in-progress day always appears, not only past days.
- Overlapping intervals in one workspace on the same day sum to the merged span length, never exceeding wall-clock seconds for that workspace/day.
- Concurrent intervals across two workspaces each report their own full span independently.
- Totals change only when underlying rows change; calling twice on identical input yields identical maps (pure, deterministically ordered).

## Test plan

1. Build a Vec of synthetic rows straddling midnight, an ISO-week boundary (Sun->Mon), and an intentional same-workspace overlap; assert day totals equal expected per-day merged spans.
2. Assert the in-progress day appears with partial seconds accumulated so far.
3. Assert the same input vector passed twice gives byte-identical serialized BTreeMaps.
4. Assert rows outside the queried day/week are excluded and never leak into neighboring buckets.

## Verification commands and expected evidence

1. cargo test -p packet core::time_totals (expect all pass)
2. cargo clippy -p packet -- -Dwarnings compared against the recorded baseline (expect zero new warnings)

## Edge cases and failure handling

- A single interval crossing two calendar days splits correctly into each day's portion rather than double-counting into one day.
- A cross-boundary interval near Sun/Mon lands in the correct ISO week; no off-by-one on the Monday anchor.

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

- Time-totals module compiles into the crate; targeted tests pass; clippy shows no new warnings versus the pinned-toolchain baseline; totals are documented as pure, derived only from IntervalRows.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
