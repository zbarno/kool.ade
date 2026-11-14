//! Independent project-manager updates; task workers never write into main chat.
mod catalog;
mod dependency;
mod dispositions;
mod execute;
mod investigation;
mod launch;
pub(crate) mod operational_recovery;
mod output;
mod prompt;
mod telemetry;
pub(crate) const MAX_PLAN_ARBITRATION_ATTEMPTS: usize = 2;
pub(crate) use dependency::{DependencyReview, DependencyTriage, enforce_policy};
pub(crate) use dispositions::has_unpromoted_nonblocking_findings;
#[cfg(test)]
pub(crate) use dispositions::next_qa_finding_repair_with_store;
pub(super) use dispositions::persist_dispositions;
#[cfg(test)]
pub(super) use dispositions::persist_plan_dispositions;
pub(crate) use dispositions::{
    CodeReviewArbitrationRequest, approve_code_review_risk, finish_code_review_arbitration,
    finish_code_review_repair, finish_qa_finding_repair, mark_code_review_repair_failed,
    next_code_review_arbitration, next_code_review_repair, next_qa_finding_repair,
};
pub(super) use dispositions::{
    has_pending_plan_contest, has_pending_plan_repair, next_plan_arbitration, next_plan_repair,
};

use crate::harness::{AiHarness, LiveProgress, PlanningRequest};
#[cfg(test)]
pub(crate) use investigation::verify_manager_investigation_receipts_for_test;
use investigation::{
    cleanup_after_cancellation_ack, manager_execution_workspace,
    manager_investigation_authorization, manager_receipt_root, watch_manager_cancellation,
    write_cancellation_marker,
};
pub(super) use output::{ManagerOutput, persist_output};
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
    investigation_receipt_root: Option<std::path::PathBuf>,
    investigation_cancellation_id: Option<String>,
    cancellation_watch_stop: Arc<AtomicBool>,
    baseline_review_id: Option<String>,
    run_id: String,
    source_events: Vec<String>,
    event_checkpoint: Option<crate::domain::ManagerEventCheckpointV1>,
    run_event_saved: bool,
    run_timed_out: Arc<AtomicBool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BaselineReviewRequest {
    pub review_id: String,
    pub repository_id: String,
}

impl Manager {
    #[cfg(test)]
    pub(super) fn completed_for_test(result: Result<ManagerOutput, String>) -> Self {
        let (progress_tx, progress) = mpsc::channel();
        drop(progress_tx);
        let (done, result_receiver) = mpsc::channel();
        let _ = done.send(result);
        Self {
            progress,
            result: result_receiver,
            cancel: Arc::new(AtomicBool::new(false)),
            prompt_manifest: None,
            investigation_receipt_root: None,
            investigation_cancellation_id: None,
            cancellation_watch_stop: Arc::new(AtomicBool::new(true)),
            baseline_review_id: None,
            run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            source_events: Vec::new(),
            event_checkpoint: None,
            run_event_saved: false,
            run_timed_out: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn prompt_body(project: &super::session::Project, events: &[String]) -> String {
        prompt::build(project, events)
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
            Ok(mut result) => {
                self.cancellation_watch_stop.store(true, Ordering::Release);
                if let Some(root) = &self.investigation_receipt_root {
                    let _ = std::fs::remove_dir_all(root);
                }
                if let Ok(output) = &mut result {
                    output.baseline_review_id = self.baseline_review_id.clone();
                }
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.cancellation_watch_stop.store(true, Ordering::Release);
                if let Some(root) = &self.investigation_receipt_root {
                    let _ = std::fs::remove_dir_all(root);
                }
                Some(Err("Project-manager process ended unexpectedly".into()))
            }
        }
    }
    pub(super) fn baseline_review_id(&self) -> Option<&str> {
        self.baseline_review_id.as_deref()
    }

    pub(crate) fn source_events(&self) -> &[String] {
        &self.source_events
    }

    pub(crate) fn run_event_saved(&self) -> bool {
        self.run_event_saved
    }

    pub(crate) fn run_event(
        &self,
        outcome: crate::domain::ManagerRunOutcomeV1,
        sequence: u64,
    ) -> Option<crate::domain::ManagerRunEventV1> {
        let checkpoint = self.event_checkpoint.as_ref()?;
        Some(crate::domain::ManagerRunEventV1 {
            run_id: self.run_id.clone(),
            sequence,
            outcome: if outcome == crate::domain::ManagerRunOutcomeV1::Failed
                && self.run_timed_out.load(Ordering::Acquire)
            {
                crate::domain::ManagerRunOutcomeV1::TimedOut
            } else {
                outcome
            },
            checkpoint: None,
            checkpoint_cursor: checkpoint.cursor,
            checkpoint_sha256: checkpoint.sha256.clone(),
            retry_after_unix_ms: None,
        })
    }
}

#[cfg(test)]
pub(super) fn duplicate_disposition_output_for_test(
    project: &crate::app::session::Project,
) -> ManagerOutput {
    use crate::harness::prompt_catalog::{ManagerActionKind, ManagerActionV1};
    use sha2::Digest;

    let reviews = catalog::reviews::current_review_evidence(project);
    let review = reviews
        .iter()
        .find(|review| review.target.feature_id == "CHG-004")
        .expect("the fixture has persisted independent plan-review evidence");
    let finding_id = review
        .target
        .finding_ids
        .first()
        .expect("review evidence contains a finding")
        .clone();
    let evidence_hashes = reviews
        .iter()
        .flat_map(|review| std::iter::once(&review.item).chain(review.supporting_items.iter()))
        .map(|item| {
            (
                item.id.clone(),
                format!("{:x}", sha2::Sha256::digest(item.content.as_bytes())),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    ManagerOutput {
        summary: "The reviewed plan finding needs repair.".into(),
        uncertainty_reported: false,
        proposed_actions: vec![ManagerActionV1 {
            kind: ManagerActionKind::RepairPlan,
            target: format!("CHG-004#{finding_id}"),
            reason: "Repair the currently reviewed plan finding.".into(),
            evidence: vec![review.id.clone()],
            expected_revision: review.target.revision.clone(),
            contradiction: None,
        }],
        opportunities: Vec::new(),
        evidence_hashes,
        evidence_confidentialities: Default::default(),
        planning_revision: project.state.baseline_planning_revision.clone(),
        code_review_targets: Default::default(),
        verified_repository_evidence: Vec::new(),
        baseline_review_id: None,
    }
}

/// Query a deterministic, bounded page of the complete local task index.
/// Other Manager context builders can retrieve selected board rows without
/// copying the whole board into a provider prompt. focus_ids may contain task
/// identifiers or trigger-event text with exact identifier tokens.
pub(crate) fn task_index_page(
    project: &super::session::Project,
    expected_revision: Option<&str>,
    offset: usize,
    limit: usize,
    focus_ids: &[String],
) -> Result<prompt::index::TaskIndexPage, prompt::index::TaskIndexError> {
    prompt::index::query(project, expected_revision, offset, limit, focus_ids)
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let (Some(root), Some(cancellation_id)) = (
            &self.investigation_receipt_root,
            &self.investigation_cancellation_id,
        ) && root.is_dir()
        {
            write_cancellation_marker(root, cancellation_id);
            cleanup_after_cancellation_ack(root.clone(), cancellation_id.clone());
        }
        self.cancellation_watch_stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
