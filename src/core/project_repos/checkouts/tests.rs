use super::*;
use crate::core::project_repos::Repository;

fn git_repo(path: &std::path::Path, remote: &str) {
    std::fs::create_dir_all(path).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args(["remote", "add", "origin", remote])
            .current_dir(path)
            .status()
            .unwrap()
            .success()
    );
}

fn manifest() -> ProjectManifest {
    ProjectManifest {
        repositories: vec![
            Repository {
                id: "root".into(),
                role: "Primary code".into(),
                remote: "https://example.test/team/product.git".into(),
                display_name: None,
            },
            Repository {
                id: "api".into(),
                role: "Backend API".into(),
                remote: "https://example.test/team/api.git".into(),
                display_name: None,
            },
        ],
    }
}

fn fixture() -> (
    tempfile::TempDir,
    PlanningStore,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let root = tempfile::tempdir().unwrap();
    let code = root.path().join("product");
    let api = root.path().join("api");
    git_repo(&code, "https://example.test/team/product.git");
    git_repo(&api, "https://example.test/team/api.git");
    let store = PlanningStore::for_code_repository(
        uuid::Uuid::new_v4(),
        root.path().join("private"),
        &code,
    );
    (root, store, code, api)
}

#[test]
// Production boundary: registration persists the user-selected checkout
// outside source Git, and resolution uses the revision-fenced association.
// Git subprocesses and temporary local repositories are the only external
// dependency; setup cost is low.
fn explicit_secondary_checkout_is_private_and_revision_fenced() {
    let (root, store, code, api) = fixture();
    let primary_only = ProjectManifest {
        repositories: vec![manifest().repositories.remove(0)],
    };
    let (registered_manifest, _) = register_repository_checkout(
        &store,
        &primary_only,
        &code,
        RepositoryCheckoutRegistration {
            id: "api",
            role: "API",
            remote: "https://example.test/team/api.git",
            path: &api,
        },
        &store.revision().unwrap(),
    )
    .unwrap();
    assert_eq!(
        registered_manifest.target(&store, &code, "api").unwrap(),
        api.canonicalize().unwrap()
    );
    assert_eq!(
        registered_manifest.target(&store, &code, "root").unwrap(),
        code
    );
    assert_eq!(
        store.published_repository_root.as_deref(),
        Some(code.as_path())
    );
    assert!(
        ProjectManifest::load(&store)
            .unwrap()
            .repositories
            .iter()
            .any(|repo| repo.id == "api")
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&code)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(status.stdout.is_empty());
    assert!(store.local_root().starts_with(root.path()));

    let stale = store.revision().unwrap();
    store.atomic_write("config/other.json", b"{}").unwrap();
    assert!(connect_checkout(&store, &registered_manifest, &code, "api", &api, &stale).is_err());
}

#[test]
fn checkout_association_rejects_wrong_origin_and_primary_alias() {
    let (_root, store, code, api) = fixture();
    assert!(
        std::process::Command::new("git")
            .args([
                "remote",
                "set-url",
                "origin",
                "https://example.test/team/wrong.git",
            ])
            .current_dir(&api)
            .status()
            .unwrap()
            .success()
    );
    let revision = store.revision().unwrap();
    assert!(connect_checkout(&store, &manifest(), &code, "api", &api, &revision).is_err());
    assert!(connect_checkout(&store, &manifest(), &code, "api", &code, &revision).is_err());
    assert!(
        RepositoryCheckoutMapV1::load(&store)
            .unwrap()
            .checkouts
            .is_empty()
    );
}

#[test]
fn resolution_fails_closed_when_secondary_origin_changes() {
    let (_root, store, code, api) = fixture();
    connect_checkout(
        &store,
        &manifest(),
        &code,
        "api",
        &api,
        &store.revision().unwrap(),
    )
    .unwrap();
    assert!(manifest().target(&store, &code, "api").is_ok());
    assert!(
        std::process::Command::new("git")
            .args([
                "remote",
                "set-url",
                "origin",
                "https://example.test/team/other.git",
            ])
            .current_dir(&api)
            .status()
            .unwrap()
            .success()
    );
    assert!(manifest().target(&store, &code, "api").is_err());
}

#[test]
fn persisted_checkout_map_requires_current_schema() {
    let (_root, store, _code, _api) = fixture();
    store
        .atomic_write(CHECKOUTS_FILE, br#"{"schema_version":0,"checkouts":{}}"#)
        .unwrap();
    assert!(RepositoryCheckoutMapV1::load(&store).is_err());
}
