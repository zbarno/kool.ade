## 13. Source Map

| Evidence | Location |
| --- | --- |
| Current authority and feature deltas | `planning/product/index.md`, `planning/product/*.md`, `planning/features/CHG-001-scalable-planning-and-feature-specification/specification.md`, `planning/features/CHG-002-board-card-display-red-line-activity-graph-and-class-col/specification.md`, `planning/features/CHG-003-readable-chat-replies-formatted-markdown-at-a-glance-ask/specification.md`, `docs/living-specification-policy.md` |
| Legacy authority and historical decisions | `.kool-ade-packet/planning/archive/specification-pre-modules.md`, git history, root `SPECIFICATION.md` (D-17 superseded by D-28) |
| Product modules, IDs, transaction and items | `src/artifacts/product_docs.rs`, `transaction.rs`, `items_io.rs`, `task_docs.rs`; `src/core/validation.rs`, `apply.rs` |
| Context, routing and agent investigation | `src/core/context_build.rs`, `prompt.rs`, `repo_overview.rs`, `routing.rs`, `investigation.rs`, `src/domain/item.rs` (D-14, FR-13) |
| Project repositories, task batches, implementation and reconciliation | `src/core/project_repos.rs`, `contract_snapshot.rs`, `workflow.rs`, `task_generation.rs`, `implementation.rs`, `implementation_queue.rs`, `reconciliation.rs` (CLR-019 resolved by D-30) |
| Desktop board, ownership and specification interaction | `src/ui/layout.rs`, `spec_viewer.rs`, `task_activity.rs`, `theme.rs`, `task_chat.rs`, `chat_pane.rs`, `message_text.rs`; `src/app/root.rs`, `dialogs.rs`, `manager.rs` (F-16–F-20; D-26/D-27 under feature CHG-002; CHG-003 chat-display work pending) |
| Harness boundary and local state | `src/harness/`, `src/persistence/`, `.planner/config.md`, `.planner/project.json` where configured (D-13, D-15, D-16, D-23, D-25) |
| Regression and demonstration evidence | `tests/multi_repository_feature.rs`, `tests/task_workflow.rs`, unit tests in the modules above, `examples/prepare_exit_demo.rs`, `docs/exit-demo-runbook.md` (D-20, D-21) |

Git commits from `f72e3ae` through the CHG-001 reconciliation checkpoint and `7f7831e`/`6b851d3`/`beb998d` through the CHG-002 reconciliation checkpoint record the implementation. Deferred-channel decisions D-18, D-22, D-32 and D-33 remain indexed by the decisions and risks modules.
