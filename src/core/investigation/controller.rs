use super::*;
use std::sync::mpsc::Receiver;

pub enum Event {
    Progress(Box<LiveProgress>),
    Done(Box<anyhow::Result<(PlannerState, String)>>),
}
pub struct Controller {
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    pub item_id: String,
    recovery_claim: Option<u8>,
    source_uid: Option<String>,
    #[cfg(test)]
    _keepalive: Option<mpsc::Sender<Event>>,
}
impl Controller {
    pub fn start(state: PlannerState, item_id: String, harness: Box<dyn AiHarness>) -> Self {
        let claim = try_claim(&state, &item_id).and_then(|claim| {
            claim.ok_or_else(|| {
                anyhow::anyhow!("{DEFER_PREFIX}: another worker owns this investigation")
            })
        });
        Self::launch(state, item_id, harness, claim)
    }

    pub(crate) fn start_claimed(
        state: PlannerState,
        item_id: String,
        harness: Box<dyn AiHarness>,
        claim: std::fs::File,
    ) -> Self {
        Self::launch(state, item_id, harness, Ok(claim))
    }

    fn launch(
        state: PlannerState,
        item_id: String,
        harness: Box<dyn AiHarness>,
        claim: anyhow::Result<std::fs::File>,
    ) -> Self {
        let source_uid = state
            .items
            .iter()
            .find(|item| item.id == item_id)
            .and_then(|item| item.uid.clone());
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_id = item_id.clone();
        std::thread::spawn(move || {
            let _claim = match claim {
                Ok(claim) => claim,
                Err(error) => {
                    let _ = tx.send(Event::Done(Box::new(Err(error))));
                    return;
                }
            };
            let (progress, updates) = mpsc::channel();
            let forward = tx.clone();
            let forwarder = std::thread::spawn(move || {
                for update in updates {
                    let _ = forward.send(Event::Progress(Box::new(update)));
                }
            });
            let result = run(
                &state,
                &worker_id,
                harness.as_ref(),
                progress,
                worker_cancel,
            );
            let _ = forwarder.join();
            let _ = tx.send(Event::Done(Box::new(result)));
        });
        Self {
            rx,
            cancel,
            item_id,
            recovery_claim: None,
            source_uid,
            #[cfg(test)]
            _keepalive: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn idle_fixture(item_id: impl Into<String>) -> Self {
        let (keepalive, rx) = mpsc::channel();
        Self {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            item_id: item_id.into(),
            recovery_claim: None,
            source_uid: None,
            _keepalive: Some(keepalive),
        }
    }
    pub(crate) fn with_recovery_claim(mut self, generation: Option<u8>) -> Self {
        self.recovery_claim = generation;
        self
    }
    pub(crate) fn source_uid(&self) -> Option<&str> {
        self.source_uid.as_deref()
    }
    pub(crate) fn recovery_claim(&self) -> Option<u8> {
        self.recovery_claim
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancellation_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel();
    }
}
