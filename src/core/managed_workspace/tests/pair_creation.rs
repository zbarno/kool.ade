use std::fs;

use crate::core::managed_workspace::SiblingPairSpec;

use super::support::TestDirectory;

fn spec_in(temp: &TestDirectory) -> SiblingPairSpec {
    let mut spec = SiblingPairSpec::new("Demo widget backend", "widget");
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
fn create_sibling_pair_materializes_and_registers_both_repositories() {
    let temp = TestDirectory::new("pair-full");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let manager = temp.manager("operator");
    let workspace = manager.create_sibling_pair(&spec_in(&temp)).unwrap();

    let code = temp.path.join("workspaces/widget");
    let planning = temp.path.join("workspaces/widget.packet");
    assert_eq!(workspace.planning_repo, planning.canonicalize().unwrap());
    assert_eq!(
        workspace.repositories.repositories.len(),
        1,
        "the pair registers exactly one code repository"
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
    assert!(
        code.join("README.md").is_file(),
        "placeholder README grounds the initial commit"
    );
    assert_eq!(
        branch_of(&planning),
        "main",
        "planning pair member follows the canonical branch"
    );
    assert!(
        planning.join("manifest.json").is_file()
            && planning.join("config/repositories.json").is_file(),
        "planning pair member carries the managed workspace scaffolding"
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

    let first = manager.create_sibling_pair(&spec).unwrap();
    let code = temp.path.join("workspaces/widget");
    let planning = temp.path.join("workspaces/widget.packet");
    let code_before = rev_count(&code);
    let planning_before = rev_count(&planning);

    let second = manager.create_sibling_pair(&spec).unwrap();
    assert_eq!(first.id, second.id, "the same pair maps to one workspace");
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
        .create_sibling_pair(&spec_in(&temp))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("already exists and is not a Git repository root"),
        "actual: {error}"
    );
    assert!(
        manager.list_local().unwrap().is_empty(),
        "no workspace association may record the failed attempt"
    );
}

#[test]
fn hostile_repository_names_are_rejected_before_touching_disk() {
    let temp = TestDirectory::new("pair-names");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let manager = temp.manager("operator");

    for name in ["../escape", "a b", ".dotfile", "", "way/too/many/slashes"] {
        let mut spec = spec_in(&temp);
        spec.code_repo_name = name.to_owned();
        spec.planning_repo_name = format!("{name}.packet");
        manager.create_sibling_pair(&spec).unwrap_err();
    }
    let entries = fs::read_dir(temp.path.join("workspaces")).unwrap().count();
    assert_eq!(
        entries, 0,
        "validation must happen before any disk mutation"
    );
}
