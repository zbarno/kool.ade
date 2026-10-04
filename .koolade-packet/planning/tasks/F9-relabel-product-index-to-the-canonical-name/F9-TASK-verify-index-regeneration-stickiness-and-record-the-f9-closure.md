---
<!-- koolade-artifact-id:v1 {"uid":"39d13f84-a9ea-4eb2-b231-7cf267f1e84e","displayId":"F9-TASK-verify-index-regeneration-stickiness-and-record-the-f9-closure","title":"Verify index-regeneration stickiness and record the F9 closure","parentUid":"006525e9-d85f-4b17-b05b-0dc1320d09d8"} -->

koolade-task: {"schemaVersion":1,"uid":"39d13f84-a9ea-4eb2-b231-7cf267f1e84e","batchUid":"006525e9-d85f-4b17-b05b-0dc1320d09d8","repositoryId":"root","dependencyUids":["14ee508f-37a5-436e-a932-4e6ec4a5d87d"]}
---

# F9-TASK-verify-index-regeneration-stickiness-and-record-the-f9-closure — Verify index-regeneration stickiness and record the F9 closure

Feature: Relabel product index to the canonical name (F9)

Status: Completed during the 2026-10-04 public-launch cleanup.

## Original problem this ticket solved and why

Task 1 rewrote the index H1/intro, but the spec requires proof the fix does not silently regress: the app-applied refresher could theoretically strip it, leaving the audit claim unsupported. This task demonstrates persistence empirically and writes the F9 closure note so the audit trail (who, when, how confirmed) exists.

## Ticket goal — what changes when done

A regenerated index snapshot built with `refreshed_index_from` preserves the new H1 and intro verbatim, and the F9 change directory records the operator-applied path, date, and stickiness evidence tied to CLR-006.

## User story

As Alex Developer I want documented proof the renamed index survives app regenerations and an auditable closure so the F9 change stays honest.

## Purpose

Confirm the new H1 and intro survive an app-applied index regeneration under `refreshed_index_from`'s copy-existing behavior, then record the F9 closure note naming the actual application path (direct operator edit during public-launch cleanup) and date, tied to the CLR-006 outcome.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context at task drafting

`refreshed_index_from` in `src/artifacts/product_docs/feature_index.rs` copies the existing H1 and intro into the regenerated index. The public-launch cleanup added a focused regression test that exercises the function against the checked-in product index and checks that its H1 and intro remain unchanged.

Feature ID: F9
Repository: root

## Approved scope mapping

- Scope 2: Verify stickiness against the copy-existing regeneration behavior of the app's index refresher (refreshed_index_from)

- Scope 3: Record application path and date in the F9 closure
- Success criterion 3: A subsequent app-applied index regeneration preserves the new H1 and intro verbatim, with stickiness confirmed (any surprising revert surfaces as a board item, not silent reversion)
- Success criterion 4: The F9 closure note records the application path (worker task under this approved feature), date, and stickiness evidence, tied to the CLR-006 outcome

## Dependencies

- [F9-TASK-rewrite-the-product-index-h1-and-intro-to-the-canonical-name](F9-TASK-rewrite-the-product-index-h1-and-intro-to-the-canonical-name.md) must be complete.

## Affected files and components

- src/artifacts/product_docs/feature_index.rs — source of `refreshed_index_from`
- src/artifacts/product_docs/index_stickiness.rs — checks the checked-in H1 and intro after regeneration
- .koolade-packet/planning/changes/F9-relabel-product-index-to-the-canonical-name/specification.md — receives the F9 closure note

## Implementation steps

1. Run the standing gates: cargo fmt --check, cargo test, cargo clippy; fix only anything this task introduced before proceeding.
2. Run the focused regression test, which regenerates the index without feature updates and checks its heading and intro against the checked-in source.
3. If either assertion fails, raise it as a board item describing observed vs expected output; do not silently resave the index or alter `refreshed_index_from`.
4. Record the operator application path, date, stickiness confirmation, and CLR-006 outcome in the F9 specification; set the change status to its terminal marker per `ChangeMetadata` conventions.

## Acceptance criteria

- Generated-from-current-index snapshot begins with '# Kool.ad/e — Living Technical Specification' and reproduces the task-1 intro intact modulo trimmed outer blanks.
- F9 change directory contains a closure note stating the worker-task application path, date, and refreshed_index_from-based stickiness evidence, referencing CLR-006.
- fmt, test, and clippy pass before the resulting commit; index.md itself is not rewritten again by this task.

## Test plan

1. Given working-tree index.md holding the new H1 and intro, invoke refreshed_index_from with a representative active update and assert the returned string starts with the exact H1 and contains the intro; expect no fallback 'Product —' title.

## Verification commands and expected evidence

1. cargo fmt --check && cargo test && cargo clippy — expect clean exit
2. Regenerate the snapshot via the added test/binary and diff its head against the H1 and intro lines of .koolade-packet/planning/product/index.md — expect zero diff except structural sections refreshed_index_from manages

## Edge cases and failure handling

- Snapshot generator must write outside .koolade-packet/planning/product/ so no accidental index mutation occurs during verification.
- Specifying an update that includes the F9 document would flip the generated 'Active features' list relative to disk; compare H1 and intro only for the stickiness assertion.

## Constraints

- The H1 must read exactly "Kool.ad/e — Living Technical Specification"; the current hosting repository is `zbarno/kool.ade`
- Approval of F9 is already current for this contract; this turn makes no specification or open-item changes
- The change was applied directly by the operator during public-launch cleanup, since neither planner turns nor app-applied module updates can rewrite product:index
- Standing quality gates (fmt, test, clippy) apply before any resulting commit per the Decisions module

## Out of scope

- Module-body relabeling (completed in F8)
- Repository renaming; the public-launch decision keeps `zbarno/kool.ade`
- Any change to module order, manifest registration, or other index content

## Definition of done

- The focused stickiness regression test passed and the F9 closure note records the completed snapshot update.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
