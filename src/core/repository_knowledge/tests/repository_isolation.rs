use super::*;

#[test]
fn profiles_isolate_repository_identity_and_branch_ref() {
    let primary = Temp::new("isolation-primary");
    let secondary = Temp::new("isolation-secondary");
    let store_root = Temp::new("isolation-map");
    repository(primary.path(), &[("src/lib.rs", "pub fn primary() {}\n")]);
    repository(
        secondary.path(),
        &[("src/lib.rs", "pub fn secondary() {}\n")],
    );
    git(
        secondary.path(),
        &[
            "remote",
            "add",
            "origin",
            secondary.path().to_str().unwrap(),
        ],
    );
    git(primary.path(), &["branch", "feature/isolated", "main"]);
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        store_root.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    let mut repos = manifest();
    repos.repositories.push(Repository {
        id: "docs".into(),
        role: "Documentation source".into(),
        remote: secondary.path().to_string_lossy().into_owned(),
        display_name: None,
    });
    let mut checkouts = crate::core::project_repos::RepositoryCheckoutMapV1 {
        schema_version: 1,
        checkouts: Default::default(),
    };
    checkouts
        .checkouts
        .insert("docs".into(), secondary.path().to_path_buf());
    store
        .atomic_write(
            "config/repository-checkouts.json",
            &serde_json::to_vec(&checkouts).unwrap(),
        )
        .unwrap();

    let main = scan_profile(ProfileRequest {
        repository_id: "root",
        source_ref: "main",
        code_root: primary.path(),
        planning_store: &store,
        repositories: &repos,
        now_unix_ms: 300,
    })
    .unwrap();
    let feature = scan_profile(ProfileRequest {
        repository_id: "root",
        source_ref: "feature/isolated",
        code_root: primary.path(),
        planning_store: &store,
        repositories: &repos,
        now_unix_ms: 301,
    })
    .unwrap();
    let other_repository = scan_profile(ProfileRequest {
        repository_id: "docs",
        source_ref: "main",
        code_root: primary.path(),
        planning_store: &store,
        repositories: &repos,
        now_unix_ms: 302,
    })
    .unwrap();
    assert_ne!(main.key, feature.key);
    assert_eq!(main.key.commit_sha, feature.key.commit_sha);
    assert_ne!(main.key.repository_id, other_repository.key.repository_id);
    assert_ne!(main.key.commit_sha, other_repository.key.commit_sha);
}

#[test]
fn rejects_invalid_refs_and_repository_identity_mismatch() {
    let code = Temp::new("isolation");
    let store_root = Temp::new("isolation-store");
    repository(code.path(), &[("README.md", "sparse")]);
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        store_root.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    assert!(
        scan_profile(ProfileRequest {
            repository_id: "unknown",
            source_ref: "main",
            code_root: code.path(),
            planning_store: &store,
            repositories: &manifest(),
            now_unix_ms: 1
        })
        .is_err()
    );
    assert!(
        scan_profile(ProfileRequest {
            repository_id: "root",
            source_ref: "main~1",
            code_root: code.path(),
            planning_store: &store,
            repositories: &manifest(),
            now_unix_ms: 1
        })
        .is_err()
    );
}
