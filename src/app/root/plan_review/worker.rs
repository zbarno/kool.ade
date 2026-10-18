use super::*;

use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};

impl PlanReview {
    #[cfg(not(test))]
    pub(crate) fn start(
        project: &crate::app::session::Project,
        feature_id: String,
        harness: Box<dyn AiHarness>,
    ) -> Result<Self, String> {
        if let Some(outcome) = super::prepare::recover_persisted_review(project, &feature_id)? {
            let (done, result) = mpsc::channel();
            done.send(Ok(outcome))
                .map_err(|_| "could not queue the persisted plan review result".to_owned())?;
            return Ok(Self {
                result,
                cancel: Arc::new(AtomicBool::new(false)),
            });
        }
        Self::start_with_context(project, feature_id, harness, None)
    }

    pub(crate) fn start_arbitration(
        project: &crate::app::session::Project,
        feature_id: String,
        arbitration: ArbitrationContext,
        harness: Box<dyn AiHarness>,
    ) -> Result<Self, String> {
        Self::start_with_context(project, feature_id, harness, Some(arbitration))
    }

    fn start_with_context(
        project: &crate::app::session::Project,
        feature_id: String,
        harness: Box<dyn AiHarness>,
        arbitration: Option<ArbitrationContext>,
    ) -> Result<Self, String> {
        let (mut request, context, outcome) = prepare::prepare(project, feature_id, arbitration)?;
        let (done, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        request.cancel = worker_cancel.clone();
        std::thread::spawn(move || {
            let review_result = verdict::catch_execution_panic(|| {
                if worker_cancel.load(Ordering::Relaxed) {
                    Err("plan review was cancelled".to_owned())
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
                                    Ok(verdict)
                                }
                                _ => Err("plan reviewer returned an unexpected schema".into()),
                            }
                        })
                }
            });
            let mut outcome = outcome;
            match review_result {
                Ok(verdict) => outcome.verdict = sanitize_verdict(verdict),
                Err(error) => {
                    outcome.verdict = unable_verdict(
                        &outcome.snapshot_hash,
                        &outcome.requirements,
                        &format!("plan:{}", outcome.snapshot_uid),
                    );
                    outcome.review_error = Some(error);
                }
            }
            let _ = done.send(Ok(outcome));
        });
        Ok(Self { result, cancel })
    }
}
#[path = "worker/prepare.rs"]
mod prepare;
