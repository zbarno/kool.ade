---
packet-task: {"schemaVersion":1,"uid":"b12e64c3-317d-41f5-a565-99e539ccd400","batchUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804","repositoryId":"root","dependencyUids":[]}
---

# F7-TASK-define-git-backed-time-ledger-artifact-and-atomic-row-persistence — Define git-backed time ledger artifact and atomic row persistence
<!-- packet-artifact-id:v1 {"uid":"b12e64c3-317d-41f5-a565-99e539ccd400","displayId":"F7-TASK-define-git-backed-time-ledger-artifact-and-atomic-row-persistence","title":"Define git-backed time ledger artifact and atomic row persistence","parentUid":"a6ebe7eb-0524-4e80-b3aa-85f092cf2804"} -->

Feature: Client Billing Time Tracking (F7)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Packet records no work hours anywhere: billable time lives in memory or rebuilt spreadsheets, so invoices ship late and no durably storable, restorable interval record exists (brief problem). Without a dedicated git-backed ledger slot, per-workspace totals cannot survive restarts (AC2) nor regenerate monthly reports (AC3).

## Ticket goal — what changes when done

The root repo gains a time-ledger module with a fixed ledger file under .kool-ade-packet/ carrying a header line and validated interval rows (stable repo/workspace id, start/end epoch seconds, worker/session identity, item uid, optional Feature: ref, end_status). Rows append atomically (existing atomic_write_bytes, temp+rename+dirsync); load restores completed rows byte-identically and handles an incomplete trailing row deterministically.

## User story

As the operator-biller I want each counted agent-work interval durably persisted per workspace so my time survives a killed Packet and later exports, enabling exact monthly invoice hand-off.

## Purpose

Stand up the dedicated ledger slot under .kool-ade-packet/, defining the interval row schema (workspace id, start/end instants, worker/session identity, item/feature ref) with atomic file writes (temp+rename+dirsync) and a coarse git-commit trigger set. Define load/restore so completed rows survive kill-mid-interval byte-identically with interrupted rows counted-or-discarded consistently.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Module src/artifacts/layout.rs (verified) defines canonical paths (.kool-ade-packet/*) with ROOT, PLANNING, etc.; no ledger constant exists yet. src/artifacts/atomic.rs (verified) provides atomic_write_bytes (write-temp, sync_all, rename, dirsync) and sibling-temp cleanup; src/artifacts.rs re-exports it plus a mod tree (spec_doc, items_io, config_io, ...). Row format, ledger type, path constant, and commit triggers are all newly introduced by this story. Discovery: locate the existing scoped-git-checkpoint helper used by FR-3 so the coarse-commits reuse it verbatim.

Feature ID: F7
Repository: root

## Approved scope mapping

- Scope 3: A persistent git-backed time ledger in a dedicated artifact slot with atomic writes and coarse commit grain, restoring identically across restarts (AD-3, R2)
- Success criterion 2: Kill and restart Packet mid-interval: all completed records restore byte-identically and the interrupted interval is counted or discarded consistently - never double-counted (AC2)

## Dependencies

None. This task can start independently.

## Affected files and components

- src/artifacts/time_ledger.rs (new: ledger slot path, IntervalRow parse/serialize, atomic append-row, load/restore, incomplete-row handling)
- src/artifacts/layout.rs (add LEDGER constant, e.g. .kool-ade-packet/state/time-ledger.log, mirroring existing style)
- src/artifacts.rs (register pub mod time_ledger)

## Implementation steps

1. Discovery: identify the FR-3 scoped git-checkpoint helper (likely under src/core/transaction or src/git) to reuse; record its signature.
2. Add LEDGER path constant to layout::canonical alongside existing entries, mirroring naming style.
3. Create src/artifacts/time_ledger.rs: define IntervalRow {repo_id, workspace_id, session_id, worker_pid?, start_epoch_s, end_epoch_s?, item_uid, feature_ref: Option<&str>, end_status: Ended|InterruptedDiscard|InterruptedCount}.
4. Serialize rows as one UTF-8 line, colon/comma-separated with stable field order, terminated \n; validate parse strictly and reject unknown end_status or malformed epochs.
5. Expose append_row(row) calling atomic_write_bytes on the full rewritten file (header + all rows + new row), ensuring parent creation and dirsync from existing primitive.
6. Implement load(repo_root) -> Vec<IntervalRow>: read file if present, tolerate missing file, parse all complete rows, detect a torn/incomplete final line (missing newline or invalid terminator) and classify it per the count-vs-discard rule documented in a doc comment.
7. Register pub mod time_ledger in src/artifacts.rs and re-export the public items.
8. Add a coarse-commits note (doc comment or const) listing candidate trigger grains (session save/close, day rollover, month export, explicit flush) deferring wiring to task 6; do not invoke git here.
9. Ensure no clippy warnings vs baseline; do not alter planning-turn checkpointing.

## Acceptance criteria

- Writing five rows sequentially and reloading them yields byte-identical parsed rows with stable ordering.
- Killing between write_all and rename on the final row results in either the pre-crash file intact or the new row fully present on next load - never a partial row; reload produces the same Vec regardless of which branch occurred.
- A deliberately truncated last line parses as InterruptedDiscard and is excluded from any downstream summation (this story exposes classification only; consumers come later).
- New module introduces no warning against the recorded clippy baseline at the pinned toolchain.

## Test plan

1. Unit: round-trip serialize->parse for each end_status variant asserts field equality and canonical bytes.
2. Integration: append three rows via append_row, remove the file, append again - ensures create_dir_all path and header line restored.
3. Fault injection: build a temp-copy of the ledger with the final line cut mid-field; assert load returns the complete rows plus one InterruptedDiscard entry; assert a separately-written valid file returns an equivalent Vec modulo that one synthetic discard.
4. Clippy: cargo clippy --workspace at pinned toolchain shows zero new warnings vs baseline recorded at b257a15a.
5. Regression: existing crate tests (cargo test) remain green; no FR-3 checkpoint behavior altered (assert via existing checkpoint tests).

## Verification commands and expected evidence

1. cargo build -p packet --lib
2. cargo test --lib artifacts::time_ledger
3. cargo clippy --workspace -- -D warnings (compared visually against the recorded pre-task baseline; expect zero NEW warnings, no new categories)

## Edge cases and failure handling

- Missing ledger file on fresh repo - load returns empty vec, does not create file until first append.
- Concurrent writers are not anticipated in v1 (single process); if one occurs, atomic_write_bytes rename replaces the losing writer's snapshot - acceptable given AD-3 and single-seated operator assumption (AD-1).
- Epoch timestamps stored as integer seconds; sub-second resolution intentionally omitted to keep the ASCII row compact and align with AC1's one-minute rounding tolerance.

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

## Rollout and compatibility

Purely additive module with no runtime callers yet; safe to merge behind the existing feature-gate for F7 if present, otherwise inert until task 2 wires the accrual engine.

## Definition of done

- time_ledger.rs compiled and wired into src/artifacts.rs; LEDGER path declared in layout::canonical; IntervalRow and load/append_row publicly exposed; fault-injection tests pass demonstrating torn-row tolerance; clippy diff against pinned-toolchain baseline is empty; no changes to existing checkpoint or planning flows.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
