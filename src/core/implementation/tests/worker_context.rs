use super::*;

#[test]
fn disconnected_worker_reports_failure_instead_of_waiting_forever() {
    let mut controller = Controller::idle_fixture();
    assert!(controller.poll().is_none());
    controller._keep_alive.take();
    assert!(
        matches!(controller.poll(), Some(Event::Done(result)) if matches!(&*result, Err(message) if message.message.contains("stopped without a result")))
    );
}

#[test]
fn completed_worker_delivers_result_before_disconnect() {
    let controller = Controller::idle_fixture();
    controller
        ._keep_alive
        .as_ref()
        .unwrap()
        .send(Event::Done(Box::new(Err(Failure::other("original cause")))))
        .unwrap();
    assert!(
        matches!(controller.poll(), Some(Event::Done(result)) if matches!(&*result, Err(message) if message.message == "original cause"))
    );
}

#[test]
fn implementation_context_uses_only_frozen_affected_product_modules() {
    let root = std::env::temp_dir().join(format!(
        "koolade_implementation_context_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let batch = root.join("planning/tasks/feature");
    fs::create_dir_all(&batch).unwrap();
    let contract = crate::core::contract_snapshot::BatchContract {
        feature_id: "CHG-001".into(),
        feature_specification: "# Feature".into(),
        product_modules: [("current-capabilities".into(), "## 5. Relevant\n".into())].into(),
        repository_bases: Default::default(),
        configuration: String::new(),
    };
    fs::write(
        batch.join("contract.json"),
        serde_json::to_vec(&contract).unwrap(),
    )
    .unwrap();
    let context = scoped_product_context(&root, "planning/tasks/feature/001-task.md")
        .unwrap()
        .unwrap();
    assert!(context.contains("product:current-capabilities"));
    assert!(context.contains("## 5. Relevant"));
    assert!(!context.contains("product:overview"));
    let _ = fs::remove_dir_all(root);
}
#[test]
fn cross_repository_dependency_context_requires_merged_record() {
    let root = std::env::temp_dir().join(format!(
        "koolade_dependency_context_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let batch = root.join("planning/tasks/feature");
    fs::create_dir_all(&batch).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::nil(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let batch_uid = uuid::Uuid::new_v4().to_string();
    let prior = "planning/tasks/feature/001-api.md";
    let prior_text = "# API contract\n\nRepository: api\n\nThe endpoint returns a saved search.\n";
    let prior_markdown = crate::domain::ArtifactIdentity::preserve_markdown_with_parent(
        prior_text,
        None,
        "TASK-001",
        "API contract",
        Some(&batch_uid),
    )
    .unwrap();
    let prior_identity = crate::domain::ArtifactIdentity::from_markdown(&prior_markdown)
        .unwrap()
        .unwrap();
    let prior_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&prior_identity, "api", Vec::new()).unwrap();
    let prior_markdown =
        crate::artifacts::task_docs::embed(&prior_markdown, &prior_metadata).unwrap();
    store
        .atomic_write(prior, prior_markdown.as_bytes())
        .unwrap();
    let next = "planning/tasks/feature/002-web.md";
    let next_text =
        "# Web client\n\nRepository: web\n\n## Dependencies\n\n- [API contract](001-api.md)\n";
    let next_markdown = crate::domain::ArtifactIdentity::preserve_markdown_with_parent(
        next_text,
        None,
        "TASK-002",
        "Web client",
        Some(&batch_uid),
    )
    .unwrap();
    let next_identity = crate::domain::ArtifactIdentity::from_markdown(&next_markdown)
        .unwrap()
        .unwrap();
    let next_metadata = crate::artifacts::task_docs::TaskMetadata::new(
        &next_identity,
        "web",
        vec![prior_identity.uid.clone()],
    )
    .unwrap();
    let next_markdown = crate::artifacts::task_docs::embed(&next_markdown, &next_metadata).unwrap();
    store.atomic_write(next, next_markdown.as_bytes()).unwrap();
    let batch_identity = crate::domain::ArtifactIdentity {
        uid: batch_uid,
        display_id: "BATCH-001".into(),
        title: "Dependency fixture".into(),
        parent_uid: None,
    };
    let workflow = crate::core::workflow::Workflow {
        task_batches: vec![crate::core::workflow::TaskBatchRef {
            identity: Some(batch_identity),
            feature: "Dependency fixture".into(),
            directory: "planning/tasks/feature".into(),
            count: 2,
            created_at_ms: 0,
        }],
        ..Default::default()
    };
    crate::artifacts::task_docs::save_workflow(&store, &workflow).unwrap();
    assert!(completed_dependency_context(&root, next, next_text).is_err());
    let record: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": prior, "task_uid": prior_identity.uid, "ticket_text": prior_text, "branch": "koolade/api", "base": "main",
        "base_commit": "base", "task_repository": root, "task_repository_kind": "clone", "status": "completed", "detail": "",
        "pr_url": null, "verified_head": "merged", "merged_commit": "merged"
    }))
    .unwrap();
    let dir = state_dir(&root, prior).unwrap();
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("state.json"),
        crate::core::implementation::serialize_state(&record).unwrap(),
    )
    .unwrap();
    let context = completed_dependency_context(&root, next, next_text)
        .unwrap()
        .unwrap();
    assert!(context.contains("Repository: api"));
    assert!(context.contains("Merged commit: merged"));
    assert!(!context.contains("Repository: web"));
    let _ = fs::remove_dir_all(root);
}
