use super::support::{TestDirectory, git_ok};

#[test]
fn same_named_workspaces_keep_distinct_ids_when_operator_joins_shared_remote() {
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("same-named-workspaces");
    let temp = TestDirectory::new("same-named-workspaces");
    let owner = temp.manager("operator-a");
    let joiner = temp.manager("operator-b");
    let owner_workspace = owner.create("Shared display name").unwrap();
    let local_workspace = joiner.create("Shared display name").unwrap();
    assert_ne!(owner_workspace.id, local_workspace.id);

    let remote = temp.path.join("remotes/planning.git");
    std::fs::create_dir_all(remote.parent().unwrap()).unwrap();
    git_ok(
        None,
        &["init", "--bare", "--quiet", "--", path_str(&remote)],
    );
    owner
        .set_planning_remote(owner_workspace.id, &remote.to_string_lossy())
        .unwrap();
    owner.sync_planning(owner_workspace.id).unwrap();
    assert!(
        remote.is_dir(),
        "the local bare planning remote must remain available"
    );
    assert!(
        !git_ok(None, &["--git-dir", path_str(&remote), "show-ref"])
            .stdout
            .is_empty(),
        "the owner must publish the workspace before the joiner connects"
    );
    git_ok(
        None,
        &[
            "--git-dir",
            path_str(&remote),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ],
    );

    let joined = joiner
        .join_remote(&remote.to_string_lossy())
        .unwrap_or_else(|error| panic!("{error:#}"));

    assert_eq!(joined.id, owner_workspace.id);
    assert_eq!(
        joiner.open(local_workspace.id).unwrap().id,
        local_workspace.id
    );
    let listed: std::collections::BTreeSet<_> = joiner
        .list_local()
        .unwrap()
        .into_iter()
        .map(|workspace| workspace.id)
        .collect();
    assert_eq!(
        listed,
        std::collections::BTreeSet::from([owner_workspace.id, local_workspace.id])
    );
}

fn path_str(path: &std::path::Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}
