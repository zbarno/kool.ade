//! Bounded independent review of a persisted, user-confirmed plan snapshot.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
#[path = "plan_review/evidence.rs"]
pub(crate) mod evidence;
#[cfg(not(test))]
mod prepare;
#[path = "plan_review/verdict.rs"]
pub(crate) mod verdict;
#[path = "plan_review/worker.rs"]
mod worker;
use verdict::{sanitize_verdict, unable_verdict};

pub(crate) struct PlanReview {
    result: mpsc::Receiver<Result<PlanReviewOutcome, String>>,
    cancel: Arc<AtomicBool>,
}

pub(crate) struct PlanReviewOutcome {
    pub feature_id: String,
    pub snapshot_uid: String,
    pub snapshot_hash: String,
    pub author_run_id: String,
    pub author_session_id: String,
    pub reviewer_run_id: String,
    pub reviewer_session_id: String,
    pub evidence_refs: Vec<String>,
    pub prompt: crate::harness::prompt_catalog::PromptManifest,
    pub verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
    /// Kept in the local activity message; persisted verdict text stays sanitized.
    pub review_error: Option<String>,
    pub arbitration: Option<ArbitrationContext>,
    requirements: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct ArbitrationContext {
    pub finding_id: String,
    pub evidence_id: String,
    pub evidence_sha256: String,
    pub source_quote: String,
    pub source_content: String,
    pub rationale: String,
}

#[cfg(test)]
#[path = "plan_review/tests.rs"]
mod tests;

impl PlanReview {
    pub(crate) fn take_result(&self) -> Option<Result<PlanReviewOutcome, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "independent plan review worker ended unexpectedly".into(),
            )),
        }
    }
}

impl Drop for PlanReview {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
