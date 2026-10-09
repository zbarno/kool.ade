use super::*;
use crate::{error::AppError, harness::PlanningRequest};
use std::{
    sync::{Mutex, mpsc},
    time::Duration,
};

struct PausedMigrationHarness {
    calls: Arc<AtomicUsize>,
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl AiHarness for PausedMigrationHarness {
    fn label(&self) -> String {
        "paused migration fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        MigrationAwareFixture {
            calls: Arc::new(AtomicUsize::new(0)),
        }
        .execute(request)
    }
}

#[test]
fn concurrent_resume_cannot_enter_the_same_legacy_migration_twice() {
    let sandbox = Sandbox::new();
    setup_legacy_workspace(&sandbox);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let harness = PausedMigrationHarness {
        calls: calls.clone(),
        started: started_tx,
        release: Mutex::new(release_rx),
    };

    std::thread::scope(|scope| {
        let first = scope.spawn(|| resume_with_harness(&sandbox, &harness));
        started_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("first migration did not reach the harness");
        let second = scope.spawn(|| resume_with_harness(&sandbox, &harness));
        let second_started = started_rx.recv_timeout(Duration::from_millis(300)).is_ok();
        let _ = release_tx.send(());
        let _ = release_tx.send(());
        let first_result = first.join().unwrap();
        let second_result = second.join().unwrap();

        assert!(!second_started, "a concurrent run entered the harness");
        assert!(first_result.is_ok(), "{first_result:?}");
        assert!(second_result.is_err(), "second run unexpectedly succeeded");
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
