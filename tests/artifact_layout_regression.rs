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

/// Exercises the filesystem boundary through the production planning writer:
/// records must land in the separate planning checkout and preserve an old
/// code-checkout directory. A unit test cannot prove the paths do not cross.
/// It uses only temporary directories and Git-independent filesystem IO, with
/// negligible setup cost and no external services.
#[test]
fn planning_work_writes_only_to_the_standalone_planning_checkout() {
    let root = std::env::temp_dir().join(format!(
        "koolade-separated-planning-layout-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let code = root.join("code");
    let planning = root.join("planning");
    let embedded_marker = code.join(".koolade-packet/planning/marker.md");
    fs::create_dir_all(embedded_marker.parent().unwrap()).unwrap();
    fs::write(&embedded_marker, "existing checkout data").unwrap();

    let _home = TestHome::new(&root);
    let store = PlanningStore::new(uuid::Uuid::nil(), &planning, StoreMode::ManagedLocal);
    let work = planning_work::Work::new(
        "planning:separate".into(),
        "Separate planning record".into(),
        "Keep this record durable.".into(),
        "Waiting".into(),
    );
    planning_work::save(&store, std::slice::from_ref(&work)).unwrap();

    assert!(planning.join(canonical::STATE).is_dir());
    assert_eq!(
        fs::read_to_string(embedded_marker).unwrap(),
        "existing checkout data"
    );
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
        root.join("planning-repo"),
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
