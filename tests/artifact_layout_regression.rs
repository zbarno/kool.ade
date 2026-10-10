use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};
use std::{fs, path::Path};

use koolade::{
    artifacts::{
        layout::canonical,
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

#[test]
fn koolade_repository_keeps_runtime_artifacts_in_the_canonical_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in ["planning", ".planner", "adr", "SPECIFICATION.md"] {
        assert!(
            !root.join(path).exists(),
            "legacy runtime artifact remains outside .koolade: {path}"
        );
    }
    assert!(
        !root
            .join(koolade::artifacts::layout::previous::WORK)
            .exists(),
        "planning work remains in its previous canonical location"
    );

    for path in [
        canonical::MANIFEST,
        canonical::PROJECT_CONFIG,
        canonical::PROJECT_MANIFEST,
        canonical::PRODUCT_MANIFEST,
        canonical::OPEN_ITEMS,
        canonical::RESOLVED_ITEMS,
        canonical::WORKFLOW,
        canonical::WORK,
    ] {
        assert!(
            root.join(path).is_file(),
            "missing canonical artifact: {path}"
        );
    }
    assert!(root.join(canonical::CHANGES).is_dir());
    let product: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(canonical::PRODUCT_MANIFEST)).unwrap()).unwrap();
    for module in product["modules"].as_array().unwrap() {
        let path = root
            .join(canonical::PRODUCT)
            .join(module["path"].as_str().unwrap());
        assert!(
            path.is_file(),
            "product module is missing: {}",
            path.display()
        );
    }
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
    planning_work::save(&root, std::slice::from_ref(&work)).unwrap();
    assert!(
        root.join(canonical::STATE)
            .join("work")
            .join(format!("{}.json", work.uid))
            .is_file()
    );
    assert!(!root.join(canonical::WORK).exists());
    assert!(root.join(canonical::STATE).is_dir());
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
        root.join("planning-repo"),
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
