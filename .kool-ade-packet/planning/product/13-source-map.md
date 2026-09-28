## 13. Source Map

| Evidence | Location |
| --- | --- |
| Current product authority and proposed changes | `.kool-ade-packet/planning/product/`, `.kool-ade-packet/planning/changes/`, `docs/planner-policy.md` |
| Legacy product inputs and migration | `planning/product/`, `planning/features/`, `src/artifacts/migration/`, `src/artifacts/layout.rs` |
| Product manifests, task documents, IDs, and recoverable writes | `src/artifacts/product_docs/`, `src/artifacts/task_docs/`, `src/artifacts/atomic.rs`, `src/artifacts/transaction.rs`, `src/domain/` |
| Context selection, prompt policy, routing, and investigation | `src/core/context_build/`, `src/core/context_retrieval/`, `src/core/prompt/`, `src/core/investigation.rs`, `src/core/validation/`, `src/domain/item.rs` |
| Task generation, isolated implementation, verification, and recovery | `src/core/task_generation.rs`, `src/core/implementation/`, `src/core/implementation_queue/`, `src/harness/pi_sandbox/` |
| Harness operation modes and wire schemas | `src/harness/api.rs`, `src/harness/responses/`, `src/harness/pi_harness/` |
| Approval, publication policy, and reconciliation | `src/app/feature_approval.rs`, `src/app/root/`, `src/core/implementation/`, `src/core/reconciliation.rs` |
| Main Chat and isolated task conversations | `src/persistence/`, `src/core/task_conversation/`, `src/ui/task_chat/` |
| Board, task details, activity graph, and typed UI commands | `src/ui.rs`, `src/ui/layout/`, `src/ui/task_detail.rs`, `src/ui/task_activity.rs`, `src/app/root/ui_actions/` |
| Configuration and MCP secret handling | `src/artifacts/config_io.rs`, `src/artifacts/mcp_io.rs`, `src/core/context_build/mcp.rs`, `src/error.rs` |
| Registered repository display-name model, validation, and shared-manifest persistence | `src/core/project_repos.rs`, `src/core/project_repos/names.rs`, `src/core/project_repos/save.rs`, `src/app/dialogs/settings/repository_names.rs` |
| Repository display-name settings and Welcome/Workspace listings | `src/app/dialogs/settings/painter.rs`, `src/app/welcome/repository_picker.rs`, `src/ui/layout/workspace_repositories.rs`, `src/app/root/repository_switcher.rs` |
| Repository display-name validation, duplicate fallback, settings clear/save/reload, and checkout routing regressions | `src/core/project_repos/names.rs`, `src/core/project_repos/save.rs`, `src/app/dialogs/settings/repository_names/tests.rs`, `src/app/welcome/repository_picker/tests.rs`, `src/app/root/repository_switcher/tests.rs` |
| F-26 typed comparison records and backward-compatible workflow loading | `src/core/workflow.rs`, `src/core/workflow/comparison_record.rs`, `src/artifacts/task_docs/workflow.rs` |
| F-26 comparison envelope validation, contrast checks, and planner prompt | `src/harness/responses.rs`, `src/core/validation/plan_comparison.rs`, `src/core/prompt/compare_plans.rs` |
| F-26 ComparePlans turn dispatch and typed proposal persistence | `src/core/turn.rs`, `src/core/turn/execute.rs`, `src/core/apply/comparison.rs` |
| F-26 plan adoption, ADR creation, approval invalidation, and selected-plan fingerprint | `src/app/feature_approval/selection/`, `src/domain/change_status/metadata/typed_adoption.rs`, `src/core/contract_snapshot.rs` |
| F-26 approval gate, comparison view, restart, failure, and adoption regressions | `src/app/feature_approval.rs`, `src/ui/feature_approval.rs`, `src/app/root/feature_approval/tests/` |
| Regression and demonstration evidence | `tests/`, module-local unit tests, `examples/`, `docs/exit-demo-runbook.md` |

Legacy paths listed here are migration inputs, not active runtime locations.
Decision and task records retain their own immutable content; source references
inside frozen feature contracts and historical evidence describe the revision
where they were written. Deferred collaboration decisions D-18, D-22, D-32,
and D-33 remain indexed in the decisions and risks modules.
