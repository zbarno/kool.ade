use super::TaskChats;

impl TaskChats {
    pub(super) fn lock_store(slug: &str) -> Result<std::fs::File, String> {
        let dir = crate::persistence::project_dir(slug);
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("task-conversations.lock"))
            .map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if lock.try_lock().is_ok() {
                return Ok(lock);
            }
            if std::time::Instant::now() >= deadline {
                return Err(
                    "Another window is saving a task conversation; retry shortly.".to_string(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn store_lock_waits_for_a_short_write_in_another_window() {
        let root = tempfile::tempdir().unwrap();
        let slug = root.path().join("project").to_string_lossy().into_owned();
        let lock_path = crate::persistence::project_dir(&slug).join("task-conversations.lock");
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let held = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .unwrap();
        held.try_lock().unwrap();

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let release = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            drop(held);
        });
        started_rx.recv().unwrap();
        let started = Instant::now();
        let acquired = TaskChats::lock_store(&slug).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(75));
        drop(acquired);
        release.join().unwrap();
    }
}
