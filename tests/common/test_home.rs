use std::{
    ffi::OsString,
    sync::{Mutex, MutexGuard},
};

static TEST_HOME_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct TestHome {
    previous: Option<OsString>,
    _root: tempfile::TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl TestHome {
    pub(crate) fn new() -> Self {
        let lock = TEST_HOME_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("KOOLADE_HOME");
        // SAFETY: test-home users in this executable share TEST_HOME_LOCK,
        // and the guard restores the value during unwinding.
        unsafe { std::env::set_var("KOOLADE_HOME", root.path()) };
        Self {
            previous,
            _root: root,
            _lock: lock,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // SAFETY: the guard keeps TEST_HOME_LOCK held until after this drop.
        unsafe {
            if let Some(previous) = self.previous.take() {
                std::env::set_var("KOOLADE_HOME", previous);
            } else {
                std::env::remove_var("KOOLADE_HOME");
            }
        }
    }
}
