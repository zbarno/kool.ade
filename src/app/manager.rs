//! Independent project-manager updates; task workers never write into main chat.
mod catalog;
mod dependency;
mod dispositions;
mod execute;
mod prompt;
pub(crate) const MAX_PLAN_ARBITRATION_ATTEMPTS: usize = 2;
pub(crate) use dependency::{DependencyReview, DependencyTriage, enforce_policy};
pub(super) use dispositions::{
    has_pending_plan_contest, has_pending_plan_repair, next_plan_arbitration, next_plan_repair,
    persist_plan_dispositions,
};

use crate::harness::{AiHarness, LiveProgress, PlanningRequest};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub use activity::{PATROL_COOLDOWN, WorkspaceActivity, patrol_manager_due, patrol_note_due};
mod activity;

pub struct Manager {
    progress: mpsc::Receiver<LiveProgress>,
    result: mpsc::Receiver<Result<ManagerOutput, String>>,
    cancel: Arc<AtomicBool>,
    prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
}

pub(super) struct ManagerOutput {
    pub summary: String,
    pub proposed_actions: Vec<crate::harness::prompt_catalog::ManagerActionV1>,
    pub opportunities: Vec<crate::harness::prompt_catalog::OpportunityProposalV1>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
    pub evidence_confidentialities:
        std::collections::HashMap<String, crate::harness::prompt_catalog::EvidenceConfidentiality>,
    pub planning_revision: String,
}

pub(super) fn persist_output(project: &super::session::Project, output: &ManagerOutput) -> String {
    let messages = catalog::persist_opportunities(project, output);
    if messages.is_empty() {
        return output.summary.clone();
    }
    format!("{}\n\n{}", output.summary, messages.join("\n"))
}
impl Manager {
    pub(super) fn prompt_body(project: &super::session::Project, events: &[String]) -> String {
        prompt::build(project, events)
    }

    pub fn start(
        project: &super::session::Project,
        events: &[String],
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (tx, progress) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let prepared = catalog::render(project, events);
        let product_prepared = catalog::render_product(project);
        let prompt_body = prepared
            .as_ref()
            .map(|prompt| prompt.rendered.user.clone())
            .unwrap_or_default();
        let system_instructions = prepared
            .as_ref()
            .map(|prompt| prompt.rendered.system.clone())
            .unwrap_or_default();
        let prompt_manifest = prepared
            .as_ref()
            .ok()
            .map(|prompt| prompt.rendered.manifest.clone());
        let _ = tx.send(LiveProgress {
            prompt_manifest: prompt_manifest.clone(),
            activity: Some("Project manager prompt prepared".into()),
            ..Default::default()
        });
        let request = PlanningRequest {
            mode: crate::harness::ExecutionMode::ReadOnlyAnalysis,
            task_id: None,
            execution_workspace: None,
            destination_sha: None,
            prompt_manifest: prompt_manifest.clone(),
            reasoning_level: "xhigh".into(),
            telemetry_phase: None,
            repo_root: project.state.repo_root.clone(),
            runtime_config_source: None,
            prompt_body,
            system_instructions,
            timeout: crate::core::turn::configured_turn_timeout(),
            progress_tx: tx,
            cancel: cancel.clone(),

            authorization_verifier: None,
            review_identity: None,
        };
        execute::launch(harness, request, prepared, product_prepared, done);
        Self {
            progress,
            result,
            cancel,
            prompt_manifest,
        }
    }
    pub fn progress(&self) -> Option<LiveProgress> {
        self.progress.try_recv().ok().map(|mut progress| {
            if progress.prompt_manifest.is_none() {
                progress.prompt_manifest = self.prompt_manifest.clone();
            }
            progress
        })
    }
    pub(super) fn result(&self) -> Option<Result<ManagerOutput, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Project-manager process ended unexpectedly".into()))
            }
        }
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
