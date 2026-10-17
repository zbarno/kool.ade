//! One bounded Planner v4 pass following a successfully reconciled clarification.
use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

mod prompt;
mod repository_evidence;
use prompt::render;

pub(crate) struct ClarificationPlanner {
    result: mpsc::Receiver<Result<crate::harness::prompt_catalog::PlannerOutputV2, String>>,
    cancel: Arc<AtomicBool>,
}

impl ClarificationPlanner {
    pub(crate) fn start(
        project: &super::session::Project,
        feature_uid: String,
        plan_revision: u64,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
        let (done_tx, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let prepared = render(project, &feature_uid, plan_revision);
        let repo_root = project.state.repo_root.clone();
        std::thread::spawn(move || {
            let outcome = (|| {
                let (rendered, validation) = prepared?;
                let request = PlanningRequest {
                    mode: ExecutionMode::ReadOnlyAnalysis,
                    task_id: None,
                    execution_workspace: None,
                    prompt_manifest: Some(rendered.manifest.clone()),
                    reasoning_level: "high".into(),
                    telemetry_phase: None,
                    repo_root,
                    runtime_config_source: None,
                    prompt_body: rendered.user,
                    system_instructions: rendered.system,
                    timeout: crate::core::turn::configured_turn_timeout(),
                    progress_tx,
                    cancel: worker_cancel,

                    authorization_verifier: None,
                    review_identity: None,
                };
                if request.cancel.load(Ordering::Relaxed) {
                    return Err("clarification planning was cancelled".into());
                }
                let response = harness.execute(&request).map_err(|error| error.detail())?;
                let crate::harness::prompt_catalog::RoleOutputV1::PlannerV3(output) =
                    crate::harness::prompt_catalog::validate_output_with_context(
                        crate::harness::prompt_catalog::PromptRole::Planner,
                        &response.final_text,
                        &validation,
                    )?
                else {
                    return Err("planner returned an unexpected schema".into());
                };
                output.validate()?;
                Ok(output)
            })();
            let _ = done_tx.send(outcome);
        });
        Self { result, cancel }
    }

    pub(crate) fn take_result(
        &self,
    ) -> Option<Result<crate::harness::prompt_catalog::PlannerOutputV2, String>> {
        self.result.try_recv().ok()
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for ClarificationPlanner {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(crate) fn spec_revision(
    project: &super::session::Project,
    feature_uid: &str,
) -> Result<String, String> {
    let (feature_id, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature_uid)
        })
        .ok_or_else(|| "the clarified feature is no longer active".to_owned())?;
    spec_revision_for_markdown(feature_uid, feature_id, markdown)
}

pub(crate) fn spec_revision_for_markdown(
    feature_uid: &str,
    feature_id: &str,
    markdown: &str,
) -> Result<String, String> {
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the clarified feature has no stable identity".to_owned())?;
    if identity.uid != feature_uid {
        return Err("the clarified feature identity changed".into());
    }
    let contract = crate::core::workflow::feature_contract(markdown);
    crate::core::lifecycle::PlanSnapshotV1::from_feature(feature_id, &identity, markdown, &contract)
        .map(|snapshot| snapshot.spec_revision)
}

mod persist;
pub(crate) use persist::persist;
