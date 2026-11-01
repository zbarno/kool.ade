use super::*;

fn feature(id: &str, title: &str) -> (String, String) {
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        &format!("# {id}: {title}\n\n**Status:** Draft\n"),
        None,
        id,
        title,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &identified,
        &identity,
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    (markdown, identity.uid)
}

fn update(document_id: &str) -> NormalizedTurn {
    NormalizedTurn {
        assistant_message: "Updated another feature".into(),
        change_summary: None,
        spec_markdown: None,
        document_updates: vec![(document_id.into(), "replacement".into())],
        planning_tasks: Vec::new(),
        change_status_updates: Default::default(),
        additional_planning_artifacts: Vec::new(),
        added: Vec::new(),
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    }
}

fn state_with_features() -> PlannerState {
    let root = std::env::temp_dir().join(format!(
        "koolade_turn_feature_target_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut state = PlannerState::load(&root).unwrap();
    state.bootstrap_missing().unwrap();
    let (first, _) = feature("F1", "Original feature");
    let (second, _) = feature("F2", "Other feature");
    state.active_features = vec![("F1".into(), first), ("F2".into(), second)];
    state
}

#[test]
fn linked_task_cannot_update_a_different_active_feature() {
    let mut state = state_with_features();
    let (first, first_uid) = feature("F1", "Original feature");
    state.active_features[0].1 = first;
    let mut work = planning_work::Work::new(
        String::new(),
        "Continue F1 planning".into(),
        "Update the original feature".into(),
        "Planning in progress".into(),
    );
    work.key = format!("task:{}", work.uid);
    work.feature_id = Some("F1".into());
    work.feature_uid = Some(first_uid);
    let key = work.key.clone();
    planning_work::save(&state.planning_store, &[work]).unwrap();

    assert!(validate_task_link(&state, Some(&key), None).is_ok());
    let error = validate_updates(&state, Some(&key), None, &update("feature:F2")).unwrap_err();
    assert!(error.starts_with(STALE_LINK));
    assert!(error.contains("feature F2"));
    let _ = std::fs::remove_dir_all(state.repo_root);
}

#[test]
fn synthetic_feature_conversation_is_bound_to_its_own_uid() {
    let state = state_with_features();
    assert!(validate_task_link(&state, Some("feature:F1"), None).is_ok());
    let error =
        validate_updates(&state, Some("feature:F1"), None, &update("feature:F2")).unwrap_err();
    assert!(error.starts_with(STALE_LINK));
    let _ = std::fs::remove_dir_all(state.repo_root);
}

#[test]
fn linked_board_item_cannot_follow_a_reused_feature_id() {
    let mut state = state_with_features();
    let (_, old_uid) = feature("F1", "Prior feature");
    let mut item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("Product".into()),
        "Clarify the feature behavior".into(),
        "A planning decision is needed".into(),
    );
    item.feature_id = Some("F1".into());
    item.feature_uid = Some(old_uid);
    state.items.push(item);

    let error = validate_task_link(&state, Some("CLR-001"), None).unwrap_err();
    assert!(error.starts_with(STALE_LINK));
    let _ = std::fs::remove_dir_all(state.repo_root);
}

#[test]
fn pinned_feature_conversation_cannot_follow_a_reused_display_id() {
    let mut state = state_with_features();
    let expected_uid = task_feature_uid(&state, "feature:F1").unwrap();
    let (replacement, replacement_uid) = feature("F1", "Replacement feature");
    assert_ne!(expected_uid, replacement_uid);
    state.active_features[0].1 = replacement;

    let error = validate_task_link(&state, Some("feature:F1"), Some(&expected_uid)).unwrap_err();
    assert!(error.starts_with(STALE_LINK));
    assert!(error.contains(&expected_uid));
    assert!(error.contains(&replacement_uid));
    let update_error = validate_updates(
        &state,
        Some("feature:F1"),
        Some(&expected_uid),
        &update("feature:F1"),
    )
    .unwrap_err();
    assert!(update_error.starts_with(STALE_LINK));
    let _ = std::fs::remove_dir_all(state.repo_root);
}
