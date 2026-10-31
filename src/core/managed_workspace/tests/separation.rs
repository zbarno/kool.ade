use super::support::{TestDirectory, git_ok};
use crate::core::managed_workspace::CodeRepositoryRegistration;

#[test]
// Production boundary: reopen a saved workspace association after a local path
// is redirected into its planning repository. The invariant is that shared
// planning and code checkouts are separate repositories; a helper-only path
// check cannot prove the persisted association is rejected. External services:
// none. Setup cost: one temporary workspace and two local Git repositories.
fn reopening_a_workspace_rejects_a_code_checkout_inside_the_planning_repository() {
    let temp = TestDirectory::new("separate-repositories");
    let manager = temp.manager("operator");
    let workspace = manager.create("Separated repositories").unwrap();
    let code = temp.path.join("code");
    std::fs::create_dir(&code).unwrap();
    git_ok(Some(&code), &["init", "--quiet", "--initial-branch=main"]);
    let mut workspace = manager
        .register_code_repository(
            workspace.id,
            CodeRepositoryRegistration {
                id: "root".into(),
                role: "Primary code".into(),
                remote: None,
                checkout: code,
            },
        )
        .unwrap();

    let embedded = workspace.planning_repo.join("embedded-code-checkout");
    std::fs::create_dir(&embedded).unwrap();
    git_ok(
        Some(&embedded),
        &["init", "--quiet", "--initial-branch=main"],
    );
    workspace.association.code_checkouts[0].path = embedded.canonicalize().unwrap();
    workspace
        .association
        .write(&manager.association_path(workspace.id))
        .unwrap();

    let error = manager.open(workspace.id).unwrap_err().to_string();
    assert!(error.contains("separate and non-nested"), "{error}");
}
