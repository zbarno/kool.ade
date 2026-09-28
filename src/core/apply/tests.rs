use super::*;
use crate::core::validation;
use crate::domain::{CurrentUser, ItemKind, Priority};
use crate::harness::TurnEnvelope;

fn state_at(tag: &str) -> (PlannerState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("packet_apply_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init"].as_slice(),
        ["config", "user.email", "packet@test.local"].as_slice(),
        ["config", "user.name", "Packet Test"].as_slice(),
    ] {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output();
    }
    let mut st = PlannerState::load(&root).unwrap();
    st.bootstrap_missing().unwrap();
    (st, root)
}

fn make_norm(change: Option<&str>, spec: Option<&str>) -> NormalizedTurn {
    NormalizedTurn {
        assistant_message: "done".into(),
        change_summary: change.map(str::to_string),
        spec_markdown: spec.map(str::to_string),
        document_updates: Vec::new(),
        change_status_updates: Default::default(),
        additional_planning_artifacts: vec![],
        added: vec![],
        updates: vec![],
        resolved: vec![],
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        workflow: None,
        task_batch: None,
        warnings: vec![],
        plan_comparison: None,
    }
}

#[test]
fn writes_changed_files_and_labels_checkpoint() {
    let (mut st, root) = state_at("both");
    st.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "deploy frequency?".into(),
        "ops cadence".into(),
    ));
    st.baseline_items_md = items_io::serialize(&[]); // queue as of the PREVIOUS commit
    let nt = make_norm(
        Some("Add caching policy"),
        Some("# Spec v2\nCache: Redis\n"),
    );
    let rc = apply(&mut st, &nt).unwrap();
    assert!(rc.spec_written && rc.items_written);
    assert_eq!(rc.repo_relative_paths, vec![SPEC_FILE, OPEN_ITEMS_FILE]);
    assert_eq!(rc.commit_message, "planner: add caching policy");
    let spec_disk =
        crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&root, SPEC_FILE))
            .unwrap();
    assert!(spec_disk.contains("Redis"));
    let items_disk =
        crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&root, OPEN_ITEMS_FILE))
            .unwrap();
    assert!(
        items_disk.contains("deploy frequency?"),
        "queued item must round-trip to disk"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn compare_plans_saves_typed_workflow_record_without_mutating_feature_document() {
    let (mut state, root) = state_at("plan_comparison");
    let feature_dir = root.join(".kool-ade-packet/planning/changes/CHG-004-saved-searches");
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
    std::fs::write(&path, &body).unwrap();
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
    assert_eq!(
        receipt.repo_relative_paths,
        vec![".kool-ade-packet/state/workflow.json"]
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
    let persisted = crate::artifacts::items_io::parse(
        &std::fs::read_to_string(root.join(crate::artifacts::OPEN_ITEMS_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        persisted[0].feature_uid.as_deref(),
        Some(feature_uid.as_str())
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_change_turn_touches_nothing() {
    let (mut st, root) = state_at("noop");
    let nt = make_norm(None, None);
    let rc = apply(&mut st, &nt).unwrap();
    assert!(!rc.spec_written && !rc.items_written);
    assert!(rc.repo_relative_paths.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unowned_used_category_spawns_single_ownership_item() {
    let (mut st, root) = state_at("own");
    st.config.user = Some(CurrentUser::new("Zach", vec!["Engineering".into()]));
    st.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Operations".into(),
        Some("Ops lead".into()),
        "who watches prod logs?".into(),
        String::new(),
    ));
    st.baseline_items_md = items_io::serialize(&st.items);
    let e = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("logged".into()),
        change_summary: None,
        document_updates: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        interview: None,
        task_stories: None,
        task_outline: None,
        plans: None,
        recommendation: None,
    };
    let nt = validation::validate(&e, &st, &st.effective_user()).unwrap();
    let rc = apply(&mut st, &nt).unwrap();
    // Only the USED category (Operations) spawns an ownership item, and
    // exactly one; a re-apply must not multiply them.
    let n_own = st
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Ownership)
        .count();
    assert_eq!(n_own, 1);
    assert_eq!(rc.synthesized_open_items.len(), 1);
    let nt2 = validation::validate(&e, &st, &st.effective_user()).unwrap();
    let rc2 = apply(&mut st, &nt2).unwrap();
    assert!(rc2.synthesized_open_items.is_empty());
    assert_eq!(
        st.items
            .iter()
            .filter(|i| i.kind == ItemKind::Ownership)
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn commit_phrase_lowercases_agent_summary() {
    let (st, root) = state_at("msg");
    let nt = make_norm(Some("Establish initial specification"), None);
    assert_eq!(
        compose_commit_message(&nt, true, false, 0, 0, 0),
        "planner: establish initial specification"
    );
    let _ = (st, std::fs::remove_dir_all(&root));
}
