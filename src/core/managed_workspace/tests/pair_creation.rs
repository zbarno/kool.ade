use std::fs;

use crate::core::managed_workspace::NewProjectSpec;

use super::support::TestDirectory;

#[path = "pair_creation/recovery.rs"]
mod recovery;

fn spec_in(temp: &TestDirectory) -> NewProjectSpec {
    let mut spec = NewProjectSpec::new("Demo widget backend", "widget");
    spec.parent_dir = temp.path.join("workspaces");
    spec
}

fn rev_count(root: &std::path::Path) -> u32 {
    let out = super::support::git_ok(Some(root), &["rev-list", "--count", "HEAD"]);
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn branch_of(root: &std::path::Path) -> String {
    let out = super::support::git_ok(Some(root), &["symbolic-ref", "--short", "HEAD"]);
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn new_project_uses_a_code_checkout_and_managed_planning_repository() {
    let temp = TestDirectory::new("pair-full");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let manager = temp.manager("operator");
    let workspace = manager.create_new_project(&spec_in(&temp)).unwrap();

    let code = temp.path.join("workspaces/widget");
    let planning = workspace.planning_repo.clone();
    assert_eq!(
        planning,
        manager.project_dir(workspace.id).join("planning-repo")
    );
    assert_ne!(planning, code);
    assert!(
        !temp.path.join("workspaces/widget.packet").exists(),
        "planning stays under the managed workspace area"
    );
    assert_eq!(
        workspace.repositories.repositories.len(),
        1,
        "the project registers exactly one code repository"
    );
    let registered = &workspace.repositories.repositories[0];
    assert_eq!(registered.id, "root");
    let mapped = workspace
        .association
        .code_checkouts
        .iter()
        .find(|mapping| mapping.repository_id == "root")
        .expect("paired checkout maps the code repository");
    assert_eq!(mapped.path, code.canonicalize().unwrap());

    assert_eq!(branch_of(&code), "main");
    assert_eq!(
        rev_count(&code),
        1,
        "the code repository carries exactly one grounding commit"
    );
    assert!(
        code.join("README.md").is_file(),
        "placeholder README grounds the initial commit"
    );
    assert!(
        std::fs::read_to_string(code.join("README.md"))
            .unwrap()
            .starts_with("# Demo widget backend"),
        "the grounding README carries the project title"
    );
    let code_status = super::support::git_ok(Some(&code), &["status", "--porcelain"]);
    assert!(
        code_status.stdout.is_empty(),
        "code repository is committed clean"
    );
    assert_eq!(
        branch_of(&planning),
        "main",
        "planning repository follows the canonical branch"
    );
    assert!(
        planning.join("manifest.json").is_file()
            && planning.join("config/repositories.json").is_file(),
        "planning repository carries the managed workspace scaffolding"
    );
    let status = super::support::git_ok(Some(&planning), &["status", "--porcelain"]);
    assert!(
        status.stdout.is_empty(),
        "planning repository is committed clean"
    );
    assert_eq!(
        manager.open(workspace.id).unwrap().planning_repo,
        planning.canonicalize().unwrap(),
        "the workspace reopens from its association"
    );
}

#[test]
fn repeated_creations_are_idempotent() {
    let temp = TestDirectory::new("pair-idempotent");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let manager = temp.manager("operator");
    let spec = spec_in(&temp);

    let first = manager.create_new_project(&spec).unwrap();
    let code = temp.path.join("workspaces/widget");
    let planning = first.planning_repo.clone();
    let code_before = rev_count(&code);
    let planning_before = rev_count(&planning);

    let second = manager.create_new_project(&spec).unwrap();
    assert_eq!(first.id, second.id, "the same pair maps to one workspace");
    assert_eq!(first.planning_repo, second.planning_repo);
    assert_eq!(
        rev_count(&code),
        code_before,
        "code repository is not recommitted"
    );
    assert_eq!(
        rev_count(&planning),
        planning_before,
        "planning repository is not reseeding itself"
    );
}

#[test]
fn occupied_non_repo_destination_is_rejected_without_side_effects() {
    let temp = TestDirectory::new("pair-occupied");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    fs::write(temp.path.join("workspaces/widget"), "mine").unwrap();
    let manager = temp.manager("operator");

    let error = manager
        .create_new_project(&spec_in(&temp))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("will not adopt an existing checkout"),
        "actual: {error}"
    );
    assert!(
        manager.list_local().unwrap().is_empty(),
        "no workspace association may record the failed attempt"
    );
}

#[test]
fn similarly_named_local_folder_is_preserved_and_never_used_as_planning() {
    let temp = TestDirectory::new("pair-plan-occupied");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    fs::write(
        temp.path.join("workspaces/widget.packet"),
        "someone's notes",
    )
    .unwrap();
    let manager = temp.manager("operator");

    let workspace = manager.create_new_project(&spec_in(&temp)).unwrap();
    assert!(temp.path.join("workspaces/widget").join(".git").is_dir());
    assert_ne!(
        workspace.planning_repo,
        temp.path.join("workspaces/widget.packet")
    );
    assert_eq!(
        fs::read_to_string(temp.path.join("workspaces/widget.packet")).unwrap(),
        "someone's notes",
        "the occupying file must survive untouched"
    );
}

#[test]
fn foreign_repository_at_code_destination_is_refused_not_adopted() {
    let temp = TestDirectory::new("pair-foreign");
    fs::create_dir_all(temp.path.join("workspaces/widget")).unwrap();
    let code = temp.path.join("workspaces/widget");
    fs::write(code.join("NOTES.txt"), "precious operator work\n").unwrap();
    super::support::git_ok(
        Some(&code),
        &["init", "--quiet", "--initial-branch=main", "."],
    );
    super::support::git_ok(Some(&code), &["add", "-A"]);
    super::support::git_ok(
        Some(&code),
        &[
            "-c",
            "user.name=Fixture Operator",
            "-c",
            "user.email=fixture@example.com",
            "commit",
            "-q",
            "-m",
            "my own baseline",
        ],
    );
    let manager = temp.manager("operator");

    let error = manager
        .create_new_project(&spec_in(&temp))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("will not adopt an existing checkout"),
        "foreign repositories must be refused, not silently adopted (actual: {error})"
    );
    assert!(manager.list_local().unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(code.join("NOTES.txt")).unwrap(),
        "precious operator work\n",
        "foreign repository content must survive untouched"
    );
    assert!(
        !temp.path.join("workspaces/widget.packet").exists(),
        "no planning counterpart may appear beside a refused code repository"
    );
}

#[test]
fn empty_existing_git_checkout_is_refused_not_adopted() {
    let temp = TestDirectory::new("pair-empty-git");
    fs::create_dir_all(temp.path.join("workspaces/widget")).unwrap();
    let code = temp.path.join("workspaces/widget");
    super::support::git_ok(
        Some(&code),
        &["init", "--quiet", "--initial-branch=main", "."],
    );
    fs::write(
        code.join("README.md"),
        "# Demo widget backend\n\nCode repository maintained by Kool.ad/e.\n",
    )
    .unwrap();
    super::support::git_ok(Some(&code), &["add", "README.md"]);
    super::support::git_ok(
        Some(&code),
        &[
            "-c",
            "user.name=Fixture Operator",
            "-c",
            "user.email=fixture@example.com",
            "commit",
            "-q",
            "-m",
            "Kool.ad/e: initialize Demo widget backend code repository",
        ],
    );
    let manager = temp.manager("operator");

    let error = manager
        .create_new_project(&spec_in(&temp))
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("will not adopt an existing checkout"),
        "actual: {error}"
    );
    assert!(manager.list_local().unwrap().is_empty());
    assert!(code.join("README.md").is_file());
}
