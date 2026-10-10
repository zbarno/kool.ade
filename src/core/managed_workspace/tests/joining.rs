use std::fs;

use crate::core::managed_workspace::WorkspaceManager;

use super::support::{TestDirectory, code_checkout, git_ok, registration};

#[test]
fn two_operator_homes_join_one_workspace_with_separate_checkout_paths() {
    let temp = TestDirectory::new("multi-operator");
    let first = temp.manager("operator-a");
    let second = temp.manager("operator-b");
    let created = first.create("Shared planning repo").unwrap();
    let (checkout_a, remote) = code_checkout(&temp.path, "alpha-a", true);
    let shared_remote = remote.as_deref().expect("bare remote URL").to_owned();
    let (checkout_b, _) = code_checkout(&temp.path, "alpha-b", true);
    super::support::git_ok(
        Some(&checkout_b),
        &["remote", "set-url", "origin", &shared_remote],
    );

    first
        .register_code_repository(
            created.id,
            registration("alpha", checkout_a.clone(), remote),
        )
        .unwrap();
    let joined = second.join(&created.planning_repo).unwrap();
    second
        .register_code_repository(
            created.id,
            registration("alpha", checkout_b.clone(), Some(shared_remote)),
        )
        .unwrap();

    let first_open = first.open(created.id).unwrap();
    let second_open = second.open(created.id).unwrap();
    assert_eq!(joined.id, created.id);
    assert_eq!(first_open.id, second_open.id);
    assert_eq!(
        first_open.association.code_checkouts[0].path,
        checkout_a.canonicalize().unwrap()
    );
    assert_eq!(
        second_open.association.code_checkouts[0].path,
        checkout_b.canonicalize().unwrap()
    );
    assert_eq!(
        first_open.planning_repo, second_open.planning_repo,
        "both operator-local associations resolve to the same planning repository"
    );
}

#[test]
fn unrelated_existing_git_repository_is_rejected_without_modification() {
    let temp = TestDirectory::new("unrelated-repo");
    let manager = temp.manager("operator");
    let unrelated = temp.path.join("unrelated");
    fs::create_dir_all(&unrelated).unwrap();
    git_ok(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=main",
            "--",
            unrelated.to_str().unwrap(),
        ],
    );
    fs::write(unrelated.join("keep.txt"), "do not replace").unwrap();
    let head_before = git_ok(Some(&unrelated), &["rev-parse", "--show-toplevel"]).stdout;

    let error = manager.join(&unrelated).unwrap_err();

    assert!(format!("{error:#}").contains("planning manifest is invalid"));
    assert_eq!(
        fs::read_to_string(unrelated.join("keep.txt")).unwrap(),
        "do not replace"
    );
    assert_eq!(
        git_ok(Some(&unrelated), &["rev-parse", "--show-toplevel"]).stdout,
        head_before
    );
    assert!(
        fs::read_dir(manager.home_path_for_test().join("projects"))
            .map(|entries| entries.count() == 0)
            .unwrap_or(true)
    );
}

#[test]
fn join_reuses_an_existing_identity_and_rejects_a_conflicting_local_mapping() {
    let temp = TestDirectory::new("join-identity");
    let owner = temp.manager("owner");
    let operator = temp.manager("operator");
    let workspace = owner.create("Identity is stable").unwrap();
    let first_join = operator.join(&workspace.planning_repo).unwrap();
    let second_join = operator.join(&workspace.planning_repo).unwrap();
    assert_eq!(first_join.id, second_join.id);

    let mut association: serde_json::Value =
        serde_json::from_slice(&fs::read(operator.association_path(workspace.id)).unwrap())
            .unwrap();
    association["workspaceId"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
    fs::write(
        operator.association_path(workspace.id),
        serde_json::to_vec_pretty(&association).unwrap(),
    )
    .unwrap();

    let error = operator.join(&workspace.planning_repo).unwrap_err();
    assert!(format!("{error:#}").contains("association identity mismatch"));
}

#[test]
fn join_rejects_a_planning_path_already_mapped_to_another_identity() {
    let temp = TestDirectory::new("join-path-collision");
    let owner = temp.manager("owner");
    let operator = temp.manager("operator");
    let workspace = owner.create("One path, one identity").unwrap();
    operator.join(&workspace.planning_repo).unwrap();

    let manifest_path = workspace.planning_repo.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let second_id = uuid::Uuid::new_v4();
    manifest["workspaceId"] = serde_json::Value::String(second_id.hyphenated().to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = operator.join(&workspace.planning_repo).unwrap_err();
    assert!(
        format!("{error:#}").contains("already associated with workspace"),
        "{error:#}"
    );
    assert!(
        !operator.association_path(second_id).exists(),
        "a conflicting identity must not gain a local association"
    );
    assert!(operator.association_path(workspace.id).is_file());
}

trait ManagerHome {
    fn home_path_for_test(&self) -> &std::path::Path;
}

impl ManagerHome for WorkspaceManager {
    fn home_path_for_test(&self) -> &std::path::Path {
        &self.home
    }
}
