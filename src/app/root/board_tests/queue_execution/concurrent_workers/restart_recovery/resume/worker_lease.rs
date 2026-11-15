use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const TASK_ENV: &str = "KOOLADE_TEST_RESTART_WORKER_TASK";
const READY_ENV: &str = "KOOLADE_TEST_RESTART_WORKER_READY";
const RELEASE_ENV: &str = "KOOLADE_TEST_RESTART_WORKER_RELEASE";

pub(in crate::app::root::board_tests::queue_execution::concurrent_workers::restart_recovery) fn register_isolated_worker_home(
    path: &Path,
) -> (
    crate::test_support::KooladeHomeGuard,
    crate::persistence::TestHomeRegistration,
    crate::test_support::ProcessEnvironmentGuard,
) {
    let home = crate::test_support::KooladeHomeGuard::set(path);
    let registration = crate::persistence::register_test_home(path);
    let mut token = crate::test_support::ProcessEnvironmentGuard::new();
    token.set("KOOLADE_TEST_HOME_TOKEN", registration.token());
    (home, registration, token)
}

pub(in crate::app::root::board_tests::queue_execution::concurrent_workers::restart_recovery) struct WorkerProcessLease
{
    child: Child,
    release: std::path::PathBuf,
    _directory: tempfile::TempDir,
    released: bool,
}

impl WorkerProcessLease {
    pub(in crate::app::root::board_tests::queue_execution::concurrent_workers::restart_recovery) fn spawn(
        task_id: &str,
    ) -> Self {
        let directory = tempfile::tempdir().expect("create worker handshake directory");
        let ready = directory.path().join("ready");
        let release = directory.path().join("release");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "app::root::board_tests::queue_execution::concurrent_workers::restart_recovery::restart_recovers_partial_task_work_without_duplicate_child_or_false_completion",
            ])
            .env(TASK_ENV, task_id)
            .env(READY_ENV, &ready)
            .env(RELEASE_ENV, &release)
            .env(
                "KOOLADE_HOME",
                std::env::var("KOOLADE_HOME").expect("parent configured its test home"),
            )
            .env(
                "KOOLADE_TEST_HOME_TOKEN",
                std::env::var("KOOLADE_TEST_HOME_TOKEN")
                    .expect("parent registered its isolated test home"),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start surviving worker process");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < deadline {
            if let Some(status) = child.try_wait().expect("poll worker process") {
                panic!("worker exited before acquiring its lease: {status}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ready.exists(), "worker did not acquire its execution lease");
        assert_eq!(
            std::fs::read_to_string(&ready).expect("read worker state root"),
            crate::persistence::state_root().display().to_string(),
            "worker and restarted app must share the same private state root"
        );
        Self {
            child,
            release,
            _directory: directory,
            released: false,
        }
    }

    pub(in crate::app::root::board_tests::queue_execution::concurrent_workers::restart_recovery) fn release(
        mut self,
    ) {
        self.finish();
    }

    fn finish(&mut self) {
        if self.released {
            return;
        }
        std::fs::write(&self.release, "release").expect("release worker lease");
        let status = self.child.wait().expect("wait for worker shutdown");
        self.released = true;
        assert!(status.success(), "worker lease helper failed: {status}");
    }
}

impl Drop for WorkerProcessLease {
    fn drop(&mut self) {
        if !self.released {
            let _ = std::fs::write(&self.release, "release");
            let _ = self.child.wait();
            self.released = true;
        }
    }
}

pub(in crate::app::root::board_tests::queue_execution::concurrent_workers::restart_recovery) fn run_worker_lease_helper_if_requested()
-> bool {
    let (Ok(task_id), Ok(ready), Ok(release)) = (
        std::env::var(TASK_ENV),
        std::env::var(READY_ENV),
        std::env::var(RELEASE_ENV),
    ) else {
        return false;
    };
    let lease = crate::persistence::execution_records::try_task_lock(&task_id)
        .expect("worker task lock is readable")
        .expect("surviving worker acquires task execution lease");
    std::fs::write(
        ready,
        crate::persistence::state_root().display().to_string(),
    )
    .expect("signal worker lease readiness");
    let deadline = Instant::now() + Duration::from_secs(120);
    while !Path::new(&release).exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(lease);
    assert!(
        Path::new(&release).exists(),
        "parent did not release worker"
    );
    true
}
