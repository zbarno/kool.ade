use super::*;
use std::sync::{Condvar, Mutex, atomic::AtomicUsize};

pub(super) struct ImplementationRendezvous {
    required: usize,
    arrivals: AtomicUsize,
    mutex: Mutex<usize>,
    changed: Condvar,
}

impl ImplementationRendezvous {
    pub(super) fn new(required: usize) -> Self {
        Self {
            required,
            arrivals: AtomicUsize::new(0),
            mutex: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    pub(super) fn arrivals(&self) -> usize {
        self.arrivals.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn wait(&self) -> Result<(), AppError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut arrived = self.mutex.lock().unwrap();
        *arrived += 1;
        self.arrivals
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.changed.notify_all();
        while *arrived < self.required {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(AppError::HarnessFailed {
                    reason: "timed out waiting for concurrent implementation workers".into(),
                    stderr_tail: String::new(),
                });
            }
            let (guard, timeout) = self.changed.wait_timeout(arrived, remaining).unwrap();
            arrived = guard;
            if timeout.timed_out() && *arrived < self.required {
                return Err(AppError::HarnessFailed {
                    reason: "timed out waiting for concurrent implementation workers".into(),
                    stderr_tail: String::new(),
                });
            }
        }
        Ok(())
    }
}

pub(super) struct TaskChanges {
    files: Vec<(&'static str, &'static str)>,
    calls: Arc<AtomicUsize>,
    implementation_rendezvous: Option<Arc<ImplementationRendezvous>>,
}

impl TaskChanges {
    pub(super) fn new(files: &[(&'static str, &'static str)]) -> Self {
        Self {
            files: files.to_vec(),
            calls: Arc::new(AtomicUsize::new(0)),
            implementation_rendezvous: None,
        }
    }

    pub(super) fn with_rendezvous(
        files: &[(&'static str, &'static str)],
        rendezvous: Arc<ImplementationRendezvous>,
    ) -> Self {
        Self {
            files: files.to_vec(),
            calls: Arc::new(AtomicUsize::new(0)),
            implementation_rendezvous: Some(rendezvous),
        }
    }
}

impl AiHarness for TaskChanges {
    fn label(&self) -> String {
        "shared destination fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        let outcome = Fixture {
            mode: "complete",
            calls: self.calls.clone(),
        }
        .execute(request)?;
        if request.mode == crate::harness::ExecutionMode::Implementation {
            if let Some(rendezvous) = &self.implementation_rendezvous {
                rendezvous.wait()?;
            }
            for (path, content) in &self.files {
                fs::write(request.repo_root.join(path), content).unwrap();
            }
        }
        Ok(outcome)
    }

    fn current_execution_identity(&self) -> Option<crate::harness::ReviewExecutionIdentity> {
        Some(crate::harness::ReviewExecutionIdentity {
            execution_id: uuid::Uuid::new_v4().to_string(),
            session_id: uuid::Uuid::new_v4().to_string(),
            runtime_qa_screenshot_review: None,
        })
    }
}

pub(super) fn run_task(
    sandbox: &Sandbox,
    ticket: &str,
    harness: &TaskChanges,
) -> anyhow::Result<Implementation> {
    let _executor = super::super::super::sandbox::install_verification_executor();
    refresh_task_spec(&sandbox.planning_store, ticket);
    let (progress, _updates) = mpsc::channel();
    run_with_planning_store_options(
        &sandbox.planning_store,
        &sandbox.planning_store.root,
        &sandbox.repo,
        ticket,
        RunOptions {
            harness,
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::CreatePullRequest,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
            claim_lease: None,
        },
    )
}

pub(super) fn remote_ref(sandbox: &Sandbox, branch: &str) -> String {
    sandbox.git(
        &sandbox.root.join("remote.git"),
        &["rev-parse", &format!("refs/heads/{branch}")],
    )
}

pub(super) fn remote_ref_exists(sandbox: &Sandbox, branch: &str) -> bool {
    std::process::Command::new("git")
        .args([
            "-C",
            sandbox.root.join("remote.git").to_str().unwrap(),
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .unwrap()
        .success()
}
