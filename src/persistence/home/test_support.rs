use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static REGISTERED_TEST_HOMES: OnceLock<Mutex<HashMap<PathBuf, (usize, String)>>> = OnceLock::new();

pub(crate) struct TestHomeRegistration {
    path: PathBuf,
    token: String,
}

impl TestHomeRegistration {
    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for TestHomeRegistration {
    fn drop(&mut self) {
        unregister_test_home(&self.path);
    }
}

pub(crate) fn register_test_home(path: &Path) -> TestHomeRegistration {
    let mut homes = REGISTERED_TEST_HOMES
        .get_or_init(Default::default)
        .lock()
        .expect("test home registry lock");
    let entry = homes.entry(path.to_path_buf()).or_insert_with(|| {
        let token = uuid::Uuid::new_v4().to_string();
        if let (Ok(canonical_home), Ok(canonical_temp)) =
            (path.canonicalize(), std::env::temp_dir().canonicalize())
            && path.is_dir()
            && canonical_home.starts_with(canonical_temp)
        {
            let marker = marker_path(path, &token);
            std::fs::write(&marker, canonical_home.to_string_lossy().as_bytes())
                .expect("write test home capability marker");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600))
                    .expect("restrict test home capability marker permissions");
            }
        }
        (0, token)
    });
    entry.0 += 1;
    TestHomeRegistration {
        path: path.to_path_buf(),
        token: entry.1.clone(),
    }
}

pub(crate) fn unregister_test_home(path: &Path) {
    let mut homes = REGISTERED_TEST_HOMES
        .get_or_init(Default::default)
        .lock()
        .expect("test home registry lock");
    if let Some((count, token)) = homes.get_mut(path) {
        *count -= 1;
        if *count == 0 {
            let token = token.clone();
            homes.remove(path);
            let _ = std::fs::remove_file(marker_path(path, &token));
        }
    }
}

pub(super) fn has_test_home_capability(path: &Path, token: &str) -> bool {
    let Ok(canonical_home) = path.canonicalize() else {
        return false;
    };
    let Ok(canonical_temp) = std::env::temp_dir().canonicalize() else {
        return false;
    };
    if canonical_home != path || !canonical_home.starts_with(canonical_temp) {
        return false;
    }
    let marker = marker_path(path, token);
    let Ok(metadata) = std::fs::symlink_metadata(&marker) else {
        return false;
    };
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && std::fs::read_to_string(marker)
            .is_ok_and(|contents| contents == canonical_home.display().to_string())
}

fn marker_path(_home: &Path, token: &str) -> PathBuf {
    std::env::temp_dir().join(format!(".koolade-test-home-{token}"))
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
