use super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreMode};

fn planning_store(sandbox: &Sandbox, project_id: uuid::Uuid) -> PlanningStore {
    let root = sandbox.root.join("planning-repository");
    if !root.join(".git").is_dir() {
        fs::create_dir_all(&root).unwrap();
        sandbox.git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("README.md"), "Separate planning fixture\n").unwrap();
        sandbox.git(&root, &["add", "README.md"]);
        sandbox.git(&root, &["commit", "-qm", "planning fixture"]);
    }
    let ticket_path = root.join(&sandbox.ticket);
    if !ticket_path.is_file() {
        fs::create_dir_all(ticket_path.parent().unwrap()).unwrap();
        fs::copy(sandbox.repo.join(&sandbox.ticket), &ticket_path).unwrap();
    }
    PlanningStore::new(project_id, root, StoreMode::ManagedLocal)
}

fn reload_with_store(sandbox: &Sandbox, store: &PlanningStore) -> Implementation {
    let (progress, _updates) = mpsc::channel();
    super::super::super::run_with_planning_store_options(
        store,
        &sandbox.repo,
        &sandbox.repo,
        &sandbox.ticket,
        RunOptions {
            harness: &Fixture {
                mode: "complete",
                calls: Arc::new(AtomicUsize::new(0)),
            },
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::HoldForReview,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
            claim_lease: None,
        },
    )
    .unwrap()
}

fn migrate_identity(
    sandbox: &Sandbox,
    workspace_uid: Option<&str>,
    task_uid: Option<&str>,
) -> (uuid::Uuid, String, Implementation) {
    let initial = sandbox
        .run("complete", Arc::new(AtomicUsize::new(0)))
        .unwrap();
    let state_dir =
        crate::core::implementation::state_dir_for_task(&sandbox.repo, &sandbox.ticket, None)
            .unwrap();
    let mut state =
        crate::core::implementation::read_state_file(&state_dir.join("state.json")).unwrap();
    state.status = ImplementationStatus::Completed;
    state.workspace_uid = workspace_uid.map(str::to_owned);
    state.task_uid = task_uid.map(str::to_owned);
    let allocation = initial
        .task_repository_allocation_key
        .as_deref()
        .expect("implementation persists its task allocation key")
        .to_owned();
    crate::core::implementation::save(&state_dir, &state).unwrap();

    let ticket_path = sandbox.repo.join(&sandbox.ticket);
    let raw_ticket = fs::read_to_string(&ticket_path).unwrap();
    fs::write(
        &ticket_path,
        crate::domain::ArtifactIdentity::visible_markdown(&raw_ticket),
    )
    .unwrap();
    sandbox.git(&sandbox.repo, &["add", &sandbox.ticket]);
    sandbox.git(
        &sandbox.repo,
        &["commit", "-qm", "remove legacy ticket identity"],
    );
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "main"]);

    let project_id = uuid::Uuid::new_v4();
    let store = planning_store(sandbox, project_id);
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Planning root".into(),
            remote: sandbox.git(&sandbox.repo, &["remote", "get-url", "origin"]),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            "config/repositories.json",
            &serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    let migrated = reload_with_store(sandbox, &store);
    assert_eq!(migrated.ticket, initial.ticket);
    (project_id, allocation, migrated)
}

#[test]
fn missing_workspace_and_task_ids_migrate_to_persisted_stable_ids() {
    let sandbox = Sandbox::new();
    let (project_id, allocation, migrated) = migrate_identity(&sandbox, None, None);
    let expected_task_id = uuid::Uuid::new_v5(&project_id, allocation.as_bytes());
    assert_eq!(
        migrated.workspace_uid.as_deref(),
        Some(project_id.to_string().as_str())
    );
    assert_eq!(
        migrated.task_uid.as_deref(),
        Some(expected_task_id.to_string().as_str())
    );

    let state_dir =
        crate::core::implementation::state_dir_for_task(&sandbox.repo, &sandbox.ticket, None)
            .unwrap();
    let persisted =
        crate::core::implementation::read_state_file(&state_dir.join("state.json")).unwrap();
    assert_eq!(persisted.workspace_uid, migrated.workspace_uid);
    assert_eq!(persisted.task_uid, migrated.task_uid);

    let store = planning_store(&sandbox, project_id);
    let repeated = reload_with_store(&sandbox, &store);
    assert_eq!(repeated.workspace_uid, migrated.workspace_uid);
    assert_eq!(repeated.task_uid, migrated.task_uid);
}

#[test]
fn stale_workspace_and_invalid_task_ids_are_replaced() {
    let sandbox = Sandbox::new();
    let stale_workspace = uuid::Uuid::new_v4().to_string();
    let (_, allocation, migrated) =
        migrate_identity(&sandbox, Some(&stale_workspace), Some("not-a-uuid"));
    let project_id = uuid::Uuid::parse_str(migrated.workspace_uid.as_deref().unwrap()).unwrap();
    assert_ne!(project_id.to_string(), stale_workspace);
    assert_eq!(
        migrated.task_uid.as_deref(),
        Some(
            uuid::Uuid::new_v5(&project_id, allocation.as_bytes())
                .to_string()
                .as_str()
        )
    );
}
