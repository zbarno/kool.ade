use super::*;

pub(super) fn seed_planning_metadata(store: &PlanningStore, code_remote: &str) {
    let root = &store.root;
    super::git(root, &["branch", "-M", "main"]);
    super::git(root, &["config", "user.name", "Koolade Test"]);
    super::git(root, &["config", "user.email", "koolade@example.test"]);
    super::git(
        root,
        &["commit", "--allow-empty", "-qm", "planning fixture"],
    );

    let mut workspace = crate::core::managed_workspace::WorkspaceManifest::new(store.project_id);
    workspace.canonical_ref = "refs/heads/main".into();
    std::fs::write(
        root.join(crate::artifacts::layout::store::MANIFEST),
        serde_json::to_vec_pretty(&workspace).unwrap(),
    )
    .unwrap();
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: code_remote.into(),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
            &serde_json::to_vec_pretty(&repositories).unwrap(),
        )
        .unwrap();
}
