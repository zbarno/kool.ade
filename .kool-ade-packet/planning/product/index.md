# Packet — Living Technical Specification

**Version:** 2.0

**Status:** Current product truth; CHG-001 reconciliation

**Authority:** These modules govern current product behavior. Approved feature deltas govern proposed changes until merged-code reconciliation.

**Origin:** Migrated from `planning/specification.md`; the prior file is archived in git.
**Latest revision:** Added modular feature planning, scoped contexts, authority-aware work, multi-repository tasks, approval gating, and reconciliation. The existing MVP acceptance bar remains D-20/D-21.

**Maintenance:** Packet's validated planner updates only affected modules as complete replacements and checkpoints them in git. The desktop specification view is read-only; direct repository edits are possible but should be reviewed against stable IDs, decisions, and evidence. Git preserves revision history.

Packet is a git-backed desktop planning partner. The index orients an agent; individual modules supply the current product truth. Active feature documents describe pending deltas.

## Product modules

- [1. Vision](01-vision.md) — required: Overview
- [2. Scope](02-scope.md)
- [3. Actors and Roles](03-actors-and-roles.md) — required: Users and Outcomes
- [4. Feature Inventory](04-feature-inventory.md)
- [5. Functional Requirements](05-functional-requirements.md) — required: Current Capabilities
- [6. Non-Functional Requirements](06-non-functional-requirements.md) — required: Quality and Acceptance
- [7. Data Model](07-data-model.md)
- [8. Architecture](08-architecture.md) — required: Architecture and Constraints
- [9. Environment, Launch, and Preconditions](09-environment.md)
- [10. Decisions Log](10-decisions.md) — required: Decisions
- [11. Risks and Open Concerns](11-risks.md)
- [12. Acceptance / Definition of Done](12-acceptance.md)
- [13. Source Map](13-source-map.md)

## Active features

- [`CHG-003-readable-chat-replies-formatted-markdown-at-a-glance-ask`](../changes/CHG-003-readable-chat-replies-formatted-markdown-at-a-glance-ask/specification.md)
- [`CHG-004-post-publication-worktree-cleanup`](../changes/CHG-004-post-publication-worktree-cleanup/specification.md)
- [`CHG-005-editable-operator-persona`](../changes/CHG-005-editable-operator-persona/specification.md)
- [`CHG-006-packet-self-review-and-enhancement-pipeline-log-mining-g`](../changes/CHG-006-packet-self-review-and-enhancement-pipeline-log-mining-g/specification.md)
- [`F7-client-billing-time-tracking`](../changes/F7-client-billing-time-tracking/specification.md)
