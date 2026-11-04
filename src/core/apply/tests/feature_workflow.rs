use super::*;

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn compare_plans_saves_typed_workflow_record_without_mutating_feature_document() {
    let (mut state, root, local_root, _home_guard) = state_at("plan_comparison");
    let feature_dir = state.repo_root.join("docs/koolade/features");
    std::fs::create_dir_all(&feature_dir).unwrap();
    let body = crate::artifacts::product_docs::identity::preserve_feature_identity_with_previous(
        None,
        "CHG-004",
        "# CHG-004: Saved searches\n\n**Status:** Ready\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nSearches are temporary.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch UI.\n\n## Affected Product Areas\n\n`product:overview`\n\n## Requirements\n\nSaved searches reopen.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nSaved searches reopen after restart.\n",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let body = crate::domain::ChangeMetadata::write_markdown(
        &body,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let path = feature_dir.join(format!("{}.md", identity.uid));
    std::fs::write(&path, &body).unwrap();
    let navigation = feature_dir.join("AGENTS.md");
    let mut index = std::fs::read_to_string(&navigation).unwrap();
    index.push_str(&format!(
        "\n- [CHG-004: Saved searches](./{}.md) - Published test feature.\n",
        identity.uid
    ));
    std::fs::write(&navigation, index).unwrap();
    for args in [
        ["add", "docs/koolade/features"].as_slice(),
        ["commit", "-qm", "Seed published plan comparison feature"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&state.repo_root)
                .status()
                .unwrap()
                .success()
        );
    }
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        state.planning_store.project_id,
        &state.planning_store.root,
        &state.repo_root,
    );
    state = PlannerState::load_with_store(&state.repo_root, &store).unwrap();
    state.active_feature = Some(("CHG-004".into(), body.clone()));
    state.active_features = vec![("CHG-004".into(), body.clone())];
    let head_before = git(&root, &["rev-parse", "HEAD"]);
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
    assert_eq!(receipt.changed_store_paths.len(), 1);
    assert!(!state.active_feature.unwrap().1.contains("planComparison"));
    assert!(!state.active_features[0].1.contains("planComparison"));
    assert!(
        state.workflow.plan_comparisons["CHG-004"]
            .validate()
            .is_ok()
    );
    let restarted = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    assert_eq!(
        restarted.plan_comparisons["CHG-004"],
        state.workflow.plan_comparisons["CHG-004"]
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head_before);
    assert!(git(&root, &["status", "--porcelain"]).is_empty());
    let pin = state.planning_store.published_spec_pin.as_ref().unwrap();
    let published = crate::core::published_specs::PublishedSpecReader::new(
        &root,
        pin.repository_id.clone(),
        pin.reference.clone(),
    )
    .unwrap()
    .read(pin, &format!("docs/koolade/features/{}.md", identity.uid))
    .unwrap();
    assert_eq!(published.content, body);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(local_root);
}

#[test]
fn new_open_item_links_to_feature_created_in_same_turn() {
    let (mut state, root, local_root, _home_guard) = state_at("new_feature_link");
    let feature_id = crate::artifacts::product_docs::next_feature_id(&state.planning_store);
    let feature = format!(
        "# {feature_id}: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave named searches.\n\n## Current Behavior\n\nSearches are temporary.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch controls only.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSaved searches reopen.\n\n## Decisions and Assumptions\n\nNone yet.\n\n## Acceptance Criteria\n\nSaved searches reopen after restart.\n"
    );
    let mut item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::High,
        ItemKind::Question,
        "Product".into(),
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

    let draft_path = std::fs::read_dir(state.planning_store.root.join("state/spec_drafts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let draft: serde_json::Value =
        serde_json::from_slice(&std::fs::read(draft_path).unwrap()).unwrap();
    let draft_feature = draft["changes"][0]["content"].as_str().unwrap();
    let feature_uid = crate::domain::ArtifactIdentity::from_markdown(draft_feature)
        .unwrap()
        .unwrap()
        .uid;
    assert!(!state.planning_store.root.join("planning/changes").exists());
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
    let _ = std::fs::remove_dir_all(&local_root);
}
