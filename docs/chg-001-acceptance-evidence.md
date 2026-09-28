# CHG-001 acceptance evidence

Historical evidence note: the repository path recorded below refers to the
pre-migration artifact layout. Current live artifacts use `.kool-ade-packet/`;
see [the artifact layout](artifact-layout.md).

This index maps the feature's numbered acceptance criteria to current repository evidence. The feature specification remains the normative acceptance list; this file points reviewers to the smallest proof for each item.

| AC | Evidence |
| --- | --- |
| 1 | `core::context_build::tests::historical_features_tasks_and_items_do_not_expand_normal_prompt_without_bound` creates 250 feature histories, task refs, items, and imports; completed feature text is absent from the bounded prompt. |
| 2 | `planning/features/CHG-001-scalable-planning-and-feature-specification/specification.md`; `core::turn::tests::modular_turn_changes_only_named_modules_and_rejects_bad_id_atomically`; `product_docs::next_feature_id`. |
| 3 | The same modular-turn test proves only the named module changes; `contract_snapshot::freeze` selects modules referenced by the feature. |
| 4 | `core::investigation::tests::evidence_resolves_agent_item_without_chat_question` runs the read-only evidence worker and resolves its Agent item without selecting a chat question. |
| 5 | `app::root::board_tests::kanban_distinguishes_authority_blockers_tasks_and_completed_work` renders a nonblocking Human item in To do while the chat-selection contract remains limited by AC6. |
| 6 | `core::validation::tests::next_question_enforces_the_routing_law` accepts only one eligible Human/Blocking ID; `ownership_items_are_never_chat_questions` and `core::prompt` enforce the same rule. |
| 7 | `tests/multi_repository_feature.rs` first rejects generation without explicit approval, then proves that the generation prompt and `contract.json` contain the approved feature, selected module, configuration identity, and repository heads. |
| 8 | `core::reconciliation::tests::merged_commit_is_inspected_before_product_truth_is_checkpointed` proves merged-code inspection and scoped product update. |
| 9 | `tests/multi_repository_feature.rs` generates dependent `api` and `web` stories, each with one target repository, while local checkout paths stay private. `core::implementation::tests::cross_repository_dependency_context_requires_merged_record` proves a dependent worker receives only a completed predecessor contract and merged commit. |
| 10 | `artifacts::product_docs::tests::migration_preserves_all_thirteen_sections_and_stable_ids`, `migration_preserves_historical_tasks_and_open_board_items`, and `restart_after_uncommitted_migration_still_reports_all_paths`. |
| 11 | `product_docs::render_product` composes the thirteen validated modules; `app::root::board_tests::document_switcher_displays_active_feature_product_and_task_story` clicks Active Feature, Product Specification, and Task Stories in the real egui renderer. |
| 12 | `core::turn::tests::modular_turn_changes_only_named_modules_and_rejects_bad_id_atomically`, `artifacts::transaction::tests::interrupted_document_set_restores_original_bytes`, and `persisted_journal_recovers_partial_write_on_restart`; accepted paths are git-checkpointed by `core::apply`/`gitops`. |
| 13 | `core::context_build::tests::accepted_feature_knowledge_survives_chat_expiration_and_derived_cache_deletion` proves an expired chat marker disappears while the accepted feature fact remains. |
| 14 | Planning: `TurnContext::build`; task generation: `tests/multi_repository_feature.rs`; implementation: `implementation_context_uses_only_frozen_affected_product_modules` and the merged-dependency test; reconciliation: `merged_commit_is_inspected_before_product_truth_is_checkpointed`. Each uses a distinct prompt and source set. |
| 15 | `accepted_feature_knowledge_survives_chat_expiration_and_derived_cache_deletion` deletes derived cache state, reloads authoritative artifacts, and rebuilds the same accepted feature context. No semantic/vector store is required or present. |
| 16 | The 250-feature growth fixture under AC1 asserts prompt growth remains bounded and caps conversation, imports, items, repository survey, task refs, and selected modules without inventing a token target. |
| 17 | `TurnContext::build` selects modules by explicit logical module ID and stable `F-`, `FR-`, `NFR-`, and `D-` references before any repository search. The growth fixture proves the explicitly affected module is selected. |
| 18 | Packet currently has no semantic/vector retrieval path. `docs/living-specification-policy.md` requires any future semantic match to be reopened at its authoritative source; AC15 proves authority does not reside in derived state. |
| 19 | `app::root::board_tests::migrated_open_item_remains_clickable_on_kanban` migrates a legacy project, renders the preserved card, clicks it, and verifies its detail. |
| 20 | `kanban_distinguishes_authority_blockers_tasks_and_completed_work` renders all five workflow columns plus Agent, Review, Human, Blocking, implementation, and completed cards. `agent_item_shows_live_investigation_on_board_and_in_detail` verifies active agent activity. |
| 21 | `src/ui/layout.rs` derives cards only from `state.items` and generated task documents; the AC20 renderer fixture verifies an unrelated observation is absent. Settled decisions remain product/feature text rather than cards. |
| 22 | `core::board_actions::tests::board_approval_records_feature_decision_and_resolves_item_atomically` records the recommendation in the related feature, resolves the Review item, and checkpoints the same validated mutation without chat. |
| 23 | `app::root::board_tests::planning_items_use_board_and_modal_even_before_tasks_exist` and the migrated-board test reconstruct the board from `PlannerState.items` and task documents. No board artifact or model-emitted board field exists. |

Additional end-to-end evidence: `app::root::board_tests::auto_queue_runs_two_tasks_through_pi_and_merges_without_prs` runs two dependent tasks through the external Pi process, preserves local planning commits, fetches/integrates/pushes them to `main`, advances the queue, keeps main-agent chat usable, and creates no PR. The desktop binary was launched successfully with `cargo run --offline` on the operator's Wayland session; direct pointer interaction is covered by the egui renderer tests above.
