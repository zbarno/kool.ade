use super::*;

#[test]
fn generated_progress_revision_is_used_by_the_real_apply_finalizer() {
    let (mut state, root) = state_at("task_generation_revision");
    let store = state.planning_store.clone();
    let mut batch = task_batch();
    approve_batch_feature(&mut state, &root, &mut batch);
    let (_, revision) = crate::artifacts::task_docs::save_progress_expected(
        &store,
        "revision-fenced-run",
        &batch,
        1,
        &state.baseline_planning_revision,
    )
    .unwrap();
    state.baseline_planning_revision = revision;

    let expected_requirement_ids = batch.stories[0].approved_requirement_ids.clone();
    let receipt = apply(&mut state, &task_batch_turn(batch)).unwrap();

    assert_eq!(state.workflow.task_batches.len(), 1);
    assert_eq!(
        crate::artifacts::task_docs::load_workflow(&store)
            .unwrap()
            .task_batches,
        state.workflow.task_batches
    );
    assert!(
        receipt
            .repo_relative_paths
            .iter()
            .any(|path| { path.ends_with(".koolade-progress.json") })
    );
    assert_eq!(state.baseline_planning_revision, store.revision().unwrap());
    let identity =
        crate::domain::ArtifactIdentity::from_markdown(&state.active_feature.as_ref().unwrap().1)
            .unwrap()
            .unwrap();
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&store);
    let feature = lifecycle.load_feature(&identity.uid).unwrap();
    let persisted_task = lifecycle.load_task(&feature.task_uids[0]).unwrap();
    assert_eq!(
        persisted_task.approved_requirement_ids,
        expected_requirement_ids
    );
    let _ = std::fs::remove_dir_all(root);
}

/// Production boundary: task batch writes atomically fence the exact readiness
/// record revision, even when the planning-store revision is unchanged. A unit
/// validator cannot prove the record check participates in the batch transaction.
/// External dependencies: temporary managed-local store only. Setup cost: low.
#[test]
fn readiness_change_after_generation_preflight_rejects_task_batch_atomically() {
    let (mut state, root) = state_at("task_generation_readiness_race");
    let mut batch = task_batch();
    approve_batch_feature(&mut state, &root, &mut batch);
    let identity =
        crate::domain::ArtifactIdentity::from_markdown(&state.active_feature.as_ref().unwrap().1)
            .unwrap()
            .unwrap();
    let feature = crate::core::lifecycle::LifecycleStore::new(&state.planning_store)
        .load_feature(&identity.uid)
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
        &state.planning_store,
        feature.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap();
    let readiness_check = crate::core::lifecycle::plan_snapshot::require_current_readiness(
        &state.planning_store,
        &snapshot,
    )
    .unwrap();

    let questions = crate::core::planning::QuestionStore::new(&state.planning_store);
    let (mut readiness, revision) = questions.load_readiness(&identity.uid).unwrap().unwrap();
    readiness.ready = false;
    readiness.blocker_notes = vec!["A blocker arrived after generation preflight.".into()];
    readiness.revision += 1;
    questions.save_readiness(&readiness, revision).unwrap();
    assert_eq!(
        state.planning_store.revision().unwrap(),
        state.baseline_planning_revision
    );

    let error = crate::artifacts::task_docs::write_batch_expected_with_revision_and_checks(
        &state.planning_store,
        &batch,
        &mut state.workflow,
        &state.baseline_planning_revision,
        &[readiness_check],
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("state/readiness/")
            && error.to_string().contains("expected revision 1, found 2"),
        "unexpected failure: {error:?}"
    );
    assert!(state.workflow.task_batches.is_empty());
    assert!(
        crate::artifacts::task_docs::load_board(&state.planning_store, &state.workflow).is_empty()
    );
    let _ = std::fs::remove_dir_all(root);
}
