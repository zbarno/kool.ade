use super::*;

#[test]
fn schema_three_seeds_stable_ids_and_relationships_idempotently() {
    use crate::domain::{ArtifactIdentity, ItemKind, OpenItem, Priority};

    let root = repo("stable-identities");
    let batch_dir = format!(
        "{}/F7-saved-searches",
        crate::artifacts::layout::canonical::TASKS
    );
    let task_path = format!("{batch_dir}/F7-TASK-save-a-search.md");
    let feature = "# F7: Saved searches\n\n**Status:** Ready\n\n## Intent\n\nSave named searches.\n\n## Current Behavior\n\nSearches are not saved.\n\n## Desired Behavior\n\nUsers can save named searches.\n\n## Scope\n\nSearches only.\n\n## Affected Product Areas\n\nProduct search.\n\n## Requirements\n\n- Saved searches can be reopened.\n\n## Decisions and Assumptions\n\n- Local persistence.\n\n## Acceptance Criteria\n\n- A saved search reopens after restart.\n";
    let task = "# F7-TASK-save-a-search — Add a saved search\n\nFeature: Saved searches\n\n## Dependencies\n\nNone.\n\n## Acceptance criteria\n\n- A saved search can be reopened.\n".to_string();
    let adr = format!(
        "# Persist saved searches locally\n\n- Ticket: `{task_path}`\n\n## Context\n\nUsers want named searches.\n\n## Decision\n\nKeep the first version local.\n"
    );

    let feature_path = root
        .join(crate::artifacts::layout::canonical::CHANGES)
        .join("F7-saved-searches/specification.md");
    fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    let feature =
        ArtifactIdentity::preserve_markdown(feature, None, "F7", "Saved searches").unwrap();
    let identity = ArtifactIdentity::from_markdown(&feature).unwrap().unwrap();
    let mut feature = crate::domain::ChangeMetadata::write_markdown(
        &feature,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let marker = feature
        .lines()
        .find(|line| line.starts_with("<!-- packet-change:v1 "))
        .unwrap()
        .to_owned();
    let mut metadata: serde_json::Value = serde_json::from_str(
        marker
            .strip_prefix("<!-- packet-change:v1 ")
            .unwrap()
            .strip_suffix(" -->")
            .unwrap(),
    )
    .unwrap();
    metadata["schemaVersion"] = 2.into();
    let comparison = serde_json::json!({
        "alternatives": [
            {"id":"A","objective":"Small change","phases":[],"filesTouched":[],"stateChanges":[],"failureModes":[],"effortBand":"Small","knownRisks":[],"reversibility":"Easy"},
            {"id":"B","objective":"Typed state","phases":[],"filesTouched":[],"stateChanges":[],"failureModes":[],"effortBand":"Medium","knownRisks":[],"reversibility":"Moderate"}
        ],
        "recommendation":{"plan_id":"A","rationale":"Smaller change","evidence":["src/app/feature_approval.rs"]},
        "selected_plan":"B"
    });
    metadata["selectedAlt"] = "B".into();
    metadata["planComparison"] = comparison.clone();
    metadata["comparisonHistory"] = serde_json::json!([comparison]);
    feature = feature.replace(
        &marker,
        &format!(
            "<!-- packet-change:v1 {} -->",
            serde_json::to_string(&metadata).unwrap()
        ),
    );
    fs::write(&feature_path, feature).unwrap();
    let batch_path = root.join(&batch_dir);
    fs::create_dir_all(&batch_path).unwrap();
    fs::write(
        batch_path.join("README.md"),
        "# Saved searches — task stories\n\n- [Add a saved search](F7-TASK-save-a-search.md)\n",
    )
    .unwrap();
    fs::write(
        batch_path.join("specification.md"),
        "Approved specification.\n",
    )
    .unwrap();
    fs::write(root.join(&task_path), &task).unwrap();
    let state_dir = crate::core::implementation::state_dir(&root, &task_path).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    let state = crate::core::implementation::Implementation {
        ticket: task_path.clone(),
        task_uid: None,
        ticket_text: task.clone(),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        branch: "packet/saved-search".into(),
        base: "main".into(),
        base_commit: "base".into(),
        worktree: root.join("worktree"),
        status: crate::core::implementation::ImplementationStatus::Blocked,
        detail: "Preserve this existing task work.".into(),
        pr_url: None,
        verified_head: None,
        auto_merge: false,
        merged_commit: None,
        pr_state: None,
        pr_checked_at: None,
        pr_check_attempted_at: None,
        pr_check_error: None,
        independent_check: None,
        cleanup: Default::default(),
    };
    fs::write(
        state_dir.join("state.json"),
        crate::core::implementation::serialize_state(&state).unwrap(),
    )
    .unwrap();

    let workflow_path = root.join(crate::artifacts::layout::canonical::WORKFLOW);
    fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    fs::write(
        &workflow_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "brief": null,
            "reviewedSpecification": null,
            "taskBatches": [{
                "feature": "Saved searches",
                "directory": batch_dir,
                "count": 1
            }],
            "approvedFeatures": {"F7": "frozen contract"}
        }))
        .unwrap(),
    )
    .unwrap();

    let mut open_item = OpenItem::new(
        "CLR-101".into(),
        Priority::High,
        ItemKind::Question,
        "Product".into(),
        None,
        "Should searches sync between devices?".into(),
        "The first feature draft does not define syncing.".into(),
    );
    open_item.uid = None;
    open_item.feature_id = Some("F7".into());
    open_item.feature_uid = None;
    fs::write(
        root.join(crate::artifacts::layout::canonical::OPEN_ITEMS),
        crate::artifacts::items_io::serialize(&[open_item]),
    )
    .unwrap();
    let mut resolved_item = OpenItem::new(
        "CLR-102".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        None,
        "Should the first release include sync?".into(),
        "This was answered during the migration fixture setup.".into(),
    );
    resolved_item.uid = None;
    resolved_item.feature_id = Some("F7".into());
    resolved_item.feature_uid = None;
    fs::write(
        root.join(crate::artifacts::layout::canonical::RESOLVED_ITEMS),
        serde_json::to_vec_pretty(&vec![resolved_item]).unwrap(),
    )
    .unwrap();
    let decision_path = root
        .join(crate::artifacts::layout::canonical::DECISIONS)
        .join("persist-searches.md");
    fs::create_dir_all(decision_path.parent().unwrap()).unwrap();
    fs::write(&decision_path, adr).unwrap();
    fs::write(
        root.join(crate::artifacts::layout::canonical::MANIFEST),
        r#"{"schemaVersion":2,"product":"Packet"}"#,
    )
    .unwrap();
    commit_all(&root, "schema two project");

    let changed = run(&root).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join(crate::artifacts::layout::canonical::MANIFEST)).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["schemaVersion"], 3);

    let feature_id = ArtifactIdentity::from_markdown(&fs::read_to_string(&feature_path).unwrap())
        .unwrap()
        .unwrap();
    let feature_metadata = crate::domain::ChangeMetadata::require_markdown(
        &fs::read_to_string(&feature_path).unwrap(),
    )
    .unwrap();
    assert_eq!(feature_metadata.status, crate::domain::ChangeStatus::Ready);
    assert_eq!(feature_metadata.uid, feature_id.uid);
    assert_eq!(feature_metadata.schema_version, 2);
    let comparison = feature_metadata.plan_comparison.unwrap();
    assert_eq!(comparison.alternatives.len(), 2);
    assert_eq!(comparison.recommendation.plan_id, "A");
    assert_eq!(comparison.selected_plan.as_deref(), Some("B"));
    assert_eq!(feature_metadata.selected_alt.as_deref(), Some("B"));
    assert_eq!(feature_metadata.comparison_history.len(), 1);
    let readme = fs::read_to_string(batch_path.join("README.md")).unwrap();
    let batch_id = ArtifactIdentity::from_markdown(&readme).unwrap().unwrap();
    assert_eq!(
        batch_id.parent_uid.as_deref(),
        Some(feature_id.uid.as_str())
    );
    let workflow: crate::core::workflow::Workflow =
        serde_json::from_slice(&fs::read(workflow_path).unwrap()).unwrap();
    assert_eq!(workflow.task_batches[0].identity.as_ref(), Some(&batch_id));

    let task_markdown = fs::read_to_string(root.join(&task_path)).unwrap();
    let task_id = ArtifactIdentity::from_markdown(&task_markdown)
        .unwrap()
        .unwrap();
    assert_eq!(task_id.display_id, "F7-TASK-save-a-search");
    assert_eq!(task_id.parent_uid.as_deref(), Some(batch_id.uid.as_str()));
    let migrated_state = crate::core::implementation::load(&root, &task_path).unwrap();
    assert_eq!(
        migrated_state.task_uid.as_deref(),
        Some(task_id.uid.as_str())
    );
    let decision = fs::read_to_string(decision_path).unwrap();
    let decision_id = ArtifactIdentity::from_markdown(&decision).unwrap().unwrap();
    assert_eq!(decision_id.display_id, "ADR-001");
    assert_eq!(
        decision_id.parent_uid.as_deref(),
        Some(task_id.uid.as_str())
    );

    let open_items = crate::artifacts::items_io::parse(
        &fs::read_to_string(root.join(crate::artifacts::layout::canonical::OPEN_ITEMS)).unwrap(),
    )
    .unwrap();
    assert!(open_items[0].uid.is_some());
    assert_eq!(
        open_items[0].feature_uid.as_deref(),
        Some(feature_id.uid.as_str())
    );
    let resolved: Vec<OpenItem> = serde_json::from_slice(
        &fs::read(root.join(crate::artifacts::layout::canonical::RESOLVED_ITEMS)).unwrap(),
    )
    .unwrap();
    assert!(resolved[0].uid.is_some());
    assert_eq!(
        resolved[0].feature_uid.as_deref(),
        Some(feature_id.uid.as_str())
    );

    for path in [
        crate::artifacts::layout::canonical::CHANGES.to_owned(),
        crate::artifacts::layout::canonical::WORKFLOW.to_owned(),
        crate::artifacts::layout::canonical::OPEN_ITEMS.to_owned(),
        crate::artifacts::layout::canonical::RESOLVED_ITEMS.to_owned(),
        crate::artifacts::layout::canonical::DECISIONS.to_owned(),
    ] {
        assert!(
            changed
                .iter()
                .any(|changed_path| changed_path.starts_with(&path))
        );
    }
    let first_head = git_ok(&root, &["rev-parse", "HEAD"]);
    let second_run = run(&root).unwrap();
    assert!(
        second_run.is_empty(),
        "unexpected second-run changes: {second_run:?}"
    );
    assert_eq!(git_ok(&root, &["rev-parse", "HEAD"]), first_head);
    let _ = fs::remove_dir_all(root);
}
