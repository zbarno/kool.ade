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
    let batch = root.join(".koolade-packet/planning/tasks/feature");
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
    let context =
        scoped_product_context(&root, ".koolade-packet/planning/tasks/feature/001-task.md")
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
    let batch = root.join(".koolade-packet/planning/tasks/feature");
    fs::create_dir_all(&batch).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let prior = ".koolade-packet/planning/tasks/feature/001-api.md";
    let prior_text = "# API contract\n\nRepository: api\n\nThe endpoint returns a saved search.\n";
    fs::write(root.join(prior), prior_text).unwrap();
    let next = ".koolade-packet/planning/tasks/feature/002-web.md";
    let next_text =
        "# Web client\n\nRepository: web\n\n## Dependencies\n\n- [API contract](001-api.md)\n";
    fs::write(root.join(next), next_text).unwrap();
    assert!(completed_dependency_context(&root, next, next_text).is_err());
    let record: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": prior, "ticket_text": prior_text, "branch": "koolade/api", "base": "main",
        "base_commit": "base", "worktree": root, "status": "completed", "detail": "",
        "pr_url": null, "verified_head": "merged", "merged_commit": "merged"
    }))
    .unwrap();
    let dir = state_dir(&root, prior).unwrap();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state.json"), serde_json::to_vec(&record).unwrap()).unwrap();
    let context = completed_dependency_context(&root, next, next_text)
        .unwrap()
        .unwrap();
    assert!(context.contains("Repository: api"));
    assert!(context.contains("Merged commit: merged"));
    assert!(!context.contains("Repository: web"));
    let _ = fs::remove_dir_all(root);
}
