use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// One app-owned QA worker. Dropping project UI state signals cancellation;
/// its receiver is polled from the bounded activity tick.
pub(crate) struct QaWorkerHandle {
    pub qa_run_id: String,
    pub task_uid: String,
    pub(crate) cancel: Arc<AtomicBool>,
    cancel_sender: tokio::sync::watch::Sender<bool>,
    progress: mpsc::Receiver<crate::harness::LiveProgress>,
    result: mpsc::Receiver<Result<(), String>>,
}

impl QaWorkerHandle {
    pub(crate) fn new(
        qa_run_id: String,
        task_uid: String,
        cancel: Arc<AtomicBool>,
        cancel_sender: tokio::sync::watch::Sender<bool>,
        progress: mpsc::Receiver<crate::harness::LiveProgress>,
        result: mpsc::Receiver<Result<(), String>>,
    ) -> Self {
        Self {
            qa_run_id,
            task_uid,
            cancel,
            cancel_sender,
            progress,
            result,
        }
    }

    pub(crate) fn take_result(&self) -> Result<Option<Result<(), String>>, ()> {
        match self.result.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(()),
        }
    }

    pub(crate) fn take_progress(&self) -> Option<crate::harness::LiveProgress> {
        self.progress.try_recv().ok()
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.cancel_sender.send(true);
    }
}

impl Drop for QaWorkerHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_authorization_progress_reaches_the_activity_tick() {
        let (progress_sender, progress) = mpsc::channel();
        let (_result_sender, result) = mpsc::channel();
        let handle = QaWorkerHandle::new(
            "qa-run".into(),
            "task".into(),
            Arc::new(AtomicBool::new(false)),
            tokio::sync::watch::channel(false).0,
            progress,
            result,
        );
        progress_sender
            .send(crate::harness::LiveProgress {
                activity: Some("Dependency authorization needs your decision".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            handle
                .take_progress()
                .and_then(|progress| progress.activity),
            Some("Dependency authorization needs your decision".into())
        );
    }
}
