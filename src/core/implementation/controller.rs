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
    pub(crate) fn start_project_with_policy_and_claim_request(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        policy: super::StartPolicy,
        user_context: Option<String>,
        harness: Box<dyn AiHarness>,
        claim_request: crate::core::task_claim::ClaimRequest,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let auto_publish_gate = Arc::new(AtomicBool::new(
            policy.publication_mode == PublicationMode::AutoPublish,
        ));
        let worker_publish_gate = auto_publish_gate.clone();
        let previous_revision = super::load_activity(&planning_root, &ticket)
            .map(|progress| progress.checklist_revision)
            .unwrap_or_default();
        let checklist_epoch =
            checklist_revision_epoch(chrono::Utc::now().timestamp_millis(), previous_revision);
        std::thread::spawn(move || {
            if worker_cancel.load(Ordering::SeqCst) {
                let _ = tx.send(Event::Done(Box::new(Err(Failure::new(
                    FailureKind::Other,
                    RecoveryDisposition::ExplicitResume,
                    "Implementation was canceled before remote task coordination began.",
                )))));
                return;
            }
            let claim_lease = match claim_request.acquire() {
                Ok(attempt) => {
                    if let Some(warning) = attempt.warning {
                        let _ = tx.send(Event::ClaimWarning(warning));
                    }
                    Some(crate::core::task_claim::ClaimLeaseHandle::new(
                        attempt.lease,
                    ))
                }
                Err(error) => {
                    let _ = tx.send(Event::ClaimBlocked(Box::new(error)));
                    return;
                }
            };
            if worker_cancel.load(Ordering::SeqCst) {
                drop(claim_lease);
                let _ = tx.send(Event::Done(Box::new(Err(Failure::new(
                    FailureKind::Other,
                    RecoveryDisposition::ExplicitResume,
                    "Implementation was canceled while acquiring its remote task claim.",
                )))));
                return;
            }
            let heartbeat = claim_lease
                .as_ref()
                .filter(|claim| claim.has_lease())
                .cloned()
                .map(|claim| {
                    let heartbeat_cancel = worker_cancel.clone();
                    let (stop, stopped) = mpsc::channel();
                    let thread = std::thread::spawn(move || {
                        while let Err(mpsc::RecvTimeoutError::Timeout) =
                            stopped.recv_timeout(std::time::Duration::from_secs(
                                crate::core::task_claim::HEARTBEAT_INTERVAL_SECONDS,
                            ))
                        {
                            if claim.refresh().is_err() {
                                heartbeat_cancel.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    });
                    (stop, thread)
                });
            let (progress, updates) = mpsc::channel::<LiveProgress>();
            let fwd = tx.clone();
            let forward = std::thread::spawn(move || {
                for mut p in updates {
                    p.checklist_revision = if p.checklist_revision == 0 {
                        0
                    } else {
                        checklist_epoch.saturating_add(p.checklist_revision)
                    };
                    let _ = fwd.send(Event::Progress(Box::new(p)));
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
                    publication_mode: policy.publication_mode,
                    require_independent_checks: policy.require_independent_checks,
                    user_context: user_context.as_deref(),
                    auto_publish_gate: Some(worker_publish_gate),
                    claim_lease,
                },
            );
            if let Some((stop, thread)) = heartbeat {
                let _ = stop.send(());
                let _ = thread.join();
            }
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

fn checklist_revision_epoch(now_ms: i64, previous_revision: u64) -> u64 {
    let time_revision = (now_ms.max(0) as u64) << 20;
    time_revision.max(previous_revision)
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::checklist_revision_epoch;

    #[test]
    fn checklist_resume_revision_exceeds_durable_revision_even_if_clock_does_not_advance() {
        for now_ms in [1_000, -1] {
            let previous = 2_000_000;
            let epoch = checklist_revision_epoch(now_ms, previous);
            assert!(epoch.saturating_add(1) > previous);
        }
    }
}
