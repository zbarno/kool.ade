# Legacy migration test audit outside implementation

This is the wider-repository migration slice of issue #204. It covers the active artifact-root migration, planning-workspace migration, and legacy record readers outside `core::implementation`. It complements the implementation worktree audit in [the core implementation inventory](core-implementation-slowest.md).

## Measurement and production scope

Durations come from the successful full Nextest run in [PR #213 CI](https://github.com/zbarno/kool.ade/actions/runs/38801858544), source based on `main` at `39f6972`. The run passed 1,936 tests and skipped 2. The 44 cases below total 79.194 cumulative case-seconds; this is summed test duration, not additional suite wall time. All migration integration fixtures use temporary synthetic project data and local Git repositories; none requires Docker or SSH. The legacy item parser is a pure data test.

These migrations remain supported production paths: `artifacts::migration` relocates prior packet roots and state, and `core::managed_workspace::migration` previews, applies, resumes, and rolls back legacy planning checkouts. The legacy task-worktree migration is still dispatched by `core::implementation::execution::task_state` and is audited in the linked core inventory. Since these paths are active, the migration assertions are not obsolete.

## Artifact layout and state migration

| Test | Source | Time (s) | Protected invariant | Decision |
| --- | --- | ---: | --- | --- |
| `migration_moves_legacy_bytes_checkpoints_only_its_paths_and_is_idempotent` | `src/artifacts/migration/tests/files.rs` | 3.757 | Moves legacy bytes, checkpoints only migrated paths, then performs an idempotent no-op. | Keep; checkpoint scope and repeat safety. |
| `conflicting_target_aborts_the_complete_plan_before_any_move` | `src/artifacts/migration/tests/files.rs` | 0.031 | A destination conflict prevents every planned move and preserves both copies. | Keep; preflight data safety. |
| `previous_koolade_root_maps_into_the_distinct_live_root` | `src/artifacts/migration/tests/files.rs` | 0.026 | Legacy and canonical roots resolve to distinct locations. | Keep; prevents migration from aliasing source and target. |
| `restart_finishes_checkpoint_after_files_and_manifest_are_written` | `src/artifacts/migration/tests/files.rs` | 2.285 | Recovery finishes a journaled migration interrupted after data writes but before checkpoint completion. | Keep; restart point distinct from completed migration. |
| `unscoped_legacy_transaction_is_refused_as_ambiguous` | `src/artifacts/migration/tests/files.rs` | 0.081 | An ambiguous old transaction is refused without overwriting partial source state. | Keep; fail-closed recovery. |
| `migration_moves_previous_work_path_and_materializes_portable_repository_manifest` | `src/artifacts/migration/tests/canonical_state.rs` | 2.201 | Moves the prior work directory and writes a portable repository manifest. | Keep; canonical cutover outcome. |
| `conflicting_previous_work_target_stops_before_writing_the_project_manifest` | `src/artifacts/migration/tests/canonical_state.rs` | 0.160 | A work-path collision preserves source and destination and prevents manifest publication. | Keep; negative counterpart to canonical cutover. |
| `ignored_progress_is_migrated_but_excluded_from_git_checkpoint` | `src/artifacts/migration/tests/ignored_progress.rs` | 2.567 | Migrates ignored progress while excluding it from the public Git checkpoint. | Keep; private/runtime data boundary. |
| `linked_worktrees_keep_artifact_migration_state_isolated` | `src/artifacts/migration/tests.rs` | 0.147 | Two linked worktrees do not share migration transaction state. | Keep; Git worktree isolation. |

## Identity, status, and task-state migration

| Test | Source | Time (s) | Protected invariant | Decision |
| --- | --- | ---: | --- | --- |
| `unchanged_legacy_workflow_is_normalized_and_checkpointed` | `src/artifacts/migration/tests/identities.rs` | 1.730 | Normalizes old workflow data, checkpoints identity records, preserves source bytes, and is idempotent. | Keep; primary workflow migration proof. |
| `story_only_batch_gets_durable_batch_identity_and_workflow_entry` | `src/artifacts/migration/tests/identities.rs` | 2.181 | Creates stable batch/task identity and a workflow entry when only stories exist. | Keep; story-only schema shape. |
| `duplicate_uids_abort_identity_migration_before_writes` | `src/artifacts/migration/tests/identities.rs` | 0.141 | Duplicate feature/task IDs fail before any file mutation. | Keep; identity collision fail-closed case. |
| `schema_three_seeds_stable_ids_and_relationships_idempotently` | `src/artifacts/migration/tests/identities/schema_three.rs` | 3.023 | Seeds stable identity and relationships from schema three and safely repeats. | Keep; older schema compatibility. |
| `legacy_implementing_status_migrates_to_typed_metadata` | `src/artifacts/migration/tests/status.rs` | 1.739 | Converts a legacy status while retaining the record identity and display ID. | Keep; status compatibility. |
| `unknown_legacy_status_fails_before_mutating_the_change` | `src/artifacts/migration/tests/status.rs` | 0.087 | Unknown status blocks migration without changing the source record. | Keep; malformed input safety. |
| `task_move_preserves_queue_workflow_approvals_and_implementation_evidence` | `src/artifacts/migration/tests/task_state.rs` | 2.911 | Moves a task while preserving queue, approval, workflow, and implementation state; rerun is idempotent. | Keep; cross-artifact task-state preservation. |
| `evidence_only_destination_merges_restartably_and_keeps_older_activity` | `src/artifacts/migration/tests/implementation_merge.rs` | 2.287 | Merges evidence into a destination lacking state, retains prior activity, and resumes safely. | Keep; evidence-only merge path. |
| `conflicting_same_name_implementation_evidence_stops_before_mutation` | `src/artifacts/migration/tests/implementation_merge.rs` | 0.113 | Conflicting same-name evidence blocks mutation and preserves the source. | Keep; negative counterpart to evidence merge. |

## Managed planning-workspace migration

| Test | Source | Time (s) | Protected invariant | Decision |
| --- | --- | ---: | --- | --- |
| `migration_preview_reports_stable_ids_and_keeps_legacy_source_unchanged` | `src/core/managed_workspace/migration/tests/preview.rs` | 0.727 | Preview reports pinned source, IDs, and backup while leaving legacy files unchanged. | Keep; explicit review-before-cutover contract. |
| `migration_confirmation_rejects_source_changes_after_preview` | `src/core/managed_workspace/migration/tests/preview.rs` | 1.154 | Confirmation rejects source changes made after preview and preserves them. | Keep; stale-preview protection. |
| `interrupted_migrations_resume_with_the_journaled_workspace_id` | `src/core/managed_workspace/migration/tests/recovery.rs` | 17.316 | Recovery after injected interruption points reuses the journaled workspace identity. | Keep; longest case, but unique multi-point crash recovery. |
| `generated_workspace_uuid_is_journaled_before_staging_and_reused` | `src/core/managed_workspace/migration/tests/recovery.rs` | 3.780 | Persists a generated UUID before staging and reuses it after interruption. | Keep; identity durability at the earliest recovery boundary. |
| `recovery_refuses_to_associate_a_modified_renamed_repository` | `src/core/managed_workspace/migration/tests/recovery.rs` | 3.738 | Modified or renamed source is not silently associated during recovery. | Keep; source identity safety. |
| `migration_preserves_shared_state_and_keeps_private_data_out_of_git` | `src/core/managed_workspace/migration/tests/preserves.rs` | 3.867 | Preserves task, feature, workflow, approval, item, and report state while excluding private data from Git; rollback remains possible. | Keep; broad preservation E2E. |
| `migration_reuses_existing_workspace_association_without_allocating_a_second_id` | `src/core/managed_workspace/migration/tests/preservation_metadata.rs` | 3.238 | Reuses an existing association and workspace identity. | Keep; duplicate-identity prevention. |
| `legacy_manifest_workspace_id_and_project_id_must_agree` | `src/core/managed_workspace/migration/tests/preservation_metadata.rs` | 0.584 | Rejects conflicting persisted workspace and project identities. | Keep; metadata consistency guard. |
| `unsupported_legacy_manifest_schema_stops_before_cutover` | `src/core/managed_workspace/migration/tests/preservation_metadata.rs` | 0.540 | Unsupported schema cannot cross the cutover boundary. | Keep; compatibility fail-closed case. |
| `rollback_warns_when_the_managed_planning_repo_has_new_changes` | `src/core/managed_workspace/migration/tests/preservation_metadata.rs` | 3.443 | Rollback does not discard new managed repository changes. | Keep; rollback data safety. |
| `divergent_branch_records_stop_before_cutover_and_write_source_hash_conflicts` | `src/core/managed_workspace/migration/tests/divergence.rs` | 1.103 | Conflicting records from branch histories produce source hashes and block cutover. | Keep; branch-level source conflict. |
| `divergent_branch_workspace_identity_stops_migration` | `src/core/managed_workspace/migration/tests/divergence.rs` | 1.116 | Conflicting workspace identities across branches block migration. | Keep; distinct from record-content conflict. |
| `divergent_linked_worktree_snapshot_is_reported_without_checkout_or_reset` | `src/core/managed_workspace/migration/tests/divergence.rs` | 1.105 | Reports linked-worktree divergence without checkout, reset, or source mutation. | Keep; worktree preservation. |
| `registered_code_repositories_aggregate_into_one_planning_workspace` | `src/core/managed_workspace/migration/tests/multi_repository.rs` | 3.311 | Aggregates registered repositories into one planning workspace and preserves each source. | Keep; multi-repository happy path. |
| `missing_registered_checkout_is_reported_and_does_not_block_planning_migration` | `src/core/managed_workspace/migration/tests/multi_repository.rs` | 2.689 | Retains missing-checkout diagnostics while migrating available sources. | Keep; partial-source availability behavior. |
| `conflicting_records_from_registered_checkouts_emit_source_hashes_and_abort` | `src/core/managed_workspace/migration/tests/multi_repository.rs` | 0.712 | Same-record conflicts produce source hashes and abort before workspace creation. | Keep; content conflict across repositories. |
| `different_legacy_workspace_ids_are_reported_instead_of_merged` | `src/core/managed_workspace/migration/tests/multi_repository.rs` | 0.799 | Distinct legacy workspace IDs remain a structured conflict. | Keep; identity conflict across repositories. |
| `approval_fingerprint_conflicts_name_the_stable_feature_uid` | `src/core/managed_workspace/migration/tests/multi_repository.rs` | 0.670 | Approval conflicts identify the stable feature UID. | Keep; approval identity provenance. |
| `open_resolved_divergence_across_sources_has_a_structured_preflight_conflict` | `src/core/managed_workspace/migration/tests/multi_repository/item_conflicts.rs` | 0.773 | Open/resolved item disagreement records each source disposition before cutover. | Keep; item-specific cross-source conflict. |
| `migration_normalizes_legacy_item_without_uid_and_reuses_the_identity` | `src/core/managed_workspace/migration/tests/item_identity.rs` | 2.147 | Normalizes an item lacking a UID and persists a reusable identity. | Keep; legacy item identity compatibility. |
| `symlink_escape_blocks_migration_without_changing_source_or_target` | `src/core/managed_workspace/migration/tests/symlink.rs` | 0.374 | Symlink traversal outside the source blocks migration without changing either tree. | Keep; path-boundary security case. |

## Legacy record readers and product documents

| Test | Source | Time (s) | Protected invariant | Decision |
| --- | --- | ---: | --- | --- |
| `legacy_items_without_koolade_identity_load_for_migration` | `src/artifacts/items_io/tests.rs` | 0.104 | Reads old item records lacking the current identity field. | Keep; pure compatibility parser case. |
| `archive_relocation_preserves_evidence_and_retries_checkpoint_paths` | `src/artifacts/product_docs/tests.rs` | 0.184 | Relocates archived evidence and recovers saved paths after checkpoint interruption. | Keep; product-document migration recovery. |
| `bootstrap_uses_compact_core_and_preserves_legacy_thirteen_section_migrations` | `src/artifacts/product_docs/tests.rs` | 0.126 | Compact bootstrap retains historical thirteen-section migration inputs. | Keep; preserves old document migration support. |
| `canonical_product_tree_does_not_fall_back_to_legacy_manifest_loading` | `src/artifacts/product_docs/tests.rs` | 0.029 | Canonical product documents do not silently load a legacy manifest. | Keep; prevents legacy data from overriding canonical state. |
| `migration_preserves_historical_tasks_and_open_board_items` | `src/artifacts/product_docs/tests.rs` | 0.098 | Product document migration retains historical tasks and open board items. | Keep; user-visible planning data preservation. |

## Audit result

The cases cover distinct source formats, recovery points, repository topologies, and fail-closed conflicts. Some tests exercise a positive path and its negative counterpart, but they protect different outcomes (for example content conflict versus workspace identity conflict). No obsolete or clearly duplicated assertion was found in this migration slice, so none was removed. This report is a focused migration slice, not a full-suite inventory. The complete source-level review of all 1,961 current inventory rows, including migration-related rows outside this slice, is recorded in [full-suite.md](full-suite.md) and [full-suite.csv](full-suite.csv).
