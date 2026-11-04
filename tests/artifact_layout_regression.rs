use sha2::Digest as _;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};
use std::{fs, path::Path};

use koolade::{
    artifacts::{
        layout::store as canonical,
        planning_store::{PlanningStore, StoreMode},
    },
    core::planning_work,
};

static TEST_HOME_LOCK: Mutex<()> = Mutex::new(());

struct TestHome {
    previous: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl TestHome {
    fn new(root: &Path) -> Self {
        let lock = TEST_HOME_LOCK.lock().unwrap();
        let previous = std::env::var_os("KOOLADE_HOME");
        // The lock serializes tests in this binary that redirect process-wide
        // Kool.ad/e storage to a disposable directory.
        unsafe { std::env::set_var("KOOLADE_HOME", root.join("koolade-home")) };
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(home) => unsafe { std::env::set_var("KOOLADE_HOME", home) },
            None => unsafe { std::env::remove_var("KOOLADE_HOME") },
        }
    }
}

/// Exercises the production local writer: TaskIntent records stay outside the
/// code Git repository, and an unrelated old `.packet` remote is never read.
/// A unit test cannot prove the application and code paths stay separated.
/// This uses temporary directories, no network, and low setup cost.
#[test]
fn planning_work_writes_only_to_private_local_storage() {
    let root = std::env::temp_dir().join(format!(
        "koolade-separated-planning-layout-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let code = root.join("code");
    let local = root.join("application-data");
    fs::create_dir_all(&code).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&code)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "packet",
                "https://example.invalid/old.packet"
            ])
            .current_dir(&code)
            .status()
            .unwrap()
            .success()
    );

    let _home = TestHome::new(&root);
    let store = PlanningStore::for_code_repository(uuid::Uuid::nil(), &local, &code);
    let work = planning_work::Work::new(
        "planning:separate".into(),
        "Separate planning record".into(),
        "Keep this record durable.".into(),
        "Waiting".into(),
    );
    planning_work::save(&store, std::slice::from_ref(&work)).unwrap();

    assert!(local.join(canonical::STATE).is_dir());
    let packet_remote = std::process::Command::new("git")
        .args(["remote", "get-url", "packet"])
        .current_dir(&code)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(packet_remote.stdout).unwrap().trim(),
        "https://example.invalid/old.packet"
    );
    assert!(!local.join(".git").exists());
    assert!(!code.join("planning").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn planning_work_save_creates_the_canonical_state_directory() {
    let root = std::env::temp_dir().join(format!(
        "koolade-work-state-layout-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    let _home = TestHome::new(&root);
    let work = planning_work::Work::new(
        "planning:fixture".into(),
        "Fixture work".into(),
        "Keep this record durable.".into(),
        "Waiting".into(),
    );
    let store = PlanningStore::new(
        uuid::Uuid::nil(),
        root.join("application-data"),
        StoreMode::ManagedLocal,
    );
    planning_work::save(&store, std::slice::from_ref(&work)).unwrap();
    assert!(
        store
            .root
            .join(canonical::STATE)
            .join("work")
            .join(format!("{}.json", work.uid))
            .is_file()
    );
    assert!(!store.root.join(canonical::WORK).exists());
    assert!(store.root.join(canonical::STATE).is_dir());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn managed_store_transactions_work_without_git_metadata() {
    let root = std::env::temp_dir().join(format!(
        "koolade-non-git-store-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    let _home = TestHome::new(&root);
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join("application-data"),
        StoreMode::ManagedLocal,
    );
    let initial_revision = store.revision().unwrap();
    let changes = vec![("state/work.json".into(), b"{}".to_vec())];

    let (_, committed_revision) = store
        .transaction_with_revision(&changes, Some(&initial_revision))
        .unwrap();

    assert_eq!(store.read("state/work.json").unwrap(), b"{}");
    assert_eq!(store.revision().unwrap(), committed_revision);
    assert!(!store.root.join(".koolade-transactions").exists());
    let _ = fs::remove_dir_all(root);
}

/// Exercises the production document reader with a private store containing
/// old local product files. A unit test cannot prove `cfg(test)` fixture
/// readers are absent from the shipped library. This uses only temporary
/// directories, no network, and low setup cost.
#[test]
fn private_local_product_files_are_never_a_published_spec_fallback() {
    let root = std::env::temp_dir().join(format!(
        "koolade-no-local-spec-fallback-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let local = root.join("application-data");
    let legacy = local.join("planning/product");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("index.md"), "# Private legacy product\n").unwrap();
    fs::write(
        legacy.join("overview.md"),
        "# Overview\n\nMust never be read as published truth.\n",
    )
    .unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &local, StoreMode::ManagedLocal);
    let draft_change = koolade::core::apply::LocalSpecDraftChange {
        document_id: "product:overview".into(),
        content: "# Overview\n\nPrivate draft without a code repository.\n".into(),
    };
    let snapshot = serde_json::json!({
        "schemaVersion": 1,
        "basePublishedSpecPin": null,
        "changes": [draft_change.clone()],
    });
    let snapshot_bytes = serde_json::to_vec_pretty(&snapshot).unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(&snapshot_bytes));
    let current = serde_json::json!({
        "schemaVersion": 1,
        "basePublishedSpecPin": null,
        "changes": [draft_change],
        "proposalSha256": digest.clone(),
    });
    store
        .transaction(
            &[
                (format!("state/spec_drafts/{digest}.json"), snapshot_bytes),
                (
                    "state/spec_drafts/current.json".into(),
                    serde_json::to_vec_pretty(&current).unwrap(),
                ),
            ],
            None,
        )
        .unwrap();

    let documents = koolade::artifacts::product_docs::load_documents(&store).unwrap();
    assert!(documents.is_none());
    assert!(
        koolade::artifacts::product_docs::current_document(&store, "product:overview")
            .unwrap()
            .is_none()
    );
    assert!(
        fs::read_to_string(legacy.join("overview.md"))
            .unwrap()
            .contains("Must never be read")
    );
    let _ = fs::remove_dir_all(root);
}
