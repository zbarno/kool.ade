use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{
        project_repos::{ProjectManifest, Repository},
        repository_knowledge::{ProfileRequest, read_pinned_file, scan_profile},
    },
};

struct Temp(PathBuf);
impl Temp {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("koolade-profile-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn repository(root: &Path, files: &[(&str, &str)]) -> String {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Synthetic Test"]);
    git(root, &["config", "user.email", "synthetic@example.invalid"]);
    for (path, content) in files {
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "initial"]);
    git(root, &["branch", "-M", "main"]);
    git(root, &["rev-parse", "HEAD"])
}
fn manifest() -> ProjectManifest {
    ProjectManifest {
        repositories: vec![Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    }
}
fn profile(
    root: &Path,
    store: &PlanningStore,
    source_ref: &str,
    now: u64,
) -> crate::core::repository_knowledge::RepositoryKnowledgeProfileV1 {
    scan_profile(ProfileRequest {
        repository_id: "root",
        source_ref,
        code_root: root,
        planning_store: store,
        repositories: &manifest(),
        now_unix_ms: now,
    })
    .unwrap()
}

/// Production boundary: full scanner resolves an explicit ref and reads Git tree blobs.
/// A helper-only test cannot prove the profile's exact evidence SHA and cache key agree.
/// Uses local Git only; no network, SSH, or Docker; setup cost is low.
#[test]
fn scans_synthetic_rust_and_non_rust_monorepo_at_immutable_commit() {
    let code = Temp::new("monorepo");
    let store_root = Temp::new("planning");
    let sha = repository(
        code.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname='sample'\nversion='0.1.0'\n[dependencies]\nserde='1'\n",
            ),
            ("src/main.rs", "fn main() {}\n"),
            (
                "frontend/package.json",
                "{\"dependencies\":{\"react\":\"1\"}}",
            ),
            ("frontend/src/index.ts", "export default function App() {}"),
            ("frontend/src/app.test.ts", "test('works', () => {});"),
            (
                "docs/AGENTS.md",
                "# Navigation\n- [architecture.md](./architecture.md) - overview\n",
            ),
            ("docs/architecture.md", "# Architecture\n"),
            ("vendor/hidden.py", "print('ignored')"),
        ],
    );
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        store_root.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    let result = profile(code.path(), &store, "main", 100);
    assert_eq!(result.key.commit_sha, sha);
    assert!(!result.key.dirty_checkout);
    assert_eq!(
        result.status,
        crate::core::repository_knowledge::KnowledgeProfileStatus::Partial
    );
    assert!(result.languages.contains(&"Rust".to_owned()));
    assert!(result.languages.contains(&"TypeScript".to_owned()));
    assert!(
        result
            .dependencies
            .iter()
            .any(|claim| claim.statement.contains("serde"))
    );
    assert!(
        result
            .navigation_candidates
            .iter()
            .any(|candidate| candidate.path == "docs/architecture.md")
    );
    assert!(result.scan.navigation_indexes_read > 0);
    assert!(!result.languages.contains(&"Python".to_owned()));
    assert!(result.scan.tree_entries_seen <= 4_096);
    assert!(result.scan.bytes_read <= 1024 * 1024);
    let hot = profile(code.path(), &store, "main", 101);
    assert_eq!(hot.scanned_at_unix_ms, result.scanned_at_unix_ms);
    assert_eq!(hot.key, result.key);
    assert_eq!(hot.scan, result.scan);
    assert!(
        store
            .list_files("knowledge")
            .unwrap()
            .iter()
            .any(|file| file.name == "repository-profiles.json")
    );
}

/// Production boundary: selected retrieval must remain pinned after checkout edits.
/// A pure parser cannot prove the returned bytes come from the profile's immutable commit.
/// Uses local Git only; no network/SSH/Docker; setup cost is low.
#[test]
fn changed_commit_dirty_worktree_and_targeted_read_keep_revision_provenance() {
    let code = Temp::new("revision");
    let store_root = Temp::new("revision-store");
    let first_sha = repository(code.path(), &[("src/lib.rs", "pub fn old() {}\n")]);
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        store_root.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    let first = profile(code.path(), &store, "main", 200);
    assert_eq!(first.key.commit_sha, first_sha);
    std::fs::write(
        code.path().join("src/lib.rs"),
        "pub fn working_tree_only() {}\n",
    )
    .unwrap();
    let selected = read_pinned_file(
        &crate::core::repository_knowledge::ProfileSource {
            repository_id: "root".into(),
            source_ref: "main".into(),
            code_root: code.path().to_path_buf(),
            planning_store: store.clone(),
            repositories: manifest(),
        },
        &first.key,
        "src/lib.rs",
    )
    .unwrap();
    assert_eq!(selected.0, "pub fn old() {}\n");
    let dirty = profile(code.path(), &store, "main", 201);
    assert!(dirty.key.dirty_checkout);
    assert_eq!(dirty.key.commit_sha, first.key.commit_sha);
    git(code.path(), &["add", "src/lib.rs"]);
    git(code.path(), &["commit", "-qm", "updated"]);
    let second_sha = git(code.path(), &["rev-parse", "HEAD"]);
    let second = profile(code.path(), &store, "main", 202);
    assert_eq!(second.key.commit_sha, second_sha);
    assert_ne!(second.key.commit_sha, first.key.commit_sha);
    assert!(
        read_pinned_file(
            &crate::core::repository_knowledge::ProfileSource {
                repository_id: "root".into(),
                source_ref: "main".into(),
                code_root: code.path().to_path_buf(),
                planning_store: store,
                repositories: manifest(),
            },
            &first.key,
            "../Cargo.toml"
        )
        .is_err()
    );
}

#[path = "tests/repository_isolation.rs"]
mod repository_isolation;
