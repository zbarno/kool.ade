# Packet Hardening Follow-Up Remediation

**Status:** In progress  
**Current phase:** P1 — dogfood the canonical `.kool-ade-packet` layout  
**Last checked:** 2026-09-26  
**Source plan:** [Full user-provided requirements](source-plan.md)  
**Current code baseline:** `44e06060e8871f4ab6d802bdc19ba38345375049` (`master`, `origin/master`)

This is the durable ledger for the nine-phase follow-up plan. The source plan is preserved verbatim; its acceptance criteria, test matrix, required quality gates, and completion definition remain binding. A phase stays incomplete until its criteria have current, scope-matched evidence.

## Task status

| ID | Task | Status | Current evidence / next action |
| --- | --- | --- | --- |
| P1 | [Dogfood the canonical `.kool-ade-packet` layout](source-plan.md#5-phase-1-dogfood-the-canonical-kool-ade-packet-layout) | In progress | Inventory/classification completed below. Production migration, identity/content audit, cleanup verification, and repository-state regression coverage remain. |
| P2 | [Structure change lifecycle metadata](source-plan.md#6-phase-2-structured-change-lifecycle-metadata) | Not started | After P1, locate all production status-text decisions and design typed persisted status. |
| P3 | [Sandbox planning and read-only model operations](source-plan.md#7-phase-3-sandbox-planning-and-read-only-model-operations) | Not started | After P2, adapt the execution boundary for bounded planning reads and prove it with sentinel security tests. |
| P4 | [Strengthen Auto Publish policy](source-plan.md#8-phase-4-strengthen-auto-publish-policy) | Not started | After P3, make independent checks mandatory for unattended publication and verify the exact-commit gate. |
| P5 | [Continue decomposing `PacketApp`](source-plan.md#9-phase-5-continue-decomposing-packetapp) | Not started | After P4, extract a cohesive remaining lifecycle from `root.rs` and preserve sequencing/cancellation tests. |
| P6 | [Narrow the UI read boundary](source-plan.md#10-phase-6-narrow-the-ui-read-boundary) | Not started | After P5, move the most complex remaining pane to a cohesive read model without adding `PacketApp` dependencies. |
| P7 | [Make platform capabilities explicit](source-plan.md#11-phase-7-make-platform-capabilities-explicit) | Not started | After P6, verify Linux/macOS/Windows capability detection and fail-closed behavior. |
| P8 | [Run end-to-end dogfood validation](source-plan.md#12-phase-8-end-to-end-dogfood-validation) | Not started | After P1–P7, execute all eight stated product/security/recovery scenarios and record direct evidence. |
| P9 | [Complete documentation and compatibility cleanup](source-plan.md#13-phase-9-documentation-and-compatibility-cleanup) | Not started | After P1–P8, update docs from verified behavior and remove obsolete compatibility code only when no longer needed. |

## P1 work items

| Work item | Status | Evidence / next action |
| --- | --- | --- |
| Inventory and classify every tracked legacy artifact | Complete | `git ls-files planning .planner adr SPECIFICATION.md` lists 25 files: 14 `planning/product` documents, 6 feature specifications, `planning/open-items.md`, `planning/resolved-items.json`, `.planner/config.md`, `.planner/workflow.json`, and root `SPECIFICATION.md`. The product modules, active changes, open/resolved items, and legacy config/workflow are migration inputs. `adr/` has no tracked files. Root `SPECIFICATION.md` is historical input and belongs in the canonical archive, consistent with `docs/artifact-layout.md`. |
| Run the production migration against this repository | Not started | Use `artifacts::migration::run` through Packet's connection path after confirming migration preflight has no conflicts. |
| Audit preservation of stable IDs and all private task/workflow associations | Not started | Compare before/after feature, task, open-item, decision, implementation, conversation, queue, and approval identities. |
| Compare canonical meaning with legacy product, changes, items, decisions, approvals, and work | Not started | Verify content and state, not only moved paths. |
| Remove obsolete live-looking legacy copies | Not started | Migration should relocate them; confirm none remain after validated checkpoint. |
| Decide and complete root `SPECIFICATION.md` treatment | Not started | Confirm archived purpose and runtime references after migration; it is currently 1,165 lines and tracked. |
| Add repository-state regression coverage and perform a real read-only audit | Not started | Assert no live runtime-owned project truth remains outside `.kool-ade-packet/`; distinguish migration, tests, history, and maintainer docs. |

## Current repository evidence

- The worktree was clean on `master`, aligned with `origin/master`, at `44e0606` before this ledger was added.
- The canonical root currently contains implementation evidence, task batches, an archive, and `planning/work.json`, but has no manifest, shared config, current product modules, change specs, decision records, open/resolved items, or canonical `state/` directory.
- The legacy project tree is still present. Packet's production connection path calls the restartable `artifacts::migration::run` before loading `PlannerState`; migration preflights planned source/destination bytes before it writes its pending journal or applies moves.
- Existing canonical task batches and ignored implementation evidence must be retained. The migration path updates task identities and state associations; verify the real records after it runs.

## Continuation guidance

- Work in phase order from the source plan. Keep each phase's status and evidence current in this file before moving to the next phase.
- Do not mark a phase complete from a build, code inspection, or fixture test when its acceptance criteria require real repository state, UI interaction, security enforcement, external CI, or other direct evidence.
- Run the required final quality gates from source-plan section 14 after implementation and retain per-scenario results for phases 8 and 9.
- If interrupted, continue with the first incomplete P1 work item above; do not rerun destructive migration if its authoritative marker/checkpoint shows it already completed.
