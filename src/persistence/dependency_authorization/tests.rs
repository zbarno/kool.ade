use super::{
    Grant, SCHEMA_VERSION, matching_scope, project_id, save, set_private_file, store_path,
};
use crate::harness::{
    DependencyAuthorizationScope, DependencyDecision, DependencyFailureCategory, DependencyKind,
    DependencyNeed, DependencyRequest, DependencyRequestStatus, PackageEcosystem,
};
use std::{fs, path::Path, process::Command, sync::Mutex};

static STATE_ROOT_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn project_grants_match_independent_task_clones_and_migrate_legacy_store() {
    let _guard = STATE_ROOT_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!(
        "koolade-dependency-grant-clone-{}",
        uuid::Uuid::new_v4()
    ));
    let state_root = root.join("app-state");
    let project = root.join("project");
    fs::create_dir_all(&state_root).unwrap();
    init_git(&project);
    let _state_root = StateRootOverride::set(&state_root);

    let id = project_id(&project).unwrap();
    let task_clone = crate::persistence::project_dir(&id)
        .join("task-repositories")
        .join("repository-main")
        .join("TASK-1");
    init_git(&task_clone);
    assert_eq!(project_id(&task_clone).unwrap(), id);

    let first = request("TASK-1", "zod");
    save(&project, &first, DependencyAuthorizationScope::Project).unwrap();
    assert_eq!(
        matching_scope(&task_clone, "TASK-1", &first.need).unwrap(),
        Some(DependencyAuthorizationScope::Project)
    );

    let path = store_path(&id).unwrap();
    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "projectRoot": project.canonicalize().unwrap(),
        "grants": [serde_json::to_value(Grant {
            task_id: first.task_id.clone(),
            need: first.need.clone(),
            scope: DependencyAuthorizationScope::Project,
        }).unwrap()],
    });
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    set_private_file(&path).unwrap();
    assert_eq!(
        matching_scope(&task_clone, "TASK-1", &first.need).unwrap(),
        Some(DependencyAuthorizationScope::Project)
    );

    let second = request("TASK-2", "serde");
    save(&task_clone, &second, DependencyAuthorizationScope::Project).unwrap();
    let migrated: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(migrated["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(migrated["projectId"], id);
    assert_eq!(migrated["grants"].as_array().unwrap().len(), 2);
    assert_eq!(
        matching_scope(&project, "TASK-2", &second.need).unwrap(),
        Some(DependencyAuthorizationScope::Project)
    );

    let mut development = request("TASK-DEV", "zod");
    development.need.kind = DependencyKind::DevelopmentDependency;
    development.need.command = "npm install zod@1.0.0 --save-dev".into();
    save(
        &project,
        &development,
        DependencyAuthorizationScope::Project,
    )
    .unwrap();
    let mut production_command = development.need.clone();
    production_command.command = "npm install zod@1.0.0 --save-prod".into();
    assert_eq!(
        matching_scope(&project, "TASK-OTHER", &production_command).unwrap(),
        None
    );

    drop(_state_root);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_restore_grant_does_not_match_a_changed_lockfile_package_set() {
    let _guard = STATE_ROOT_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!(
        "koolade-dependency-restore-grant-{}",
        uuid::Uuid::new_v4()
    ));
    let state_root = root.join("app-state");
    let project = root.join("project");
    fs::create_dir_all(&state_root).unwrap();
    init_git(&project);
    let _state_root = StateRootOverride::set(&state_root);

    let mut approved = request("TASK-RESTORE", "unused");
    approved.need.kind = DependencyKind::ExistingRestore;
    approved.need.package = None;
    approved.need.version = None;
    approved.need.command = "npm ci --no-audit".into();
    approved.need.lockfile_identity = Some(format!("sha256:{}", "a".repeat(64)));
    save(&project, &approved, DependencyAuthorizationScope::Project).unwrap();

    assert_eq!(
        matching_scope(&project, &approved.task_id, &approved.need).unwrap(),
        Some(DependencyAuthorizationScope::Project)
    );
    let mut changed_lock = approved.need.clone();
    changed_lock.lockfile_identity = Some(format!("sha256:{}", "b".repeat(64)));
    assert_eq!(
        matching_scope(&project, &approved.task_id, &changed_lock).unwrap(),
        None,
        "a project restore grant must not authorize a changed lockfile"
    );

    drop(_state_root);
    fs::remove_dir_all(root).unwrap();
}

fn init_git(path: &Path) {
    fs::create_dir_all(path).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

fn request(task_id: &str, package: &str) -> DependencyRequest {
    DependencyRequest {
        id: format!("request-{task_id}-{package}"),
        task_id: task_id.to_owned(),
        need: DependencyNeed {
            ecosystem: PackageEcosystem::Npm,
            package: Some(package.to_owned()),
            version: Some("1.0.0".into()),
            source: Some("https://registry.npmjs.org".into()),
            command: format!("npm install {package}@1.0.0"),
            reason: "Exercise a task-scoped dependency authorization grant".into(),
            kind: DependencyKind::NewProjectDependency,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        },
        category: DependencyFailureCategory::DependencyNewPackageRequested,
        decision: DependencyDecision::UserAuthorizeForProject,
        rationale: "The project approved this exact package request.".into(),
        risk: "A new public dependency is added to the project.".into(),
        status: DependencyRequestStatus::Authorized,
        preparation: None,
    }
}

struct StateRootOverride {
    previous: Option<std::ffi::OsString>,
}

impl StateRootOverride {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("KOOLADE_HOME");
        unsafe { std::env::set_var("KOOLADE_HOME", path) };
        Self { previous }
    }
}

impl Drop for StateRootOverride {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => unsafe { std::env::set_var("KOOLADE_HOME", previous) },
            None => unsafe { std::env::remove_var("KOOLADE_HOME") },
        }
    }
}
