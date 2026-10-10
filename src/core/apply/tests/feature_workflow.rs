use super::*;

#[test]
fn compare_plans_saves_typed_workflow_record_without_mutating_feature_document() {
    let (mut state, root) = state_at("plan_comparison");
    let feature_dir = root.join(".koolade-packet/planning/changes/CHG-004-saved-searches");
    std::fs::create_dir_all(&feature_dir).unwrap();
    let path = feature_dir.join("specification.md");
    let body = crate::artifacts::product_docs::identity::preserve_feature_identity(
        &path,
        "CHG-004",
        "# CHG-004: Saved searches\n\n**Status:** Draft\n",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let body = crate::domain::ChangeMetadata::write_markdown(
        &body,
        &identity,
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    let body = crate::domain::ChangeMetadata::write_markdown(
        &body,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let store = state.planning_store.clone();
    let (_, revision) = store
        .transaction_with_revision(
            &[(
                "planning/changes/CHG-004-saved-searches/specification.md".into(),
                body.as_bytes().to_vec(),
            )],
            Some(&state.baseline_planning_revision),
        )
        .unwrap();
    state.baseline_planning_revision = revision;
    state.active_feature = Some(("CHG-004".into(), body.clone()));
    state.active_features.push(("CHG-004".into(), body));
    let plan = |id: &str| crate::domain::PlanAlternative {
        id: id.into(),
        objective: "Safe rollout".into(),
        phases: vec![
            crate::domain::PlanPhase {
                name: "Prepare".into(),
                subtasks: vec!["Record current values".into()],
            };
            3
        ],
        files_touched: vec![format!("src/search_{id}.rs")],
        state_changes: vec!["Persist choice".into()],
        failure_modes: vec!["Write error".into()],
        effort_band: "Small — one module".into(),
        known_risks: vec!["Migration".into()],
        reversibility: "Restore prior file".into(),
    };
    let mut normalized = make_norm(Some("Compare plans"), None);
    normalized.plan_comparison = Some(crate::domain::PlanComparison {
        alternatives: vec![plan("A"), plan("B")],
        recommendation: crate::domain::PlanRecommendation {
            plan_id: "A".into(),
            rationale: "Fewer writes".into(),
            evidence: vec!["src/search.rs".into()],
        },
        selected_plan: None,
    });
    let receipt = apply(&mut state, &normalized).unwrap();
    let workflow_uid = state.workflow.feature_record_ids["CHG-004"].clone();
    assert_eq!(
        receipt.repo_relative_paths,
        vec![format!(
            ".koolade-packet/state/workflow/{workflow_uid}.json"
        )]
    );
    assert!(!state.active_feature.unwrap().1.contains("planComparison"));
    assert!(!state.active_features[0].1.contains("planComparison"));
    assert!(
        state.workflow.plan_comparisons["CHG-004"]
            .validate()
            .is_ok()
    );
    let restarted = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    assert_eq!(
        restarted.plan_comparisons["CHG-004"],
        state.workflow.plan_comparisons["CHG-004"]
    );
    assert!(
        !std::fs::read_to_string(path)
            .unwrap()
            .contains("planComparison")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_open_item_links_to_feature_created_in_same_turn() {
    let (mut state, root) = state_at("new_feature_link");
    let feature_id = crate::artifacts::product_docs::next_feature_id(&root);
    let feature = format!(
        "# {feature_id}: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave named searches.\n\n## Current Behavior\n\nSearches are temporary.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch controls only.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSaved searches reopen.\n\n## Decisions and Assumptions\n\nNone yet.\n\n## Acceptance Criteria\n\nSaved searches reopen after restart.\n"
    );
    let mut item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::High,
        ItemKind::Question,
        "Product".into(),
        None,
        "Should saved searches be shared?".into(),
        "The feature draft leaves sharing open.".into(),
    );
    item.feature_id = Some(feature_id.clone());
    state.items.push(item);
    let mut turn = make_norm(None, None);
    turn.document_updates
        .push((format!("feature:{feature_id}"), feature));
    turn.change_status_updates
        .insert(feature_id.clone(), crate::domain::ChangeStatus::Draft);

    apply(&mut state, &turn).unwrap();

    let path =
        crate::artifacts::product_docs::document_path(&root, &format!("feature:{feature_id}"))
            .unwrap();
    let feature_uid =
        crate::domain::ArtifactIdentity::from_markdown(&std::fs::read_to_string(path).unwrap())
            .unwrap()
            .unwrap()
            .uid;
    assert_eq!(
        state.items[0].feature_uid.as_deref(),
        Some(feature_uid.as_str())
    );
    let (persisted, _, _) = crate::artifacts::items_io::load_store(&state.planning_store).unwrap();
    assert_eq!(
        persisted[0].feature_uid.as_deref(),
        Some(feature_uid.as_str())
    );
    let _ = std::fs::remove_dir_all(&root);
}
