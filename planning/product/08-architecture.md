## 8. Architecture

Current layers: `ui → app → core → harness`, with `domain`, `artifacts`, and `persistence` as shared boundaries. `ui` renders chat, the document switcher, paper specification, Kanban, details, and activity. `app` owns session controllers and queue/worker orchestration. `core` selects context, validates envelopes, applies transactions, routes authority, generates tasks, implements them in worktrees, and reconciles merged code. `harness` starts and supervises external Pi; it does not own project truth.

The Context Builder in `src/core/context_build.rs` starts with standing policy and small project orientation, then active feature/items/recent chat, then deterministic product-module references and on-demand repository evidence. It clips bounded sections and does not load completed feature history by default. Planning, task generation, implementation, investigation, and reconciliation have distinct prompts/contracts (F-5, F-16, F-18, F-22).

Primary planning flow:

1. Load current git-backed product modules, active feature, open items, ownership, and repository manifest.
2. Compile bounded activity-specific context; Pi may inspect additional evidence read-only.
3. Parse the schema-v2 envelope; validate logical document IDs, stable identifiers, item authority, routing, approval, and scope.
4. Journal and apply the complete changed set; checkpoint only touched planning paths. Recover a leftover journal at connection.
5. Project validated items/tasks onto the board and issue at most one eligible blocking Human question.

Task generation freezes an approved feature, affected modules, repository heads and configuration. Implementation fetches the current target branch, runs one task in an isolated worktree, verifies, then creates a PR or auto-integrates according to queue mode. A dependent task receives the completed predecessor's story and merged commit. Reconciliation reads merged commits, updates only affected product modules, and marks the feature Implemented; disagreement yields a board review item. `core/implementation.rs` is resumable and handles lost final harness messages (D-29).

Architectural invariants: git artifacts outrank model recollection; board state is a projection; target checkouts are verified by logical repository identity; no atomic transaction is promised across repositories; product truth is not advanced before merged-code reconciliation. D-13 harness discovery and D-14 routing remain active. F-19/F-20 are pending UI deltas. The WebSocket channel remains reserved future architecture (D-18, D-22); D-31 bounds its eventual wire payload to planning artifacts and presence, never chat. D-32 rules the channel's shape: direct instance-to-instance WebSockets (instances listen and dial peers; no shared relay), endpoints learned first by manual entry from git-remote host information, local-network reach for the first cut, and peer qualification grounded in repository access proved at connect time. CLR-016's writer model and the concrete connect-time proof mechanic remain its open design gates.
