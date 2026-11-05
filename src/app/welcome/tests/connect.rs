use super::super::*;
use crate::error::AppError;
#[test]
fn connecting_missing_path_errors_friendly() {
    match attempt_connect("") {
        Ok(_) => panic!("expected connection error"),
        Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
    }
}

#[test]
fn connecting_nonexistent_path_errors_friendly() {
    match attempt_connect("/no/such/dir-xyz-123") {
        Ok(_) => panic!("expected connection error"),
        Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
    }
}

/// Production boundary: ordinary welcome connection resolves a code checkout
/// to private per-installation state. Store unit tests cannot prove the UI
/// connection path avoids a second Git repository and the code `.git` tree.
/// Uses a temporary local Git repo only, with low setup cost and no services.
#[test]
fn ordinary_code_checkout_connects_without_a_planning_repository() {
    let _env = super::helpers::EnvSandbox::enter("unregistered-code-checkout", false);
    let code = super::helpers::local_source_repo("unregistered-code-checkout");

    let project = attempt_connect(code.to_str().unwrap()).unwrap();
    assert_eq!(project.state.repo_root, code);
    assert!(
        project
            .state
            .planning_store
            .root
            .starts_with(crate::persistence::state_root())
    );
    assert!(
        project
            .state
            .planning_store
            .root
            .ends_with("operational-state")
    );
    let execution_state = crate::persistence::state_root()
        .join("projects")
        .join(
            project
                .state
                .planning_store
                .project_id
                .hyphenated()
                .to_string(),
        )
        .join("execution");
    assert!(execution_state.is_dir());
    assert!(!code.join(".git/koolade-queue.json").exists());
    assert!(!code.join(".koolade-packet").exists());
    assert!(!code.join("planning").exists());
    let status = std::process::Command::new("git")
        .args(["-C", code.to_str().unwrap(), "status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        status.stdout.is_empty(),
        "connection changed the code checkout"
    );
    let _ = std::fs::remove_dir_all(code);
}

/// Production boundary: persist an actual TaskIntent through the connected
/// project store and prove its sensitive request text stays outside code Git.
/// Store serialization tests cannot prove the welcome path selected an
/// external private root. Uses temporary local Git only; setup cost is low.
#[test]
fn persisted_task_intent_content_never_enters_code_repository() {
    let _env = super::helpers::EnvSandbox::enter("private-task-content", false);
    let code = super::helpers::local_source_repo("private-task-content");
    let project = attempt_connect(code.to_str().unwrap()).unwrap();
    let marker = "synthetic-private-task-marker-484";
    let mut task = crate::core::planning_work::Work::new(
        String::new(),
        "Private task request".into(),
        marker.into(),
        "Todo".into(),
    );
    task.kind = crate::core::planning_work::WorkKind::Feature;
    task.key = format!("task:{}", task.uid);
    task.task_intent_uid = Some(task.uid.clone());

    crate::core::planning_work::save(&project.state.planning_store, &[task.clone()]).unwrap();

    let code_root = code.canonicalize().unwrap();
    let private_root = project
        .state
        .planning_store
        .local_root()
        .canonicalize()
        .unwrap();
    assert!(!private_root.starts_with(&code_root));
    let task_record = project
        .state
        .planning_store
        .root
        .join("state/work")
        .join(format!("{}.json", task.uid));
    let task_record = task_record.canonicalize().unwrap();
    assert!(!task_record.starts_with(&code_root));
    let private_record = std::fs::read_to_string(task_record).unwrap();
    assert!(private_record.contains(marker));
    assert_eq!(crate::core::gitops::snapshot(&code).dirty, 0);
    let tracked_match = std::process::Command::new("git")
        .args([
            "-C",
            code.to_str().unwrap(),
            "grep",
            "-F",
            marker,
            "HEAD",
            "--",
        ])
        .output()
        .unwrap();
    assert!(tracked_match.status.code() == Some(1));
    let _ = std::fs::remove_dir_all(code);
}

#[test]
fn connection_rejects_private_application_data_inside_the_code_git_worktree() {
    let _env = super::helpers::EnvSandbox::enter("private-state-inside-git", false);
    let code = super::helpers::local_source_repo("private-state-inside-git");
    let private_home = code.join(".koolade-private");
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", &private_home);

    let error = match attempt_connect(code.to_str().unwrap()) {
        Ok(_) => panic!("private data rooted in Git must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(error, AppError::InvalidRepo { .. }));
    assert!(error.to_string().contains("outside Git worktrees"));
    assert!(!private_home.exists());
    let _ = std::fs::remove_dir_all(code);
}

/// Production boundary: connecting the same code repository under two
/// KOOLADE_HOME roots must leave each board independent. A path helper test
/// cannot prove `attempt_connect` maps both roots correctly. Uses one local
/// Git repo and two temp state roots, with no external services.
#[test]
fn same_code_repository_has_independent_operational_stores_per_installation() {
    let _env = super::helpers::EnvSandbox::enter("isolated-install-state", false);
    let code = super::helpers::local_source_repo("isolated-install-state");
    let first = attempt_connect(code.to_str().unwrap()).unwrap();
    let marker = "state/private-install-marker.json";
    first
        .state
        .planning_store
        .atomic_write(marker, br#"{"installation":"first"}"#)
        .unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let second_home = _env.home.join("other-installation");
    std::fs::create_dir_all(&second_home).unwrap();
    environment.set("KOOLADE_HOME", &second_home);

    let second = attempt_connect(code.to_str().unwrap()).unwrap();

    assert_eq!(
        first.state.planning_store.project_id,
        second.state.planning_store.project_id
    );
    assert_ne!(
        first.state.planning_store.root,
        second.state.planning_store.root
    );
    assert!(second.state.planning_store.read(marker).is_err());
    drop(environment);
    assert!(code.join(".git").is_dir());
    let _ = std::fs::remove_dir_all(code);
}

#[test]
fn connecting_with_open_board_items_queues_an_initial_manager_review() {
    let _env = super::helpers::EnvSandbox::enter("initial-manager-review", false);
    let code = super::helpers::local_source_repo("initial-manager-review");
    let slug = crate::persistence::project_slug(&code);
    let project_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, slug.as_bytes());
    let store = crate::artifacts::planning_store::PlanningStore::new(
        project_id,
        crate::persistence::project_dir(&project_id.hyphenated().to_string())
            .join("operational-state"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        "Choose a behavior".into(),
        "The implementation needs this decision".into(),
    );
    let (changes, checks) =
        crate::artifacts::items_io::record_changes(&store, &[item], &[]).unwrap();
    store
        .transaction_with_record_revisions(&changes, &checks)
        .unwrap();
    let project = attempt_connect(code.to_str().unwrap()).unwrap();

    assert_eq!(project.state.repo_root, code);
    assert_eq!(project.state.planning_store.root, store.root);
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| { event.contains("review existing board work") })
    );
    assert!(!project.state.repo_root.join("planning").exists());
    assert!(project.state.spec_text.is_none());
    assert!(
        !project
            .state
            .planning_store
            .layout()
            .product_index()
            .exists()
    );
    assert_eq!(crate::core::gitops::snapshot(&code).dirty, 0);
    let _ = std::fs::remove_dir_all(project.state.repo_root);
}

#[test]
fn connecting_from_a_code_subfolder_uses_its_root() {
    let _env = super::helpers::EnvSandbox::enter("code-subfolder", false);
    let root = super::helpers::local_source_repo("code-subfolder");
    let selected = root.join("src/nested");
    std::fs::create_dir_all(&selected).unwrap();

    let reopened = attempt_connect(selected.to_str().unwrap()).unwrap();

    assert_eq!(reopened.state.repo_root, root);
    assert!(!selected.join(".koolade-packet").exists());
    assert!(!root.join(".koolade-packet").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reconnect_recovers_unmapped_primary_git_fields_without_erasing_task_history() {
    let _env = super::helpers::EnvSandbox::enter("reconnect-legacy-git-intent", false);
    let code = super::helpers::local_source_repo("reconnect-legacy-git-intent");
    let project = attempt_connect(code.to_str().unwrap()).unwrap();
    let store = project.state.planning_store.clone();
    let work = crate::core::planning_work::Work::new(
        "task:legacy-reconnect".into(),
        "Existing task".into(),
        "Do not lose this task request".into(),
        "Saved conversation context".into(),
    );
    crate::core::planning_work::save(&store, std::slice::from_ref(&work)).unwrap();
    let path = format!("state/work/{}.json", work.uid);
    let mut value: serde_json::Value = serde_json::from_slice(&store.read(&path).unwrap()).unwrap();
    value["data"]["sourceBranch"] = "main".into();
    value["data"]["destinationBranch"] = "release".into();
    let original = serde_json::to_vec_pretty(&value).unwrap();
    store.atomic_write(&path, &original).unwrap();

    let reopened = attempt_connect(code.to_str().unwrap()).unwrap();
    let recovered = reopened
        .planning_work
        .iter()
        .find(|item| item.uid == work.uid)
        .unwrap();
    assert_eq!(recovered.request, work.request);
    assert_eq!(recovered.detail, work.detail);
    assert!(recovered.git_intent.is_none());
    assert!(recovered.git_intents.is_empty());
    assert!(recovered.source_branch.is_none());
    assert!(recovered.destination_branch.is_none());
    assert!(
        reopened
            .activity
            .pending
            .iter()
            .any(|message| message.contains("Recovered 1 legacy work record"))
    );
    let backup = format!(
        "state/recovery/unmapped-primary-git-intent/{}-revision-1.json",
        work.uid
    );
    assert_eq!(store.read(backup).unwrap(), original);
    assert_eq!(crate::core::gitops::snapshot(&code).dirty, 0);
    let _ = std::fs::remove_dir_all(code);
}
