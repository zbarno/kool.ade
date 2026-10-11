use super::support::{TestDirectory, code_checkout, git_ok, registration};
use std::{fs, path::Path};

#[test]
fn remote_join_rejects_an_oversized_workspace_manifest_before_deserializing_it() {
    let temp = TestDirectory::new("remote-oversized-manifest");
    let manager = temp.manager("joiner");
    let remote = temp.path.join("remotes/oversized.git");
    let seed = temp.path.join("seed");
    fs::create_dir_all(&seed).unwrap();
    git_ok(
        None,
        &[
            "init",
            "--quiet",
            "--initial-branch=main",
            "--",
            path_str(&seed),
        ],
    );
    fs::write(
        seed.join("manifest.json"),
        vec![b'x'; super::super::manifest::MAX_MANIFEST_BYTES + 1],
    )
    .unwrap();
    git_ok(Some(&seed), &["add", "manifest.json"]);
    git_ok(
        Some(&seed),
        &[
            "-c",
            "user.name=Koolade Test",
            "-c",
            "user.email=koolade@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "oversized manifest fixture",
        ],
    );
    fs::create_dir_all(remote.parent().unwrap()).unwrap();
    git_ok(
        None,
        &["init", "--bare", "--quiet", "--", path_str(&remote)],
    );
    git_ok(Some(&seed), &["remote", "add", "origin", path_str(&remote)]);
    git_ok(Some(&seed), &["push", "--quiet", "origin", "main"]);
    git_ok(Some(&remote), &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let error = manager.join_remote(&remote.to_string_lossy()).unwrap_err();

    assert!(format!("{error:#}").contains("manifest exceeds the 64 KiB size limit"));
    assert!(manager.list_local().unwrap().is_empty());
}

#[test]
fn remote_join_rejects_an_invalid_project_manifest_before_creating_local_state() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "remote-join-invalid-project-manifest",
    );
    let temp = TestDirectory::new("remote-invalid-project-manifest");
    let owner = temp.manager("owner");
    let joiner = temp.manager("joiner");
    let workspace = owner.create("Invalid remote project manifest").unwrap();
    let manifest_path = workspace.store.layout().project_manifest();
    fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    fs::write(&manifest_path, b"{ invalid json\n").unwrap();
    git_ok(
        Some(&workspace.planning_repo),
        &[
            "add",
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
        ],
    );
    git_ok(
        Some(&workspace.planning_repo),
        &[
            "-c",
            "user.name=Koolade Test",
            "-c",
            "user.email=koolade@example.invalid",
            "commit",
            "-m",
            "invalid project manifest fixture",
        ],
    );

    let remote = temp.path.join("remotes/invalid-planning.git");
    fs::create_dir_all(remote.parent().unwrap()).unwrap();
    git_ok(
        None,
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            "--",
            path_str(&workspace.planning_repo),
            path_str(&remote),
        ],
    );

    let error = joiner.join_remote(&remote.to_string_lossy()).unwrap_err();

    let detail = format!("{error:#}");
    assert!(
        detail.contains("project repository manifest is invalid"),
        "{detail}"
    );
    assert!(!joiner.association_path(workspace.id).exists());
    assert!(!joiner.planning_repo_path(workspace.id).exists());
    assert!(joiner.list_local().unwrap().is_empty());
    let project_dir = joiner.project_dir(workspace.id);
    let projects = project_dir.parent().unwrap();
    assert!(
        fs::read_dir(projects)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(".join-")),
        "a rejected remote must remove its clone staging directory"
    );
}

#[test]
fn two_operators_join_a_published_workspace_from_remote_without_shared_local_paths() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "remote-join-two-operators",
    );
    let temp = TestDirectory::new("remote-join");
    let owner = temp.manager("operator-a");
    let joiner = temp.manager("operator-b");
    let workspace = owner.create("Shared planning identity").unwrap();
    let (checkout, _) = code_checkout(&temp.path, "application", false);
    owner
        .register_code_repository(workspace.id, registration("application", checkout, None))
        .unwrap();
    let mut manifest = workspace.manifest.clone();
    manifest.canonical_ref = "refs/heads/planning".into();
    fs::write(
        workspace.planning_repo.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    git_ok(Some(&workspace.planning_repo), &["add", "manifest.json"]);
    git_ok(
        Some(&workspace.planning_repo),
        &[
            "-c",
            "user.name=Koolade Test",
            "-c",
            "user.email=koolade@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "use non-default canonical branch",
        ],
    );
    git_ok(Some(&workspace.planning_repo), &["branch", "planning"]);
    git_ok(Some(&workspace.planning_repo), &["checkout", "planning"]);
    let remote = temp.path.join("remotes/planning.git");
    init_main_bare_remote(&remote);
    let remote_url = remote.to_string_lossy().into_owned();
    owner
        .set_planning_remote(workspace.id, &remote_url)
        .unwrap();
    git_ok(
        Some(&workspace.planning_repo),
        &["push", "--quiet", "origin", "main"],
    );
    git_ok(
        Some(&workspace.planning_repo),
        &["push", "--quiet", "origin", "planning"],
    );

    let joined = joiner
        .join_remote(&remote_url)
        .unwrap_or_else(|error| panic!("{error:#}"));
    let joined_again = joiner.join_remote(&remote_url).unwrap();
    let first = owner.open(workspace.id).unwrap();
    let second = joiner.open(workspace.id).unwrap();
    assert_eq!(joined.id, workspace.id);
    assert_eq!(joined_again.id, workspace.id);
    assert_eq!(first.manifest.workspace_id, second.manifest.workspace_id);
    assert_ne!(first.planning_repo, second.planning_repo);
    assert_eq!(first.association.code_checkouts.len(), 1);
    assert!(second.association.code_checkouts.is_empty());
    assert!(
        second
            .repositories
            .repositories
            .iter()
            .any(|repository| repository.id == "application")
    );
    assert_eq!(owner.list_local().unwrap().len(), 1);
    assert_eq!(joiner.list_local().unwrap().len(), 1);
    let leftover_staging = fs::read_dir(joiner.project_dir(workspace.id).parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".join-"));
    assert!(
        !leftover_staging,
        "reusing a joined workspace removes clone staging"
    );
}

#[test]
fn remote_join_keeps_staged_checkout_retryable_when_join_lock_is_busy() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "remote-join-lock-contention",
    );
    let temp = TestDirectory::new("remote-join-lock-contention");
    let owner = temp.manager("owner");
    let joiner = temp.manager("joiner");
    let workspace = owner.create("Join lock contention").unwrap();
    let remote = temp.path.join("remotes/planning.git");
    init_main_bare_remote(&remote);
    let remote_url = remote.to_string_lossy().into_owned();
    owner
        .set_planning_remote(workspace.id, &remote_url)
        .unwrap();
    owner.sync_planning(workspace.id).unwrap();

    let planning_repo = joiner.planning_repo_path(workspace.id);
    let projects = planning_repo.parent().unwrap().parent().unwrap();
    fs::create_dir_all(projects).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(projects.join(".join.lock"))
        .unwrap();
    lock.try_lock().unwrap();

    let error = joiner.join_remote(&remote_url).unwrap_err();
    assert!(format!("{error:#}").contains("workspace directory is locked"));
    assert!(!joiner.association_path(workspace.id).exists());
    assert!(!joiner.planning_repo_path(workspace.id).exists());
    assert!(
        fs::read_dir(projects)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(".join-")),
        "failed join removes its staging checkout"
    );

    drop(lock);
    assert_eq!(joiner.join_remote(&remote_url).unwrap().id, workspace.id);
}

#[test]
fn join_remote_rejects_a_remote_already_associated_to_a_different_workspace_id() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "remote-join-identity-collision",
    );
    let temp = TestDirectory::new("join-remote-identity-collision");
    let owner = temp.manager("owner");
    let operator = temp.manager("operator");
    let shared = owner.create("Shared identity").unwrap();
    let remote = temp.path.join("remotes/shared-planning.git");
    init_main_bare_remote(&remote);
    let remote_url = remote.to_string_lossy().into_owned();
    owner.set_planning_remote(shared.id, &remote_url).unwrap();
    owner.sync_planning(shared.id).unwrap();

    let conflicting_local = operator.create("Conflicting local identity").unwrap();
    operator
        .set_planning_remote(conflicting_local.id, &remote_url)
        .unwrap();

    let error = operator.join_remote(&remote_url).unwrap_err();

    let detail = format!("{error:#}");
    assert!(detail.contains("already associated locally"), "{detail}");
    assert!(
        detail.contains(&conflicting_local.id.to_string()),
        "{detail}"
    );
    assert!(operator.association_path(conflicting_local.id).is_file());
    assert!(!operator.association_path(shared.id).exists());
    let projects = conflicting_local
        .planning_repo
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    assert!(
        fs::read_dir(projects)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(".join-")),
        "failed join must remove its temporary clone"
    );
}

fn init_main_bare_remote(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    git_ok(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            "--",
            path_str(path),
        ],
    );
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}
