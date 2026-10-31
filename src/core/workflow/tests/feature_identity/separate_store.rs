use super::*;
use crate::artifacts::planning_store::PlanningStore;

#[test]
fn separate_store_routes_spec_edit_approval_and_task_generation_away_from_code() {
    let root = std::env::temp_dir().join(format!(
        "koolade-workflow-separated-store-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let code_root = root.join("code");
    let planning_root = root.join("planning");
    for repository in [&code_root, &planning_root] {
        std::fs::create_dir_all(repository).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(repository)
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
                    .current_dir(repository)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(repository.join("README.md"), "fixture\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "README.md"])
                .current_dir(repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "-qm", "fixture"])
                .current_dir(repository)
                .status()
                .unwrap()
                .success()
        );
    }
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    let mut initial_state = PlannerState::load_with_store(&code_root, &store).unwrap();
    initial_state.bootstrap_missing().unwrap();

    let spec = "# Overview\n\nUpdated through the injected planning store.\n";
    let updates = vec![("product:overview".to_owned(), spec.to_owned())];
    let layout = store.layout();
    let module_path =
        crate::artifacts::product_docs::document_path_for_update(&store, &updates[0].0, spec)
            .unwrap();
    let manifest = crate::artifacts::product_docs::updated_manifest(&store, &updates).unwrap();
    let index = crate::artifacts::product_docs::refreshed_index(&store, &updates).unwrap();
    let module_relative = module_path
        .strip_prefix(layout.root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let changes = vec![
        (module_relative, spec.as_bytes().to_vec()),
        (
            crate::artifacts::planning_store::paths::PRODUCT_MANIFEST.to_owned(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        ),
        (
            crate::artifacts::planning_store::paths::PRODUCT_INDEX.to_owned(),
            index.into_bytes(),
        ),
    ];
    let revision = store.revision().unwrap();
    store.transaction(&changes, Some(&revision)).unwrap();
    assert_eq!(std::fs::read(module_path).unwrap(), spec.as_bytes());

    let feature = feature_document(
        "store-compatibility",
        &planning_root,
        "CHG-097",
        "Saved searches",
    );
    for (key, value) in [
        ("user.name", "Koolade Test"),
        ("user.email", "koolade@example.test"),
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&code_root)
                .args(["config", key, value])
                .status()
                .unwrap()
                .success()
        );
    }
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
    let batch = TaskBatch {
        brief: task_brief,
        specification: feature.clone(),
        feature_id: Some("CHG-097".into()),
        contract: None,
        branch_targets: None,
        task_routing: TaskRoutingSnapshot::default(),
        stories: vec![story()],
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
    assert_eq!(
        std::fs::read(layout.product_root().join("overview.md")).unwrap(),
        spec.as_bytes()
    );
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
    assert!(!code_root.join("planning").exists());

    let _ = std::fs::remove_dir_all(root);
}
