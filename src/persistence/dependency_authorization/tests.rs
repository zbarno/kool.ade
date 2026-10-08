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

/// Child entry point used by the process-concurrency test. Each worker writes
/// distinct grants through the public API rather than sharing in-process locks.
#[test]
fn grant_writer_child_process() {
    let Ok(project) = std::env::var("KOOLADE_TEST_AUTH_CHILD_PROJECT") else {
        return;
    };
    let worker = std::env::var("KOOLADE_TEST_AUTH_CHILD_WORKER").unwrap();
    for index in 0..20 {
        let task = format!("TASK-{worker}-{index}");
        let package = format!("package-{worker}-{index}");
        save(
            Path::new(&project),
            &request(&task, &package),
            DependencyAuthorizationScope::Project,
        )
        .unwrap();
    }
}

#[test]
fn concurrent_processes_do_not_lose_project_grants() {
    let _guard = STATE_ROOT_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!(
        "koolade-authorization-multiprocess-{}",
        uuid::Uuid::new_v4()
    ));
    let state_root = root.join("app-state");
    let project = root.join("project");
    fs::create_dir_all(&state_root).unwrap();
    init_git(&project);
    let _override = StateRootOverride::set(&state_root);
    let project_id = project_id(&project).unwrap();

    let children = (0..4)
        .map(|worker| {
            Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("persistence::dependency_authorization::tests::grant_writer_child_process")
                .arg("--nocapture")
                .env("KOOLADE_HOME", &state_root)
                .env("KOOLADE_TEST_AUTH_CHILD_PROJECT", &project)
                .env("KOOLADE_TEST_AUTH_CHILD_WORKER", worker.to_string())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (index, child) in children.into_iter().enumerate() {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "writer {index} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("1 passed"),
            "child test was not executed"
        );
    }
    let path = store_path(&project_id).unwrap();
    let file: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(file["grants"].as_array().unwrap().len(), 80);
    for worker in 0..4 {
        for index in 0..20 {
            let package = format!("package-{worker}-{index}");
            let need = request(&format!("TASK-{worker}-{index}"), &package);
            assert_eq!(
                matching_scope(&project, &need.task_id, &need.need).unwrap(),
                Some(DependencyAuthorizationScope::Project)
            );
        }
    }

    drop(_override);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn existing_symlinked_authorization_lock_is_rejected() {
    use std::os::unix::fs::symlink;
    let _guard = STATE_ROOT_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!(
        "koolade-authorization-lock-symlink-{}",
        uuid::Uuid::new_v4()
    ));
    let state_root = root.join("app-state");
    let project = root.join("project");
    fs::create_dir_all(&state_root).unwrap();
    init_git(&project);
    let _override = StateRootOverride::set(&state_root);
    let id = project_id(&project).unwrap();
    let store = store_path(&id).unwrap();
    let unrelated = root.join("do-not-overwrite");
    fs::write(&unrelated, "keep").unwrap();
    symlink(&unrelated, store.with_extension("lock")).unwrap();
    let result = save(
        &project,
        &request("TASK-BLOCKED", "zod"),
        DependencyAuthorizationScope::Project,
    );
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep");
    drop(_override);
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
