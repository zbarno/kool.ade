# F9: Relabel product index to the canonical name
<!-- koolade-artifact-id:v1 {"uid":"6a552dd8-caee-40e5-8ef3-92ae855394ee","displayId":"F9","title":"Relabel product index to the canonical name"} -->
<!-- koolade-change:v1 {"schemaVersion":1,"uid":"6a552dd8-caee-40e5-8ef3-92ae855394ee","displayId":"F9","status":"implemented"} -->

**Status:** Implemented — recorded here as the task-generation copy; the operator applied the index edit on 2026-10-04 and the stickiness check passed.

## Intent

Bring the front door of the living specification in line with the canonical project name (Kool.ad/e) adopted across the spec body (CLR-001, F8): replace the legacy Packet-branded index H1, update the stale bootstrap intro, and verify the change persists across future app-applied regenerations. First filed as future-scheduled work in the CLR-006 conversation; the operator later directed immediate execution.

## Current Behavior

- At filing, the index H1 used the legacy Packet-branded title while all six refreshed modules treated Kool.ad/e as canonical (F8).
- The index intro is bootstrap-era boilerplate (inviting the reader to describe the project in chat to begin) that predates the populated, modularized specification.
- The app-applied index refresher (`src/artifacts/product_docs/feature_index.rs`, fn `refreshed_index_from`) copies the existing H1 and intro verbatim on every change set and falls back to a generic `Product — Living Technical Specification` only when no H1 is present (established in F8, CLR-005). The stale title therefore never self-heals — symmetrically, a once-edited H1 sticks.
- Neither planner turns nor app-applied module updates can rewrite `product:index`; the change requires a direct operator edit or an implementation task under this approved feature.

## Desired Behavior

- The index H1 reads `Kool.ad/e — Living Technical Specification`.
- The intro is replaced with a concise orientation matching the manifest-order module list, with no bootstrap-era placeholder.
- A subsequent app-applied index regeneration keeps the new H1 and intro verbatim (stickiness verified).
- The application path (operator edit or worker task) and its date are recorded in this feature.

## Scope

- Included: the index H1 and intro rewrite; stickiness verification; recording the outcome.
- Excluded: module-body relabeling (completed in F8); the earlier `Packet` to `koolade` repository rename proposal, superseded by the public-launch decision to retain `zbarno/kool.ade`; any change to module order, manifest registration, or other index content.

## Affected Product Areas

- The `product:index` front page only (app/operator-maintained file); no module bodies are touched.

## Requirements

1. R1 — The H1 reads exactly `Kool.ad/e — Living Technical Specification`.
2. R2 — The bootstrap-era placeholder intro is gone, replaced by orientation consistent with the manifest-order module list.
3. R3 — Stickiness verified: a subsequent app-applied regeneration preserves the new H1 and intro verbatim under the copy-existing behavior of `refreshed_index_from`.
4. R4 — Closure records the application path (operator edit versus worker task) and date, tied to the CLR-006 outcome.
5. R5 — Module order, manifest registration, and all other index content are unchanged in substance.

## Decisions and Assumptions

- Timing: operator-directed in the CLR-006 conversation — first future-scheduled, then immediate execution; this feature proceeds upon the standard approval of this document, and approval alone does not start a worker.
- Mechanism (grounded in F8/CLR-005): the index H1 is frozen bootstrap text for planner and app-applied module turns; a single authorized rewrite suffices and persists, because every regeneration copies the existing H1 and intro verbatim.
- Application path follows the operator's choice in this conversation: approval of this feature's card routes the change through implementation and reconciliation; alternatively the operator hand-applies the two-line change directly, in which case this feature is abandoned as moot. Whichever lands, this feature (or its abandonment record) preserves the audit trail.
- Name: the H1 uses the canonical project name Kool.ad/e (CLR-001); the current hosting repository is `zbarno/kool.ade`.

## Acceptance Criteria

- AC1 — `product/index.md` H1 reads `Kool.ad/e — Living Technical Specification`.
- AC2 — No bootstrap-era placeholder remains in the index intro; the orientation matches the module manifest.
- AC3 — A subsequent app-applied regeneration demonstrably preserves the H1 and intro; any surprising revert surfaces as a board item, not silent reversion.
- AC4 — The closure note records who applied the change (operator or worker), when, and how stickiness was confirmed.
