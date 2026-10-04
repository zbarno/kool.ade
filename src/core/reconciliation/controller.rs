use super::*;

pub struct Controller {
    rx: Receiver<anyhow::Result<(PlannerState, String)>>,
    cancel: Arc<AtomicBool>,
    pub feature_id: String,
}
impl Controller {
    pub fn start(state: PlannerState, candidate: Candidate, harness: Box<dyn AiHarness>) -> Self {
        let (tx, rx) = mpsc::channel();
        let feature_id = candidate.feature_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let (progress, updates) = mpsc::channel();
            let drain = std::thread::spawn(move || for _ in updates {});
            let result = run(
                &state,
                &candidate,
                harness.as_ref(),
                progress,
                worker_cancel,
            );
            let _ = drain.join();
            let _ = tx.send(result);
        });
        Self {
            rx,
            cancel,
            feature_id,
        }
    }
    pub fn poll(&self) -> Option<anyhow::Result<(PlannerState, String)>> {
        self.rx.try_recv().ok()
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
