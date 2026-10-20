//! Independent reviewer arbitration for a contested blocking code finding.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::harness::{AiHarness, LiveProgress};

pub(crate) struct CodeReviewArbitration {
    pub(crate) request: crate::app::manager::CodeReviewArbitrationRequest,
    result: mpsc::Receiver<Result<crate::core::implementation::ManagerCodeReviewFindings, String>>,
    progress: mpsc::Receiver<LiveProgress>,
    cancel: Arc<AtomicBool>,
}

impl CodeReviewArbitration {
    pub(crate) fn start(
        project: &crate::app::session::Project,
        request: crate::app::manager::CodeReviewArbitrationRequest,
        harness: Box<dyn AiHarness>,
    ) -> Result<Self, String> {
        let planning_store = project.state.planning_store.clone();
        let state_root = project.state.repo_root.clone();
        let ticket = request.ticket.clone();
        let context = request.context.clone();
        let (done, result) = mpsc::channel();
        let (updates, progress) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::Builder::new()
            .name("code-review-arbitration".into())
            .spawn(move || {
                let result = crate::core::implementation::arbitrate_code_review_finding(
                    &planning_store,
                    &state_root,
                    &ticket,
                    &context,
                    harness.as_ref(),
                    worker_cancel,
                    updates,
                )
                .map_err(|error| format!("{error:#}"));
                let _ = done.send(result);
            })
            .map_err(|error| format!("could not start code-review arbitration worker: {error}"))?;
        Ok(Self {
            request,
            result,
            progress,
            cancel,
        })
    }

    pub(crate) fn take_progress(&self) -> impl Iterator<Item = LiveProgress> + '_ {
        std::iter::from_fn(|| self.progress.try_recv().ok())
    }

    pub(crate) fn take_result(
        &self,
    ) -> Option<Result<crate::core::implementation::ManagerCodeReviewFindings, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "code-review arbitration worker ended unexpectedly".into(),
            )),
        }
    }
}

impl Drop for CodeReviewArbitration {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
