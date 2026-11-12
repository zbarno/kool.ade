use super::super::super::super::*;
use std::{
    path::Path,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

struct WorkerAttempt {
    task_id: String,
    acquired_lease: bool,
}

struct LeaseProbeHarness {
    attempts: Sender<WorkerAttempt>,
    release_after_acquire: Option<Arc<Barrier>>,
    waited_for_release: AtomicBool,
}

impl crate::harness::AiHarness for LeaseProbeHarness {
    fn label(&self) -> String {
        "restart lease probe fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        let task_id = request
            .task_id
            .as_deref()
            .expect("production request retains the persisted task UID");
        let lock = crate::execution::docker::backend::acquire_task_execution_lock(task_id);
        let acquired_lease = lock.is_ok();
        self.attempts
            .send(WorkerAttempt {
                task_id: task_id.to_owned(),
                acquired_lease,
            })
            .expect("test is waiting for the worker attempt");
        if acquired_lease
            && self
                .waited_for_release
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            && let Some(release) = &self.release_after_acquire
        {
            release.wait();
        }
        drop(lock);
        Err(crate::AppError::HarnessExecutionFailed {
            failure: Box::new(crate::harness::HarnessExecutionFailure {
                provider_id: "restart-lease-probe".into(),
                role: crate::execution::contract::HarnessRole::Implementer,
                mode: crate::harness::ExecutionMode::Implementation,
                target: Some(task_id.to_owned()),
                category: crate::harness::HarnessFailureCategory::Cancelled,
                provider_code: None,
                sanitized_message: if acquired_lease {
                    "synthetic recovery probe stopped after acquiring the worker lease".into()
                } else {
                    "synthetic recovery probe was fenced by the active worker lease".into()
                },
                actionable_fix: None,
                retryable: false,
            }),
        })
    }
}

/// Exercise the actual Task Details -> queue -> ImplementationController path
/// after `restart_fixture` has reloaded persisted state. The held OS task lease
/// represents a worker that survived the app-session restart: a second run must
/// reach the harness with the same TaskIntent UID and be refused before work.
/// Releasing the old lease must let that same interrupted child resume while its
/// clone and source SHA remain pinned. A serializer test cannot prove this app
/// command and controller path. The harness is synthetic; local files/Git only,
/// no Docker or network. Setup is low cost beyond the parent recovery fixture.
pub(super) fn exercise_task_details_resume(
    app: &mut KooladeApp,
    ticket: &str,
    task_id: &str,
    clone_path: &Path,
    source_sha: &str,
    old_worker_lease: std::fs::File,
) {
    // An unrelated optional VM setup item remains open while this ordinary
    // approved task resumes. This is the dispatch-boundary regression for
    // issue #581 acceptance scenario 10.
    use crate::persistence::setup_tasks::{SetupTaskEventV1, SetupTaskStateV1, SetupTaskV1};
    let setup_state = SetupTaskStateV1::ReadyToFix;
    app.machine_setup_tasks = vec![SetupTaskV1 {
        schema_version: 1,
        id: "setup:optional-vm-smoke".into(),
        title: "Optional VM smoke-test setup".into(),
        explanation: "An optional VM smoke-test profile is not available.".into(),
        next_step: "Prepare that profile only if a future task requires it.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state: setup_state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: vec![SetupTaskEventV1 {
            observed_at_unix_ms: 1,
            scan_generation: 1,
            state: setup_state,
        }],
        superseded_by: Vec::new(),
    }];
    let (attempt_tx, attempt_rx) = mpsc::channel();
    app.task_harness = Some(Box::new(LeaseProbeHarness {
        attempts: attempt_tx,
        release_after_acquire: None,
        waited_for_release: AtomicBool::new(false),
    }));
    dispatch_resume(app, ticket);
    assert!(
        matches!(&app.screen, Screen::Connected(project)
        if project.active_implementations.contains_key(ticket)),
        "resume was rejected before controller startup: {}",
        match &app.screen {
            Screen::Connected(project) => &project.queue.last_error,
            _ => "disconnected",
        }
    );

    let fenced = receive_attempt(&attempt_rx, app, ticket);
    assert_eq!(fenced.task_id, task_id);
    assert!(!fenced.acquired_lease, "duplicate worker must be fenced");
    wait_for_controller(app, ticket);
    let duplicate_attempts = drain_attempts(&attempt_rx);
    assert!(!duplicate_attempts.is_empty());
    assert!(
        duplicate_attempts
            .iter()
            .all(|attempt| attempt.task_id == task_id && !attempt.acquired_lease)
    );
    assert_pinned_interrupted_state(app, ticket, task_id, clone_path, source_sha);

    drop(old_worker_lease);
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let release = Arc::new(Barrier::new(2));
    app.task_harness = Some(Box::new(LeaseProbeHarness {
        attempts: attempt_tx,
        release_after_acquire: Some(release.clone()),
        waited_for_release: AtomicBool::new(false),
    }));
    dispatch_resume(app, ticket);
    let resumed = receive_attempt(&attempt_rx, app, ticket);
    assert_eq!(resumed.task_id, task_id);
    assert!(
        resumed.acquired_lease,
        "resume must acquire the released lease"
    );
    assert_pinned_interrupted_state(app, ticket, task_id, clone_path, source_sha);
    release.wait();
    wait_for_controller(app, ticket);
    let resumed_attempts = drain_attempts(&attempt_rx);
    assert!(!resumed_attempts.is_empty());
    assert!(
        resumed_attempts
            .iter()
            .all(|attempt| attempt.task_id == task_id && attempt.acquired_lease)
    );
    assert_pinned_interrupted_state(app, ticket, task_id, clone_path, source_sha);
}

fn drain_attempts(receiver: &Receiver<WorkerAttempt>) -> Vec<WorkerAttempt> {
    receiver.try_iter().collect()
}

fn dispatch_resume(app: &mut KooladeApp, ticket: &str) {
    let _capabilities =
        crate::harness::runtime_capabilities::RuntimeCapabilities::override_for_test(
            crate::harness::runtime_capabilities::RuntimeCapabilities {
                planning_access:
                    crate::harness::runtime_capabilities::PlanningAccess::SandboxedRepositoryReads,
                implementation: true,
            },
        );
    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::StartOrResume {
            ticket: ticket.to_owned(),
        },
    ));
}

fn receive_attempt(
    receiver: &Receiver<WorkerAttempt>,
    app: &mut KooladeApp,
    ticket: &str,
) -> WorkerAttempt {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(attempt) = receiver.try_recv() {
            return attempt;
        }
        crate::app::root::tick::poll_implementations_for_test(app);
        if !matches!(&app.screen, Screen::Connected(project)
            if project.active_implementations.contains_key(ticket))
        {
            panic!(
                "controller ended before harness; queue state: {}",
                match &app.screen {
                    Screen::Connected(project) => project.queue.last_error.clone(),
                    _ => "disconnected".into(),
                }
            );
        }
        assert!(
            Instant::now() < deadline,
            "resumed controller did not reach the harness"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_controller(app: &mut KooladeApp, ticket: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        crate::app::root::tick::poll_implementations_for_test(app);
        let active = matches!(&app.screen, Screen::Connected(project)
            if project.active_implementations.contains_key(ticket));
        if !active {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "resumed controller did not stop; queue error: {}; implementation status: {:?}",
            match &app.screen {
                Screen::Connected(project) => project.queue.last_error.as_str(),
                _ => "disconnected",
            },
            match &app.screen {
                Screen::Connected(project) => project
                    .implementation_states
                    .get(ticket)
                    .map(|state| state.status),
                _ => None,
            }
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_pinned_interrupted_state(
    app: &KooladeApp,
    ticket: &str,
    task_id: &str,
    clone_path: &Path,
    source_sha: &str,
) {
    let Screen::Connected(project) = &app.screen else {
        panic!("restarted app disconnected");
    };
    let state = project
        .implementation_states
        .get(ticket)
        .expect("persisted task state remains loaded");
    assert_eq!(state.task_uid.as_deref(), Some(task_id));
    assert_eq!(state.task_repository, clone_path);
    assert_eq!(state.source_commit.as_deref(), Some(source_sha));
    assert_ne!(
        state.status,
        crate::core::implementation::ImplementationStatus::Completed,
        "interrupted work cannot be falsely completed"
    );
    assert!(clone_path.join("partial-progress.txt").is_file());
    assert!(project.queue.in_flight.contains(ticket) || project.queue.blocked.contains_key(ticket));
}
