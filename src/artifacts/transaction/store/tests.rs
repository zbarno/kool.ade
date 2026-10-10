use super::*;
use crate::artifacts::planning_store::StoreMode;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

static TEST_HOME_LOCK: Mutex<()> = Mutex::new(());

struct TempDir {
    root: PathBuf,
    previous_home: Option<OsString>,
    _home_lock: MutexGuard<'static, ()>,
}

impl TempDir {
    fn new() -> Self {
        let lock = TEST_HOME_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("koolade-store-tx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let previous = std::env::var_os("KOOLADE_HOME");
        unsafe { std::env::set_var("KOOLADE_HOME", root.join("koolade-home")) };
        Self {
            root,
            previous_home: previous,
            _home_lock: lock,
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        match self.previous_home.take() {
            Some(home) => unsafe { std::env::set_var("KOOLADE_HOME", home) },
            None => unsafe { std::env::remove_var("KOOLADE_HOME") },
        }
    }
}

#[test]
fn interrupted_transaction_recovers_after_restart_then_retries() {
    let temp = TempDir::new();
    let root = temp.root.join("planning");
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    store.atomic_write("planning/a.md", b"before a").unwrap();
    store.atomic_write("planning/b.md", b"before b").unwrap();
    let changes = vec![
        ("planning/a.md".into(), b"after a".to_vec()),
        ("planning/b.md".into(), b"after b".to_vec()),
    ];

    assert!(apply_store_with_interruption(&store, &changes, None, Some(1)).is_err());
    assert_eq!(store.read("planning/a.md").unwrap(), b"after a");
    assert_eq!(store.read("planning/b.md").unwrap(), b"before b");

    let restarted = PlanningStore::new(store.project_id, &root, StoreMode::ManagedLocal);
    assert!(restarted.recover().unwrap());
    assert_eq!(restarted.read("planning/a.md").unwrap(), b"before a");
    assert_eq!(restarted.read("planning/b.md").unwrap(), b"before b");
    assert_eq!(restarted.transaction(&changes, None).unwrap().len(), 2);
    assert_eq!(restarted.read("planning/a.md").unwrap(), b"after a");
    assert_eq!(restarted.read("planning/b.md").unwrap(), b"after b");
}

#[test]
fn interrupted_transaction_recovers_across_git_availability_changes() {
    let temp = TempDir::new();
    let root = temp.root.join("planning");
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    store.atomic_write("planning/a.md", b"before a").unwrap();
    store.atomic_write("planning/b.md", b"before b").unwrap();
    let changes = vec![
        ("planning/a.md".into(), b"after a".to_vec()),
        ("planning/b.md".into(), b"after b".to_vec()),
    ];
    let journal_root = transaction_root(&store).unwrap();

    assert!(apply_store_with_interruption(&store, &changes, None, Some(1)).is_err());
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(transaction_root(&store).unwrap(), journal_root);
    let restarted = PlanningStore::new(store.project_id, &root, StoreMode::ManagedLocal);
    assert!(restarted.recover().unwrap());
    assert_eq!(restarted.read("planning/a.md").unwrap(), b"before a");
    assert_eq!(restarted.read("planning/b.md").unwrap(), b"before b");

    assert!(apply_store_with_interruption(&restarted, &changes, None, Some(1)).is_err());
    let hidden_git_metadata = root.with_extension("git-offline");
    std::fs::rename(root.join(".git"), &hidden_git_metadata).unwrap();
    assert!(restarted.recover().unwrap());
    assert_eq!(restarted.read("planning/a.md").unwrap(), b"before a");
    assert_eq!(restarted.read("planning/b.md").unwrap(), b"before b");
    assert_eq!(restarted.transaction(&changes, None).unwrap().len(), 2);
    assert_eq!(restarted.read("planning/a.md").unwrap(), b"after a");
    assert!(!std::fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("transaction")
    }));
    let _ = std::fs::remove_dir_all(hidden_git_metadata);
}

#[test]
fn transaction_removals_are_atomic_and_recoverable() {
    let temp = TempDir::new();
    let root = temp.root.join("planning");
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    store.atomic_write("planning/a.md", b"old").unwrap();
    store.atomic_write("planning/b.md", b"old b").unwrap();
    store
        .atomic_write("planning/remove.md", b"remove me")
        .unwrap();
    store
        .atomic_write("planning/remove-too.md", b"also remove me")
        .unwrap();
    let expected = store.revision().unwrap();
    let changes = vec![
        ("planning/a.md".into(), b"new".to_vec()),
        ("planning/b.md".into(), b"new b".to_vec()),
    ];
    let removals = vec![
        "planning/remove.md".to_owned(),
        "planning/remove-too.md".to_owned(),
    ];

    assert!(apply_store_inner(&store, &changes, &removals, Some(&expected), Some(3)).is_err());
    assert_eq!(store.read("planning/a.md").unwrap(), b"new");
    assert_eq!(store.read("planning/b.md").unwrap(), b"new b");
    assert!(matches!(
        store.read("planning/remove.md"),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound
    ));
    assert_eq!(
        store.read("planning/remove-too.md").unwrap(),
        b"also remove me"
    );
    assert!(store.recover().unwrap());
    assert_eq!(store.read("planning/a.md").unwrap(), b"old");
    assert_eq!(store.read("planning/b.md").unwrap(), b"old b");
    assert_eq!(store.read("planning/remove.md").unwrap(), b"remove me");
    assert_eq!(
        store.read("planning/remove-too.md").unwrap(),
        b"also remove me"
    );

    let (paths, revision) = store
        .transaction_with_removals_and_revision(&changes, &removals, Some(&expected))
        .unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(revision, store.revision().unwrap());
    assert_eq!(store.read("planning/a.md").unwrap(), b"new");
    assert!(matches!(
        store.read("planning/remove.md"),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound
    ));
}

#[test]
fn linked_worktrees_keep_recovery_journals_scoped_to_each_store_root() {
    let temp = TempDir::new();
    let first = temp.root.join("first");
    let second = temp.root.join("second");
    std::fs::create_dir_all(&first).unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&first, &["init", "-q", "-b", "main"]);
    git(&first, &["config", "user.name", "Koolade Test"]);
    git(&first, &["config", "user.email", "koolade@example.test"]);
    std::fs::write(first.join("tracked.txt"), "base").unwrap();
    git(&first, &["add", "tracked.txt"]);
    git(&first, &["commit", "-qm", "base"]);
    git(
        &first,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "second",
            second.to_str().unwrap(),
        ],
    );
    let root_a = first.join(crate::artifacts::layout::canonical::ROOT);
    let root_b = second.join(crate::artifacts::layout::canonical::ROOT);
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    let store_a = PlanningStore::new(uuid::Uuid::new_v4(), &root_a, StoreMode::LegacyEmbedded);
    let store_b = PlanningStore::new(uuid::Uuid::new_v4(), &root_b, StoreMode::LegacyEmbedded);
    assert_eq!(
        transaction_root(&store_a).unwrap(),
        transaction_root(&store_b).unwrap()
    );
    store_a
        .atomic_write("planning/a.md", b"worktree a old")
        .unwrap();
    store_a
        .atomic_write("planning/b.md", b"worktree a second")
        .unwrap();
    store_b
        .atomic_write("planning/a.md", b"worktree b own data")
        .unwrap();
    let changes = vec![
        ("planning/a.md".into(), b"worktree a new".to_vec()),
        ("planning/b.md".into(), b"worktree a updated".to_vec()),
    ];

    assert!(apply_store_with_interruption(&store_a, &changes, None, Some(1)).is_err());
    assert!(!store_b.recover().unwrap());
    assert_eq!(
        store_b.read("planning/a.md").unwrap(),
        b"worktree b own data"
    );
    assert!(store_a.recover().unwrap());
    assert_eq!(store_a.read("planning/a.md").unwrap(), b"worktree a old");
    assert_eq!(store_a.read("planning/b.md").unwrap(), b"worktree a second");
}
