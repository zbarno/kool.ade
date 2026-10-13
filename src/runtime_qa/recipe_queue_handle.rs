use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::{execution::session::ExecutionSession, project_runtime::RuntimeReceiptV1};

/// One app-owned build or test worker dispatched by the activity queue.
pub(crate) struct RuntimeRecipeWorkerHandle {
    pub task_uid: String,
    cancel: Arc<AtomicBool>,
    session: Arc<Mutex<Option<Arc<ExecutionSession>>>>,
    result: mpsc::Receiver<Result<Vec<RuntimeReceiptV1>, String>>,
}

impl RuntimeRecipeWorkerHandle {
    pub(crate) fn new(
        task_uid: String,
        cancel: Arc<AtomicBool>,
        session: Arc<Mutex<Option<Arc<ExecutionSession>>>>,
        result: mpsc::Receiver<Result<Vec<RuntimeReceiptV1>, String>>,
    ) -> Self {
        Self {
            task_uid,
            cancel,
            session,
            result,
        }
    }

    pub(crate) fn take_result(&self) -> Result<Option<Result<Vec<RuntimeReceiptV1>, String>>, ()> {
        match self.result.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(()),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        if let Ok(session) = self.session.lock()
            && let Some(session) = session.as_ref()
        {
            let _ = session.cancel();
        }
    }
}

impl Drop for RuntimeRecipeWorkerHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_visible_to_the_detached_worker() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (_sender, receiver) = mpsc::channel();
        let handle = RuntimeRecipeWorkerHandle::new(
            "task-1".into(),
            Arc::clone(&cancelled),
            Arc::new(Mutex::new(None)),
            receiver,
        );
        handle.cancel();
        assert!(cancelled.load(Ordering::Acquire));
    }
}
