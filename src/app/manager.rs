//! Independent project-manager updates; task workers never write into main chat.
mod catalog;
mod dependency;
mod prompt;
pub(crate) use dependency::{DependencyReview, DependencyTriage, enforce_policy};

use crate::harness::{AiHarness, LiveProgress, PlanningRequest};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Cap on the SYNTHETIC stalled-worker note. A stuck queue previously fed
/// itself: silence note -> manager turn -> re-arm ~2.5 min later -> endless
/// LLM billing. Genuine milestone events (assignments, completions) still
/// surface the manager on their own terms; only the manufactured
/// "still running" note is subject to this cap.
pub const PATROL_COOLDOWN: Duration = Duration::from_secs(30 * 60);
/// Worker silence tolerated before a stalled-worker note is surfaced.
const PATROL_NOTE_AFTER: Duration = Duration::from_secs(120);
/// Minimum spacing between automatic manager patrols, measured from the
/// previous patrol start (the watchdog's legacy pacing contract).
const PATROL_MANAGER_AFTER: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct WorkspaceActivity {
    pub overall: Option<LiveProgress>,
    pub conversations: BTreeMap<String, LiveProgress>,
    pub tasks: BTreeMap<String, LiveProgress>,
    pub manager: Option<Manager>,
    /// One queued Planner v4 pass, armed only by a saved clarification answer.
    pub(crate) pending_clarification_planner: Option<(String, u64)>,
    pub(crate) clarification_planner: Option<crate::app::root::planner_pass::ClarificationPlanner>,
    pub(crate) clarification_planner_identity: Option<(String, u64, String)>,
    pub(crate) plan_review: Option<crate::app::root::plan_review::PlanReview>,
    pub(crate) plan_review_feature: Option<String>,
    pub(crate) plan_review_revision: Option<String>,
    pub(crate) spec_reconciliation: Option<crate::app::root::SpecReconciliation>,
    pub(crate) feature_spec_reconciliation:
        Option<crate::app::root::spec_reconciliation::FeatureSpecReconciliation>,
    pub(crate) runtime_qa_worker: Option<crate::runtime_qa::QaWorkerHandle>,
    pub(crate) runtime_qa_scenario_synthesis:
        Option<crate::runtime_qa::scenario_synthesis::ScenarioSynthesisHandle>,
    pub(crate) runtime_qa_applicability_review:
        Option<crate::runtime_qa::applicability_review::ApplicabilityReviewHandle>,
    pub(crate) runtime_recipe_worker: Option<crate::runtime_qa::RuntimeRecipeWorkerHandle>,
    pub(crate) dependency_manager: Option<DependencyReview>,
    pub(crate) pending_dependency_reviews: Vec<(String, crate::harness::DependencyRequest)>,
    pub pending: Vec<String>,
    pub last_update: Option<Instant>,
    pub last_save: Option<Instant>,
    /// Limits reconciliation of self-improvement projections to once per second.
    pub last_self_improvement_sync: Option<Instant>,
    /// Tickets whose persisted activity may be stale (dirty-tracking gate
    /// for the periodic `activity.json` rewrite).
    pub dirty_tickets: HashSet<String>,
    /// Reconciled planning records awaiting a successful ledger write.
    pub pending_planning_work: bool,
    /// When the last synthetic stalled-worker patrol note was surfaced
    /// (subject to [`PATROL_COOLDOWN`]).
    pub last_patrol_note: Option<Instant>,
}

/// Pure gate: should a "worker still running" patrol note be surfaced?
/// Clock is injected so tests stay wall-clock-free (house convention).
pub fn patrol_note_due(
    live_worker_count: usize,
    pending_count: usize,
    last_update: Option<Instant>,
    last_note: Option<Instant>,
    now: Instant,
) -> bool {
    live_worker_count > 0
        && pending_count == 0
        && last_update.is_none_or(|then| stale_by(then, now, PATROL_NOTE_AFTER))
        && last_note.is_none_or(|then| stale_by(then, now, PATROL_COOLDOWN))
}

/// Pure gate: should an automatic manager patrol start? Driven exclusively
/// by unconsumed pending events (real milestones, or the cooldown-capped
/// synthetic stall note) plus patrol spacing — never by worker silence
/// alone. See [`patrol_note_due`] for where `last_update` originates.
pub fn patrol_manager_due(
    has_active_turn: bool,
    manager_running: bool,
    pending_count: usize,
    last_update: Option<Instant>,
    now: Instant,
) -> bool {
    !has_active_turn
        && !manager_running
        && pending_count > 0
        && last_update.is_none_or(|then| stale_by(then, now, PATROL_MANAGER_AFTER))
}

fn stale_by(then: Instant, now: Instant, by: Duration) -> bool {
    now.checked_duration_since(then)
        .is_some_and(|age| age >= by)
}

impl WorkspaceActivity {
    /// Requeue dependency requests whose in-memory broker/review disappeared
    /// while Kool.ad/e was closed. A saved authorization is not proof that
    /// offline preparation completed.
    pub fn recover_dependency_reviews(&mut self) -> Vec<String> {
        let mut recovered = Vec::new();
        for (ticket, progress) in &mut self.tasks {
            let mut changed = false;
            for request in &mut progress.dependency_requests {
                if matches!(
                    request.status,
                    crate::harness::DependencyRequestStatus::Pending
                        | crate::harness::DependencyRequestStatus::ManagerReviewing
                        | crate::harness::DependencyRequestStatus::Authorized
                ) {
                    request.decision =
                        crate::harness::DependencyDecision::RequiresUserAuthorization;
                    request.status = crate::harness::DependencyRequestStatus::AwaitingUser;
                    request.rationale = "Kool.ad/e restarted before dependency preparation completed. Review the request again; no new permission was granted during recovery.".into();
                    changed = true;
                }
            }
            if changed {
                recovered.push(ticket.clone());
            }
        }
        recovered
    }

    pub fn ensure_overall(&mut self) {
        if self.overall.is_some() {
            return;
        }
        let mut buckets = BTreeMap::<i64, u64>::new();
        for progress in self.tasks.values().chain(self.conversations.values()) {
            for (bucket, count) in &progress.telemetry.samples {
                *buckets.entry(*bucket).or_default() += count;
            }
        }
        let mut progress = LiveProgress::default();
        progress.telemetry.samples = buckets.into_iter().collect();
        self.overall = Some(progress);
    }

    /// Mark one ticket's persisted activity as potentially stale; the
    /// periodic flush then rewrites only these tickets (instead of
    /// every active ticket every 2 s whether changed or not).
    pub fn mark_ticket_dirty(&mut self, ticket: &str) {
        self.dirty_tickets.insert(ticket.to_owned());
    }

    /// Take out every dirty ticket for the periodic flush.
    pub fn take_dirty_tickets(&mut self) -> Vec<String> {
        let mut out: Vec<String> = std::mem::take(&mut self.dirty_tickets)
            .into_iter()
            .collect();
        out.sort();
        out
    }
}

pub struct Manager {
    progress: mpsc::Receiver<LiveProgress>,
    result: mpsc::Receiver<Result<ManagerOutput, String>>,
    cancel: Arc<AtomicBool>,
    prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
}

pub(super) struct ManagerOutput {
    pub summary: String,
    pub opportunities: Vec<crate::harness::prompt_catalog::OpportunityProposalV1>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
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
        let evidence_hashes = prepared
            .as_ref()
            .map(|prompt| prompt.evidence_hashes.clone())
            .unwrap_or_default();
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
        std::thread::spawn(move || {
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    let _ = done.send(Err(error));
                    return;
                }
            };
            let mut outcome = Err("Project manager did not complete".into());
            for _ in 0..2 {
                if request.cancel.load(Ordering::Relaxed) {
                    return;
                }
                outcome = harness
                    .execute(&request)
                    .map_err(|e| e.detail())
                    .and_then(|r| {
                        catalog::decode(
                            &r.final_text,
                            &prepared.validation,
                            evidence_hashes.clone(),
                        )
                    });
                if outcome.is_ok() {
                    break;
                }
            }
            let _ = done.send(outcome);
        });
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
