use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) struct InFlightRequest(Arc<AtomicUsize>);

impl InFlightRequest {
    pub(super) fn start(counter: Arc<AtomicUsize>) -> Self {
        Self(counter)
    }
}

impl Drop for InFlightRequest {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

pub(super) struct TemporaryDirectoryGuard(Option<PathBuf>);

impl TemporaryDirectoryGuard {
    pub(super) fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    pub(super) fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for TemporaryDirectoryGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}
