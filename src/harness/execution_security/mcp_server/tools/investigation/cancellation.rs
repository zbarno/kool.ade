use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) fn cancellation_watch(
    cancelled: Arc<AtomicBool>,
    receipt_root: PathBuf,
    cancellation_id: String,
) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let initial_parent = linux_parent_pid();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = std::thread::spawn(move || {
        while !worker_stop.load(Ordering::Acquire) {
            let marker = receipt_root.join(format!(".cancel-{cancellation_id}.flag"));
            let parent_changed =
                initial_parent.is_some_and(|parent| linux_parent_pid() != Some(parent));
            if marker.is_file() || parent_changed {
                cancelled.store(true, Ordering::Release);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    (stop, worker)
}

pub(super) fn write_cancellation_ack(
    receipt_root: &std::path::Path,
    cancellation_id: &str,
) -> Result<(), String> {
    let path = receipt_root.join(format!(".cancel-{cancellation_id}.ack"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(b"cancelled")?;
            file.sync_all()
        })
        .map_err(|_| "Manager cancellation acknowledgement could not be written".to_owned())
}

#[cfg(target_os = "linux")]
fn linux_parent_pid() -> Option<u32> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    stat.get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn linux_parent_pid() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Production boundary: app cancellation marker to broker cancellation flag.
    /// This verifies the cross-process marker handoff and acknowledgement; a
    /// broker flag unit test cannot prove that file transport. Local filesystem
    /// and thread only; no Docker, network, or SSH; low setup cost.
    #[test]
    fn cancellation_marker_reaches_broker_and_is_acknowledged() {
        let root = std::env::temp_dir().join(format!("k-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let id = uuid::Uuid::new_v4().hyphenated().to_string();
        std::fs::write(root.join(format!(".cancel-{id}.flag")), b"cancel").unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (stop, worker) = cancellation_watch(cancelled.clone(), root.clone(), id.clone());
        for _ in 0..100 {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(cancelled.load(Ordering::Acquire));
        write_cancellation_ack(&root, &id).unwrap();
        assert!(root.join(format!(".cancel-{id}.ack")).is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
