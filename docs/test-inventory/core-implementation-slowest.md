# Core implementation test inventory: slowest 20

This is the first scoped slice of the #204 test inventory. It records the 20 slowest `core::implementation` cases in the baseline JUnit report, their protected invariants, fixture dependencies, and keep decisions. The full-suite inventory and legacy migration audit remain open.

## Measurement

The baseline is `main` at `60d213e` ([CI run](https://github.com/zbarno/kool.ade/actions/runs/38789423600)). The comparison is the same source with Nextest set to 16 test threads ([PR #211 CI run](https://github.com/zbarno/kool.ade/actions/runs/38789796651)). Each run reported 1,936 passed, 2 skipped, and 0 failed. The baseline Nextest wall time was 507.616 s; the two 16-thread trials measured 222.770 s and 224.837 s. This table uses the second trial. It changes test scheduling only; it does not compare different test inventories.

The 20 cases sum to 470.919 s in the baseline and 466.507 s in the 16-thread trial. Summed case durations are not suite wall time. All listed cases use isolated local Git fixtures and synthetic harness behavior; none requires Docker or SSH. Decisions below preserve each distinct behavior. No case is removed or consolidated by this inventory.

## Full core profile on PR #212

The complete local Nextest report from PR #212's latest source (`target/nextest/default/nextest.xml`, 2026-10-15) includes **193** `core::implementation` cases. Their cumulative case duration is **1,340.014 s**; **75** cases exceed 10 s and **96** exceed 5 s. The full suite in that run passed 1,936 tests, skipped 2, and took 239.629 s of Nextest wall time. These local timings are a distribution snapshot, not a controlled comparison with the CI trials above; machine and Docker capability differ.

| Test group | Cases | Cumulative time (s) |
| --- | ---: | ---: |
| `initial_reconciliation::resilience` | 7 | 133.50 |
| `recovery_corrections` | 14 | 127.75 |
| `cleanup` | 6 | 77.47 |
| `resume_and_pr` | 10 | 70.87 |
| `resume_and_pr::branch_intent` | 7 | 65.21 |
| `initial_reconciliation::runtime_config` | 4 | 64.03 |
| `publication_checks::provider_checks::remote_guards` | 6 | 53.77 |
| `initial_reconciliation::publication_retry` | 2 | 53.06 |
| `publication_checks` | 5 | 52.16 |
| `initial_reconciliation::cache_reuse` | 2 | 51.95 |

This report widens the measured scope beyond the historical slowest-20 table and identifies where further source-level review can focus. It does not establish that the remaining cases are duplicate-free; the complete duplicate/vacuous-test audit and the rest of the repository inventory remain open.

## Inventory

| Test | Source | Baseline / 16 threads (s) | Category and protected invariant | Dependency | Decision |
| --- | --- | ---: | --- | --- | --- |
| `modified_output_is_preserved_and_prior_snapshot_is_archived_on_resume` | `src/core/implementation/tests/initial_reconciliation/resilience.rs:142` | 37.100 / 36.903 | Recovery, data integrity: retain operator edits and archive the exact prior snapshot when resuming. | Git fixture; synthetic harness | Keep; unique archive and modified-output guarantee. |
| `verified_reconciliation_and_generated_output_survive_publication_retry` | `src/core/implementation/tests/initial_reconciliation/publication_retry.rs:56` | 35.633 / 34.787 | Recovery, publication: verified reconciliation and generated files survive a failed publication attempt. | Git fixture; synthetic harness | Keep; covers a failed-publication recovery path. |
| `verified_disjoint_baseline_is_reused_by_followup_tasks` | `src/core/implementation/tests/initial_reconciliation/cache_reuse.rs:17` | 31.680 / 31.416 | Git/reconciliation: a verified disjoint baseline can be reused by a later task. | Git fixture; synthetic harness | Keep; positive cache-reuse invariant. |
| `failed_verification_resumes_with_unchanged_generated_files_and_old_snapshot` | `src/core/implementation/tests/initial_reconciliation/resilience.rs:100` | 30.459 / 29.958 | Recovery, data integrity: failed verification preserves generated output and the old recovery snapshot. | Git fixture; synthetic harness | Keep; distinct from operator-modified output and snapshot archival. |
| `harness_failures_retry_and_saved_diagnostics_survive_lost_final_message` | `src/core/implementation/tests/verification_harness.rs:130` | 28.892 / 31.168 | Harness recovery: retry after provider failure and retain saved diagnostics if the final message is lost. | Git fixture; fake harness | Keep; separate provider retry and checkpoint invariant. The duplicate `sidecar` fixture path was removed in #208. |
| `verified_legacy_plan_refreshes_pending_checks_without_touching_task_edits` | `src/core/implementation/tests/initial_reconciliation/legacy_recovery.rs:88` | 28.328 / 27.739 | Migration, recovery: refresh pending checks from a verified legacy plan while preserving task edits. | Git fixture; synthetic harness | Keep; protects supported legacy recovery data. |
| `resume_after_exhaustion_gets_full_budget_without_nested_history` | `src/core/implementation/tests/recovery_corrections.rs:133` | 26.352 / 26.416 | Recovery: a resumed exhausted task gets its full retry budget without nesting recovery history. | Git fixture; synthetic harness | Keep; retry-budget and history-shape guarantee. |
| `independent_check_gate_pushes_only_the_candidate_and_records_the_exact_result` | `src/core/implementation/tests/publication_checks/provider_checks.rs:7` | 24.258 / 23.766 | Security, publication: the independent gate publishes only the candidate and records the exact check result. | Git fixture; fake provider/check result | Keep; publication boundary and result provenance. |
| `runtime_config_survives_recovery_and_never_enters_stash_or_task_commit` | `src/core/implementation/tests/initial_reconciliation/runtime_config.rs:18` | 21.765 / 21.637 | Security, data isolation: runtime configuration survives recovery without entering a stash or task commit. | Git fixture; synthetic harness | Keep; protects private runtime configuration. |
| `an_interrupted_pinned_merge_is_snapshotted_and_retried` | `src/core/implementation/tests/initial_reconciliation/merge_recovery/snapshots.rs:4` | 21.570 / 21.149 | Recovery: snapshot and retry an interrupted pinned merge. | Git fixture; synthetic harness | Keep; interrupted-merge recovery path. |
| `fast_forward_hook_edits_are_not_accepted_as_a_cached_baseline` | `src/core/implementation/tests/initial_reconciliation/cache_reuse.rs:87` | 20.620 / 20.140 | Git safety: reject hook edits introduced during fast-forward when validating a cached baseline. | Git fixture; synthetic harness | Keep; negative counterpart to valid baseline reuse. |
| `legacy_nested_project_check_is_rescoped_when_reconciliation_resumes` | `src/core/implementation/tests/initial_reconciliation/crash_recovery.rs:83` | 19.955 / 19.702 | Migration, recovery: rescope legacy nested-project checks on resume. | Git fixture; synthetic harness | Keep; distinct legacy check migration behavior. |
| `missing_task_state_resumes_the_saved_commit_snapshots` | `src/core/implementation/tests/initial_reconciliation/crash_recovery.rs:133` | 18.682 / 18.305 | Crash recovery: recover from missing task state using the saved commit snapshots. | Git fixture; synthetic harness | Keep; protects reconstruction after lost state. |
| `histories_already_in_task_base_skip_duplicate_reconciliation` | `src/core/implementation/tests/initial_reconciliation/merge_recovery/cases.rs:57` | 18.327 / 18.272 | Git/reconciliation: skip redundant reconciliation when histories are already in the task base. | Git fixture; synthetic harness | Keep; proves safe duplicate-work avoidance. |
| `inherited_eof_whitespace_from_remote_history_is_preserved` | `src/core/implementation/tests/initial_reconciliation/resilience.rs:249` | 18.007 / 17.813 | Merge data integrity: preserve inherited EOF whitespace from remote history. | Git fixture; synthetic harness | Keep; remote-side input to merge preservation. |
| `inherited_eof_whitespace_from_local_history_is_preserved` | `src/core/implementation/tests/initial_reconciliation/resilience.rs:244` | 17.976 / 17.691 | Merge data integrity: preserve inherited EOF whitespace from local history. | Git fixture; synthetic harness | Keep; local-side input is distinct from remote history. |
| `generated_output_survives_publication_retry_without_reconciliation_plan` | `src/core/implementation/tests/initial_reconciliation/publication_retry.rs:127` | 17.864 / 17.351 | Recovery, publication: retain generated output on retry when no reconciliation plan exists. | Git fixture; synthetic harness | Keep; exercises the no-plan path, distinct from planned reconciliation. |
| `divergent_histories_are_combined_before_the_task_agent_runs` | `src/core/implementation/tests/initial_reconciliation.rs:155` | 17.860 / 17.543 | Git/reconciliation: combine divergent local and remote histories before starting the agent. | Git fixture; synthetic harness | Keep; ordering guarantee for agent execution. |
| `moved_whitespace_inherited_from_both_sources_is_preserved` | `src/core/implementation/tests/initial_reconciliation/whitespace.rs:41` | 17.817 / 17.760 | Merge data integrity: preserve moved whitespace inherited from both histories. | Git fixture; synthetic harness | Keep; distinct two-source whitespace case. |
| `preface_and_json_fence_are_removed_before_report_validation` | `src/core/implementation/tests/initial_reconciliation/report_envelope.rs:30` | 17.774 / 16.991 | Report validation: strip permitted preface/fence formatting before validating the report body. | Git fixture; synthetic harness | Keep; protects accepted report-envelope parsing. |

## Findings and follow-up

- The longest cases are stateful Git/recovery and publication flows. Their cost is not evidence of duplicate coverage by itself.
- #207 replaced two expensive invalid-history end-to-end repetitions with direct tests of the same recovery predicates while retaining an end-to-end invalid-phase test.
- #208 removed a redundant fixture mode from the long harness-recovery case. Its traced Git process count fell from 1,237 to 883, with CPU and elapsed measurements included in that PR's description.
- #208 measured the process-local environment mutex in 121 core tests: 5.11 ms cumulative wait, 44.4 μs mean, 59.6 μs p95, and 181 μs maximum. Since Nextest runs each case in a separate process, this mutex does not serialize cases across the suite.
- Continue the inventory across the remaining tests, and separately audit legacy migration coverage before considering any further removals.

## Legacy worktree and migration audit

This audit covers `src/core/implementation`. It checked the legacy task repository discriminator, migration entry point, validation/snapshot code, and tests that construct legacy task or lifecycle state. The wider repository migration audit remains open.

`execution/task_state.rs` still dispatches `TaskRepositoryKind::LegacyWorktree` into `execution/task_state/legacy_workspace.rs`. That path validates the saved checkout and branch, captures staged and unstaged state, copies changes into an app-owned task clone, verifies the result, and preserves the original worktree. It records progress so an interrupted copy can resume safely. Related code also migrates legacy integration workspaces and attaches clone identity to old reconciliation plans. The production migration window is therefore still active.

| Test | Source | Protected migration invariant | Decision |
| --- | --- | --- | --- |
| `saved_legacy_workspace_migrates_all_local_changes_and_keeps_original` | `src/core/implementation/tests/task_repositories/legacy_migration.rs:58` | Copies staged, unstaged, branch-local, untracked, unusual-name, and symlink changes; preserves the original linked worktree. | Keep; primary successful migration and source-preservation proof. |
| `interrupted_migration_resumes_a_partial_clone_without_overwriting_it` | `src/core/implementation/tests/task_repositories/legacy_migration.rs:97` | Refuses to overwrite conflicting data in a partial destination and keeps the source. | Keep; crash/resume data-integrity case. |
| `concurrent_resume_cannot_enter_the_same_legacy_migration_twice` | `src/core/implementation/tests/task_repositories/legacy_migration/concurrency.rs:56` | Serializes concurrent resumes so only one can reach the harness for a migration. | Keep; migration race guard. |
| `missing_legacy_worktree_metadata_blocks_migration_and_preserves_workspace_files` | `src/core/implementation/tests/task_repositories/legacy_migration/metadata.rs:4` | Fails closed when Git metadata is missing without deleting or rewriting user files. | Keep; fail-closed data-preservation case. |
| `completed_migration_is_idempotent_after_task_state_save_interruption` | `src/core/implementation/tests/task_repositories/legacy_migration/idempotency.rs:31` | Reuses the completed migration record after task-state persistence is interrupted. | Keep; idempotent recovery case. |
| `verified_legacy_plan_refreshes_pending_checks_without_touching_task_edits` | `src/core/implementation/tests/initial_reconciliation/legacy_recovery.rs:88` | Refreshes legacy verification state while preserving task edits through clean resume and repair. | Keep; legacy plan and task-edit preservation. |
| `legacy_nested_project_check_is_rescoped_when_reconciliation_resumes` | `src/core/implementation/tests/initial_reconciliation/crash_recovery.rs:83` | Re-scopes old nested-project checks when a saved reconciliation resumes. | Keep; compatibility for persisted legacy verification commands. |
| `default_destination_pr_reconciles_remote_changes_before_publication` | `src/core/implementation/tests/resume_and_pr/branch_intent.rs:225` | Exercises branch-intent fallback with the legacy embedded lifecycle store. | Keep; compatibility path still used by production entry points. |
| `generated_output_survives_publication_retry_without_reconciliation_plan` | `src/core/implementation/tests/initial_reconciliation/publication_retry.rs:127` | Runs without a lifecycle task record and preserves publication/reconciliation behavior. | Keep; protects the legacy embedded-store fallback. |

Other `initial_reconciliation` fixtures explicitly serialize `task_repository_kind: legacy_worktree` to exercise recovery, merge, runtime-configuration, and snapshot-rotation protections. The two helper functions in `tests/sandbox/lifecycle.rs` seed and read the legacy embedded store for lifecycle fixtures. These paths exercise separate recovery or lifecycle behaviors rather than repeat the five direct migration outcomes above. No obsolete legacy assertion was identified in this scoped audit, so none was removed. Continue the audit for legacy migration tests outside `core::implementation`.
