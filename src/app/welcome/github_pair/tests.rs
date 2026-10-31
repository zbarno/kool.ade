use super::*;
use crate::core::managed_workspace::CodeRepositoryRegistration;

#[test]
fn visibility_flags_match_github_visibility_values() {
    assert_eq!(RepositoryVisibility::Public.flag(), "--public");
    assert_eq!(RepositoryVisibility::Private.flag(), "--private");
    assert_eq!(RepositoryVisibility::Internal.flag(), "--internal");
    assert_eq!(
        RepositoryVisibility::parse("public").unwrap(),
        RepositoryVisibility::Public
    );
    assert_eq!(
        RepositoryVisibility::parse("private").unwrap(),
        RepositoryVisibility::Private
    );
    assert_eq!(
        RepositoryVisibility::parse("internal").unwrap(),
        RepositoryVisibility::Internal
    );
}

#[test]
fn planning_visibility_must_match_the_code_repository() {
    let values = [
        RepositoryVisibility::Public,
        RepositoryVisibility::Private,
        RepositoryVisibility::Internal,
    ];
    for code in values {
        assert!(validate_pair_visibility("owner", "repo.packet", code, None).is_ok());
        for planning in values {
            let result = validate_pair_visibility("owner", "repo.packet", code, Some(planning));
            assert_eq!(result.is_ok(), code == planning);
        }
    }
}

#[test]
fn retry_reuses_the_local_planning_workspace_without_creating_another() {
    let _identity =
        crate::core::gitops::test_support::shield_with_synthetic_identity("planning-pair-retry");
    let root = tempfile::tempdir().unwrap();
    let manager = WorkspaceManager::new(root.path().join("operator"));
    let workspace = manager.create("Existing planning workspace").unwrap();
    let code = root.path().join("code");
    std::fs::create_dir(&code).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(&code)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/example/widget.git",
    ]);
    let workspace = manager
        .register_code_repository(
            workspace.id,
            CodeRepositoryRegistration {
                id: crate::core::managed_workspace::PAIR_CODE_REPO_ID.into(),
                role: "Primary code".into(),
                remote: Some("https://github.com/example/widget.git".into()),
                checkout: code,
            },
        )
        .unwrap();
    let selected = missing_planning_workspace(
        &manager,
        reusable_local_pair(&manager, "https://github.com/EXAMPLE/widget").unwrap(),
        "Do not create another workspace",
    )
    .unwrap();

    assert_eq!(selected.id, workspace.id);
    assert_eq!(manager.list_local().unwrap().len(), 1);
}

#[test]
fn paired_remote_identity_ignores_case_and_git_suffix() {
    assert!(same_github_remote(
        "https://github.com/Example/Project.packet.git",
        "https://github.com/example/project.packet"
    ));
    assert!(!same_github_remote(
        "https://github.com/example/project.git",
        "https://github.com/example/project.packet.git"
    ));
}

#[test]
fn derived_planning_name_obeys_github_length_limit() {
    assert!(validate_planning_name(&format!("{}.packet", "a".repeat(92))).is_ok());
    assert!(validate_planning_name(&format!("{}.packet", "a".repeat(93))).is_ok());
    assert!(validate_planning_name(&format!("{}.packet", "a".repeat(94))).is_err());
}
