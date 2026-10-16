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
