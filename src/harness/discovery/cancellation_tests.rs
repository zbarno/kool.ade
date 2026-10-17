use super::{run, with_cancellation};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct ProbeWorkerGuard {
    cancellation: Arc<AtomicBool>,
    worker: Option<JoinHandle<anyhow::Result<()>>>,
}

impl ProbeWorkerGuard {
    fn cancel_and_join(&mut self) -> thread::Result<anyhow::Result<()>> {
        self.cancellation.store(true, Ordering::Release);
        self.worker
            .take()
            .expect("probe worker must be joined only once")
            .join()
    }
}

impl Drop for ProbeWorkerGuard {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Production boundary: launches a fake CLI through the supervised process helper.
/// Invariant: cancellation kills descendants as well as the direct child promptly.
/// A unit-only mock cannot prove OS process-group termination; this needs a real shell.
/// Dependency: Unix shell/process groups only. Setup: one temporary script and markers.
#[test]
fn cancellation_terminates_forked_probe_process_tree() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("probe.sh");
    let started = root.path().join("descendant-started");
    let trigger = root.path().join("write-if-still-running");
    let survived = root.path().join("descendant-survived");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\n( : > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done; : > '{}' ) &\nwhile [ ! -f '{}' ]; do sleep 0.01; done\nsleep 30\n",
            started.display(),
            trigger.display(),
            survived.display(),
            started.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();

    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = cancellation.clone();
    let worker_path = path.clone();
    let began = Instant::now();
    let worker = thread::spawn(move || {
        with_cancellation(worker_cancellation, || {
            run(
                &[worker_path.to_string_lossy().into_owned()],
                Duration::from_secs(3),
                1024,
                1024,
                &[],
            )
        })
        .map(|_| ())
    });
    let mut worker = ProbeWorkerGuard {
        cancellation,
        worker: Some(worker),
    };
    while !started.exists() && began.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        started.exists(),
        "the background descendant must start before cancellation"
    );
    let cancellation_started = Instant::now();
    let result = worker
        .cancel_and_join()
        .expect("probe worker should not panic");
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(cancellation_started.elapsed() < Duration::from_secs(2));
    fs::write(trigger, "check descendant").unwrap();
    let observation_deadline = Instant::now() + Duration::from_secs(2);
    while !survived.exists() && Instant::now() < observation_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !survived.exists(),
        "cancelled probe descendants must not survive"
    );
}
