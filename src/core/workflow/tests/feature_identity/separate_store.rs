use super::*;
use crate::artifacts::planning_store::PlanningStore;

#[test]
fn published_specs_stay_in_code_while_proposals_and_task_generation_stay_private() {
    let root = std::env::temp_dir().join(format!(
        "koolade-workflow-local-operations-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let code_root = root.join("code");
    let local_root = root.join("application-data");
    std::fs::create_dir_all(&code_root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&code_root)
            .status()
            .unwrap()
            .success()
    );
    for (key, value) in [
        ("user.name", "Koolade Test"),
        ("user.email", "koolade@example.test"),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(["config", key, value])
                .current_dir(&code_root)
                .status()
                .unwrap()
                .success()
        );
    }
    let overview = code_root.join("docs/koolade/product/overview.md");
    let feature_index = code_root.join("docs/koolade/features/AGENTS.md");
    std::fs::create_dir_all(overview.parent().unwrap()).unwrap();
    std::fs::create_dir_all(feature_index.parent().unwrap()).unwrap();
    std::fs::write(&overview, "# Overview\n\nPublished product truth.\n").unwrap();
    std::fs::write(&feature_index, "# Navigation\n").unwrap();
    for path in [
        "docs/koolade/product/overview.md",
        "docs/koolade/features/AGENTS.md",
    ] {
        assert!(
            std::process::Command::new("git")
                .args(["add", path])
                .current_dir(&code_root)
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        std::process::Command::new("git")
            .args(["commit", "-qm", "Publish synthetic product documentation"])
            .current_dir(&code_root)
            .status()
            .unwrap()
            .success()
    );
    let published_head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&code_root)
        .output()
        .unwrap();
    let published_head = String::from_utf8(published_head.stdout).unwrap();
    let store = PlanningStore::for_code_repository(uuid::Uuid::new_v4(), &local_root, &code_root);
    let mut initial_state = PlannerState::load_with_store(&code_root, &store).unwrap();
    initial_state.bootstrap_missing().unwrap();
    // Use the snapshot's resolved published pin when binding private proposals.
    let store = initial_state.planning_store.clone();

    let spec = "# Overview\n\nA private product proposal.\n";
    let updates = vec![("product:overview".to_owned(), spec.to_owned())];
    let proposal = crate::core::apply::private_document_draft_changes(&store, &updates).unwrap();
    let revision = store.revision().unwrap();
    store
        .transaction_with_revision(&proposal, Some(&revision))
        .unwrap();
    let feature = feature_document("local-operations", &store, "CHG-097", "Saved searches");
    let mut workflow = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    workflow.approved_features.insert(
        "CHG-097".into(),
        crate::core::workflow::feature_contract(&feature),
    );
    assert!(!crate::core::workflow::feature_approved(
        &store, &workflow, "CHG-097"
    ));
    crate::core::lifecycle::test_support::approve_and_review_feature(
        &store,
        &mut workflow,
        "CHG-097",
        &feature,
    )
    .unwrap();
    assert!(crate::core::workflow::feature_approved(
        &store, &workflow, "CHG-097"
    ));

    let mut task_brief = brief();
    task_brief.feature_name = "Saved searches (CHG-097)".into();
    let mut task_story = story();
    task_story.source_sha = Some(published_head.trim().to_owned());
    let batch = TaskBatch {
        brief: task_brief,
        specification: feature.clone(),
        feature_id: Some("CHG-097".into()),
        contract: None,
        repository_git_intents: std::collections::BTreeMap::from([(
            "root".into(),
            crate::test_support::approved_git_intent("root", "main", published_head.trim(), "main"),
        )]),
        task_routing: TaskRoutingSnapshot::default(),
        stories: vec![task_story],
    };
    let generated =
        crate::artifacts::task_docs::write_batch(&store, &batch, &mut workflow).unwrap();

    assert!(
        generated
            .iter()
            .any(|path| path.starts_with("planning/tasks/"))
    );
    assert!(
        generated
            .iter()
            .any(|path| path.starts_with("state/workflow/"))
    );
    assert!(
        crate::artifacts::product_docs::current_document(&store, "product:overview")
            .unwrap()
            .unwrap()
            .contains("A private product proposal")
    );
    assert!(
        crate::artifacts::product_docs::render_product(&store)
            .unwrap()
            .unwrap()
            .contains("Published product truth")
    );
    assert!(store.root.join("state/spec_drafts/current.json").is_file());
    assert!(!store.root.join(".git").exists());
    assert!(!store.layout().product_root().exists());
    assert!(store.layout().tasks_root().is_dir());
    assert!(!store.layout().workflow_state().exists());
    assert!(
        !store
            .list_files(crate::artifacts::planning_store::paths::WORKFLOW_RECORDS)
            .unwrap()
            .is_empty()
    );
    let board = crate::artifacts::task_docs::load_board(&store, &workflow);
    let task_documents = board
        .iter()
        .filter(|document| document.path.ends_with(".md") && document.metadata.is_some())
        .collect::<Vec<_>>();
    assert!(!task_documents.is_empty());
    assert!(task_documents.iter().all(|document| {
        document
            .task_state
            .as_ref()
            .is_some_and(|state| state.status == crate::core::planning_work::WorkStatus::Todo)
    }));
    assert_eq!(
        store
            .list_files(crate::artifacts::planning_store::paths::TASK_STATES)
            .unwrap()
            .len(),
        task_documents.len()
    );
    let head_after = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&code_root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(head_after.stdout).unwrap(),
        published_head
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&code_root)
        .output()
        .unwrap();
    assert!(status.stdout.is_empty());

    let _ = std::fs::remove_dir_all(root);
}
