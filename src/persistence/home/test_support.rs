use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static REGISTERED_TEST_HOMES: OnceLock<Mutex<HashMap<PathBuf, usize>>> = OnceLock::new();

pub(crate) fn register_test_home(path: &Path) {
    REGISTERED_TEST_HOMES
        .get_or_init(Default::default)
        .lock()
        .expect("test home registry lock")
        .entry(path.to_path_buf())
        .and_modify(|count| *count += 1)
        .or_insert(1);
}

pub(crate) fn unregister_test_home(path: &Path) {
    let mut homes = REGISTERED_TEST_HOMES
        .get_or_init(Default::default)
        .lock()
        .expect("test home registry lock");
    if let Some(count) = homes.get_mut(path) {
        *count -= 1;
        if *count == 0 {
            homes.remove(path);
        }
    }
}

pub(super) fn is_registered_test_home(path: &Path) -> bool {
    REGISTERED_TEST_HOMES
        .get_or_init(Default::default)
        .lock()
        .expect("test home registry lock")
        .contains_key(path)
}

/// Keeps unit-test persistence away from an operator's real packet.
pub(super) fn test_state_root() -> PathBuf {
    static TEST_ROOT: OnceLock<PathBuf> = OnceLock::new();
    TEST_ROOT
        .get_or_init(|| {
            let root = std::env::temp_dir().join(format!(
                "koolade-test-state-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir(&root).expect("create isolated test application state root");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                    .expect("restrict isolated test application state root permissions");
            }
            root
        })
        .clone()
}
