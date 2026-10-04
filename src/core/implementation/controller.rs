use super::*;

pub struct Controller {
    #[cfg(test)]
    pub(super) _keep_alive: Option<Sender<Event>>,
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    auto_publish_gate: Arc<AtomicBool>,
}
impl Controller {
    #[cfg(test)]
    pub(crate) fn idle_fixture() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            _keep_alive: Some(tx),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            auto_publish_gate: Arc::new(AtomicBool::new(false)),
        }
    }
    #[cfg(test)]
    pub(crate) fn cancellation_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    pub fn start(
        repo: PathBuf,
        ticket: String,
        auto_merge: bool,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        Self::start_project(repo.clone(), repo, ticket, auto_merge, harness)
    }
    pub fn start_project(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        auto_merge: bool,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        Self::start_project_with_context(
            planning_root,
            target_repo,
            ticket,
            auto_merge,
            None,
            harness,
        )
    }
    pub fn start_project_with_context(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        auto_merge: bool,
        user_context: Option<String>,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let mode = if auto_merge {
            PublicationMode::AutoPublish
        } else {
            PublicationMode::HoldForReview
        };
        Self::start_project_with_policy(
            planning_root,
            target_repo,
            ticket,
            mode,
            false,
            user_context,
            harness,
        )
    }

    pub(crate) fn start_project_with_policy(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        publication_mode: PublicationMode,
        require_independent_checks: bool,
        user_context: Option<String>,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let auto_publish_gate = Arc::new(AtomicBool::new(
            publication_mode == PublicationMode::AutoPublish,
        ));
        let worker_publish_gate = auto_publish_gate.clone();
        std::thread::spawn(move || {
            let (progress, updates) = mpsc::channel();
            let fwd = tx.clone();
            let forward = std::thread::spawn(move || {
                for p in updates {
                    let _ = fwd.send(Event::Progress(p));
                }
            });
            let result = run_with_project_options(
                &planning_root,
                &target_repo,
                &ticket,
                RunOptions {
                    harness: harness.as_ref(),
                    cancel: worker_cancel,
                    progress,
                    gh: "gh",
                    publication_mode,
                    require_independent_checks,
                    user_context: user_context.as_deref(),
                    auto_publish_gate: Some(worker_publish_gate),
                },
            );
            let _ = forward.join();
            let _ = tx.send(Event::Done(Box::new(
                result.map_err(|error| Failure::from_error(&error)),
            )));
        });
        Self {
            rx,
            cancel,
            auto_publish_gate,
            #[cfg(test)]
            _keep_alive: None,
        }
    }
    pub fn poll(&self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Event::Done(Box::new(Err(Failure::new(
                    FailureKind::Other,
                    RecoveryDisposition::ExplicitResume,
                    "Implementation worker stopped without a result. Work is preserved; inspect the task failure and Resume implementation.",
                )))))
            }
        }
    }
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn disable_automatic_publication(&self) {
        self.auto_publish_gate.store(false, Ordering::SeqCst);
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.request_cancel();
    }
}
