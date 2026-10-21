use super::{DependencyReview, Manager};
use crate::harness::LiveProgress;
use std::{
    collections::{BTreeMap, HashSet},
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
    pub(crate) code_review_arbitration:
        Option<crate::app::root::code_review_arbitration::CodeReviewArbitration>,
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
