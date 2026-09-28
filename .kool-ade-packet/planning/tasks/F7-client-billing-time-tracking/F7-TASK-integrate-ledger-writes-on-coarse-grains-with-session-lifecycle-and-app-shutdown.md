---
packet-task: {"schemaVersion":1,"uid":"35d88288-7a47-498b-aba9-b877dbec6dd8","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":["b12e64c3-317d-41f5-a565-99e539ccd400","cc582e2e-e762-45cc-bc5c-87b811421c41"]}
---

# F7-TASK-integrate-ledger-writes-on-coarse-grains-with-session-lifecycle-and-app-shutdown — Integrate ledger writes on coarse grains with session lifecycle and app shutdown
<!-- packet-artifact-id:v1 {"uid":"35d88288-7a47-498b-aba9-b877dbec6dd8","displayId":"F7-TASK-integrate-ledger-writes-on-coarse-grains-with-session-lifecycle-and-app-shutdown","title":"Integrate ledger writes on coarse grains with session lifecycle and app shutdown","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Session and shutdown paths lack coarse-grained time-ledger flushing, risking loss or duplication of active agent intervals.

## Ticket goal — what changes when done

Lifecycle events atomically persist active intervals and perform coarse ledger commits only when necessary.

## User story

As a seated operator I want my accumulated time persisted at predictable lifecycle moments so a restart cannot lose work.

## Purpose

Flush ledger rows and take coarse git commits at natural boundaries (session save/close, day rollover, period export, explicit flush) so NFR-2 holds without per-event flooding. Capture the in-flight interval per the R3 count/discard rule on clean shutdown, keeping the write path fully local with no network side effects.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Tasks 1 and 2 supply the ledger file, append/load logic, and accrual start/stop behavior. Production code does not flush intervals at session save, day rollover, export, or shutdown.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 1: Workspace-only attribution: the connected repository is the unit of account; workspace-to-client mapping stays external with no client entity in the app (AD-6, CLR-030)

- Scope 6: Isolation: ledger writes never disturb planning turns, queue progression, board rendering, or network activity (R6)
- Success criterion 2: Kill and restart Packet mid-interval: all completed records restore byte-identically and the interrupted interval is counted or discarded consistently - never double-counted (AC2)
- Success criterion 5: Full regression suite green with no new clippy warnings versus the recorded baseline at the pinned toolchain (AC5, D-34)

## Dependencies

- [F7-TASK-define-git-backed-time-ledger-artifact-and-atomic-row-persistence](F7-TASK-define-git-backed-time-ledger-artifact-and-atomic-row-persistence.md) must be complete.
- [F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals](F7-TASK-implement-agent-activity-observation-driving-per-workspace-accrual-intervals.md) must be complete.

## Affected files and components

- ledger persistence module from task 1
- app-layer session and shutdown coordinator

## Implementation steps

1. Find the session save/close, day rollover, period export, and clean shutdown touchpoints.
2. Flush any open ledger intervals before each transition.
3. Take a git commit only at selected coarse grains when the ledger is dirty.
4. Confirm that ledger writes stay local and do not affect planning, rendering, or networking.

## Acceptance criteria

- A clean shutdown converts the in-progress interval into one terminal row.
- Dirty-free transitions create no unnecessary ledger commits.

## Test plan

1. Simulate an open worker interval followed by clean shutdown and assert one terminal row appears.
2. Repeat a transition with no dirty changes and assert no extra ledger commit occurs.

## Verification commands and expected evidence

1. Run the normal Cargo test suite and a focused shutdown simulation

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

- Flushing is invoked at the selected lifecycle boundaries and clean shutdown preserves every completed interval

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
