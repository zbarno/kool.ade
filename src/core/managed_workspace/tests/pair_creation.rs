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
fn occupied_planning_destination_leaves_no_half_pair_behind() {
    let temp = TestDirectory::new("pair-plan-occupied");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    fs::write(
        temp.path.join("workspaces/widget.packet"),
        "someone's notes",
    )
    .unwrap();
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
        !temp.path.join("workspaces/widget").exists(),
        "the code repository must not exist when the planning destination is refused"
    );
    assert!(manager.list_local().unwrap().is_empty());
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
        .create_sibling_pair(&spec_in(&temp))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("did not create for this project"),
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
fn explicit_planning_repository_name_is_honored_and_resumable() {
    let temp = TestDirectory::new("pair-named-plan");
    fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let mut spec = spec_in(&temp);
    spec.planning_repo_name = "bay-of-packets".to_owned();
    spec.parent_dir = temp.path.join("workspaces");
    let manager = temp.manager("operator");

    manager.create_sibling_pair(&spec).unwrap();
    let planning = temp.path.join("workspaces/bay-of-packets");
    assert!(planning.join("manifest.json").is_file());
    assert!(
        !temp.path.join("workspaces/widget.packet").exists(),
        "the explicit name replaces the conventional sibling"
    );
    let before = rev_count(&planning);
    manager.create_sibling_pair(&spec).unwrap();
    assert_eq!(rev_count(&planning), before);
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

#[test]
fn existing_code_repository_is_paired_untouched() {
    let temp = TestDirectory::new("pair-ext");
    let code = temp.path.join("workspaces/ext-app");
    fs::create_dir_all(&code).unwrap();
    fs::write(code.join("src.txt"), "operator's code\n").unwrap();
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
            "their commit one",
        ],
    );
    fs::write(code.join("more.txt"), "more\n").unwrap();
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
            "their commit two",
        ],
    );

    let mut spec = spec_in(&temp);
    spec.code_repo_name = "ext-app".to_owned();
    spec.planning_repo_name = "ext-app.packet".to_owned();
    spec.code_already_exists = true;
    let manager = temp.manager("operator");
    let workspace = manager.create_sibling_pair(&spec).unwrap();

    assert_eq!(
        rev_count(&code),
        2,
        "the operator's history must survive intact"
    );
    let subject = super::support::git_ok(Some(&code), &["log", "-1", "--pretty=%s"]);
    assert_eq!(
        String::from_utf8_lossy(&subject.stdout).trim(),
        "their commit two"
    );
    assert_eq!(
        fs::read_to_string(code.join("src.txt")).unwrap(),
        "operator's code\n"
    );
    let planning = temp.path.join("workspaces/ext-app.packet");
    assert!(planning.join("manifest.json").is_file());
    let reopened = manager.open(workspace.id).unwrap();
    let mapped = reopened
        .association
        .code_checkouts
        .iter()
        .find(|mapping| mapping.repository_id == "root")
        .expect("root checkout registered");
    assert_eq!(mapped.path, code.canonicalize().unwrap());
}

#[test]
fn existing_flag_requires_a_real_repository_root() {
    let temp = TestDirectory::new("pair-ext-invalid");
    let plain = temp.path.join("workspaces/plain-old");
    fs::create_dir_all(&plain).unwrap();
    let mut spec = spec_in(&temp);
    spec.code_repo_name = "plain-old".to_owned();
    spec.code_already_exists = true;
    let manager = temp.manager("operator");
    let error = manager.create_sibling_pair(&spec).unwrap_err().to_string();
    assert!(
        error.contains("is not a Git repository root"),
        "actual: {error}"
    );
    assert!(manager.list_local().unwrap().is_empty());
    assert!(!temp.path.join("workspaces/plain-old.packet").exists());
}
