use super::*;
use crate::artifacts::planning_store::StoreMode;

#[test]
fn separate_code_checkouts_load_the_same_injected_planning_root() {
    let first = git_fixture("store-first", Some("First Checkout"));
    let second = git_fixture("store-second", Some("Second Checkout"));
    let planning_root = mkrepo("external-planning");
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    store
        .atomic_write(
            crate::artifacts::planning_store::paths::PRODUCT_INDEX,
            b"# Shared project plan\n",
        )
        .unwrap();

    let first_state = PlannerState::load_with_store(&first, &store).unwrap();
    let second_state = PlannerState::load_with_store(&second, &store).unwrap();

    assert_ne!(first_state.repo_root, second_state.repo_root);
    assert_eq!(
        first_state.planning_store.root,
        second_state.planning_store.root
    );
    assert_eq!(
        first_state.spec_text.as_deref(),
        Some("# Shared project plan\n")
    );
    assert_eq!(first_state.spec_text, second_state.spec_text);
    assert_eq!(
        first_state.baseline_planning_revision,
        second_state.baseline_planning_revision
    );

    let _ = std::fs::remove_dir_all(first);
    let _ = std::fs::remove_dir_all(second);
    let _ = std::fs::remove_dir_all(planning_root);
}

#[test]
fn managed_store_bootstrap_preserves_the_connected_checkout_remote_identity() {
    let first = git_fixture("manifest-first", Some("First Checkout"));
    let second = git_fixture("manifest-second", Some("Second Checkout"));
    let remote = "https://github.com/example/first-project.git";
    let other_remote = "https://github.com/example/second-project.git";
    for (root, url) in [(&first, remote), (&second, other_remote)] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["remote", "add", "origin", url])
                .status()
                .unwrap()
                .success()
        );
    }
    let planning_root = mkrepo("manifest-store");
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );

    let mut first_state = PlannerState::load_with_store(&first, &store).unwrap();
    assert_eq!(first_state.repositories.repositories[0].remote, remote);
    first_state.bootstrap_missing().unwrap();
    let persisted = crate::core::project_repos::ProjectManifest::load(&store).unwrap();
    assert_eq!(persisted.repositories[0].remote, remote);
    assert!(
        persisted
            .target_if_available(&second, "root")
            .unwrap()
            .is_none(),
        "the saved root identity must reject a different code checkout"
    );

    let second_state = PlannerState::load_with_store(&second, &store).unwrap();
    assert_eq!(second_state.repositories.repositories[0].remote, remote);

    let _ = std::fs::remove_dir_all(first);
    let _ = std::fs::remove_dir_all(second);
    let _ = std::fs::remove_dir_all(planning_root);
}

#[test]
fn managed_store_keeps_create_edit_approval_and_generation_across_code_branch_switch() {
    use crate::core::validation::NormalizedTurn;
    use crate::domain::{ChangeStatus, ItemKind, OpenItem, Priority};

    let code = mkrepo("managed-lifecycle-code");
    git(&code, &["branch", "-M", "main"]);
    git(&code, &["config", "user.name", "Koolade Test"]);
    git(&code, &["config", "user.email", "koolade@example.test"]);
    std::fs::write(code.join("README.md"), "code checkout\n").unwrap();
    git(&code, &["add", "README.md"]);
    git(&code, &["commit", "-q", "-m", "fixture"]);
    git(&code, &["checkout", "-q", "-b", "feature/auth"]);

    let planning_root = mkrepo("managed-lifecycle-planning");
    git(&planning_root, &["config", "user.name", "Koolade Test"]);
    git(
        &planning_root,
        &["config", "user.email", "koolade@example.test"],
    );
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        StoreMode::ManagedLocal,
    );
    let mut state = PlannerState::load_with_store(&code, &store).unwrap();
    state.bootstrap_missing().unwrap();

    let feature = "# F1: Shared import context\n\n## Intent\n\nExpose stored reference content to planning turns.\n\n## Current Behavior\n\nImports are listed by path.\n\n## Desired Behavior\n\nManaged imports are available to the planner.\n\n## Scope\n\nPlanning context only.\n\n## Affected Product Areas\n\n`overview.md`\n\n## Requirements\n\n- FR-1 (MUST). Read managed imports from the selected planning store.\n\n## Decisions and Assumptions\n\n- **D-1:** Imported documents are reference data.\n\n## Acceptance Criteria\n\n1. The planner can use a managed imported document.\n";
    let mut item = OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        None,
        "Should imported text be inlined?".into(),
        "Workers do not read from the planning repository path.".into(),
    );
    item.feature_id = Some("F1".into());
    let turn = NormalizedTurn {
        assistant_message: "Created the shared import feature.".into(),
        change_summary: Some("Create shared import context".into()),
        spec_markdown: None,
        document_updates: vec![
            (
                "product:overview".into(),
                "# Overview\n\nManaged planning data remains stable across code branches.\n".into(),
            ),
            ("feature:F1".into(), feature.into()),
        ],
        planning_tasks: Vec::new(),
        change_status_updates: [("F1".into(), ChangeStatus::Ready)].into(),
        additional_planning_artifacts: Vec::new(),
        added: vec![item],
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    };
    let receipt = crate::core::apply::apply(&mut state, &turn).unwrap();
    assert!(receipt.spec_written && receipt.items_written);
    assert!(
        store
            .layout()
            .change_specification("F1-shared-import-context")
            .is_some()
    );

    let feature_path = crate::artifacts::product_docs::document_path(&store, "feature:F1").unwrap();
    let feature_text = String::from_utf8(store.read_planning_path(&feature_path).unwrap()).unwrap();
    let contract = crate::core::workflow::feature_contract(&feature_text);
    crate::core::workflow::approve_feature(&store, &mut state.workflow, "F1").unwrap();
    assert!(crate::core::workflow::feature_approved(
        &store,
        &state.workflow,
        "F1"
    ));

    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Shared import context (F1)".into(),
            goal: "Make imported evidence available to planner turns.".into(),
            target_users: "Project operators".into(),
            intended_outcome: "Managed imports inform planning.".into(),
            success_criteria: vec!["The imported content reaches the prompt.".into()],
            in_scope: vec!["Load text imports from the store.".into()],
            ready_for_tasks: true,
            ..Default::default()
        },
        specification: feature_text,
        feature_id: Some("F1".into()),
        contract: None,
        branch_targets: None,
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            title: "Load managed imports".into(),
            purpose: "Give the planner imported reference evidence.".into(),
            ..Default::default()
        }],
    };
    let generated =
        crate::artifacts::task_docs::write_batch(&store, &batch, &mut state.workflow).unwrap();
    assert!(!generated.is_empty());
    assert!(state.workflow.approved_features.contains_key("F1"));
    assert_eq!(state.workflow.task_batches.len(), 1);

    let before_switch = snapshot(&planning_root);
    git(&code, &["checkout", "-q", "main"]);
    let reopened = PlannerState::load_with_store(&code, &store).unwrap();
    assert_eq!(reopened.planning_store.root, store.root);
    assert_eq!(
        reopened
            .items
            .iter()
            .find(|item| item.id == "CLR-001")
            .and_then(|item| item.feature_id.as_deref()),
        Some("F1")
    );
    assert_eq!(
        reopened.active_feature.as_ref().map(|(id, _)| id.as_str()),
        Some("F1")
    );
    assert!(reopened.workflow.approved_features.contains_key("F1"));
    assert_eq!(reopened.workflow.task_batches.len(), 1);
    assert_eq!(
        snapshot(&planning_root),
        before_switch,
        "switching code branches must leave every shared planning byte unchanged"
    );
    assert!(!code.join(".koolade-packet").exists());
    assert!(!code.join("planning").exists());
    assert!(!contract.is_empty());

    let _ = std::fs::remove_dir_all(code);
    let _ = std::fs::remove_dir_all(planning_root);
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(
        root: &std::path::Path,
        current: &std::path::Path,
        files: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    visit(root, root, &mut files);
    files
}
