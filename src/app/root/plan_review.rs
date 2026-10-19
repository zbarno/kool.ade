//! Bounded independent review of a persisted, user-confirmed plan snapshot.
use crate::harness::AiHarness;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

mod prepare;
use prepare::recover_persisted_review;

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
}

impl PlanReview {
    #[cfg(test)]
    pub(crate) fn prepare_for_test(
        project: &super::session::Project,
        feature_id: String,
    ) -> Result<(), String> {
        prepare::prepare(project, feature_id).map(|_| ())
    }

    pub(crate) fn start(
        project: &super::session::Project,
        feature_id: String,
        harness: Box<dyn AiHarness>,
    ) -> Result<Self, String> {
        if let Some(outcome) = recover_persisted_review(project, &feature_id)? {
            let (done, result) = mpsc::channel();
            done.send(Ok(outcome))
                .map_err(|_| "could not queue the persisted plan review result".to_owned())?;
            return Ok(Self {
                result,
                cancel: Arc::new(AtomicBool::new(false)),
            });
        }
        let (mut request, context, outcome) = prepare::prepare(project, feature_id)?;
        let (done, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        request.cancel = worker_cancel.clone();
        std::thread::spawn(move || {
            let result = if worker_cancel.load(Ordering::Relaxed) {
                Err("plan review was cancelled".into())
            } else {
                harness
                    .execute_review(
                        &request,
                        request
                            .review_identity
                            .as_ref()
                            .expect("review identity set"),
                    )
                    .map_err(|error| error.detail())
                    .and_then(|response| {
                        let expected_hash = outcome.snapshot_hash.clone();
                        let mut context = context;
                        context.expected_subject_hash = Some(&expected_hash);
                        match crate::harness::prompt_catalog::validate_output_with_context(
                            crate::harness::prompt_catalog::PromptRole::PlanReviewer,
                            &response.final_text,
                            &context,
                        )? {
                            crate::harness::prompt_catalog::RoleOutputV1::Review(verdict) => {
                                Ok(PlanReviewOutcome { verdict, ..outcome })
                            }
                            _ => Err("plan reviewer returned an unexpected schema".into()),
                        }
                    })
            };
            let _ = done.send(result);
        });
        Ok(Self { result, cancel })
    }

    pub(crate) fn take_result(&self) -> Option<Result<PlanReviewOutcome, String>> {
        self.result.try_recv().ok()
    }
}

#[cfg(test)]
#[path = "plan_review_tests.rs"]
mod tests;

impl Drop for PlanReview {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
