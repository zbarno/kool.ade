//! The pure, deterministic heart of the "Prepare this computer" flow.
//!
//! `beat` inspects an injected snapshot (flow position, persisted tasks,
//! scan generation, controller occupancy, clocks) and returns exactly one
//! action. The application layer translates actions into the existing
//! repair-dispatch, consent, and rescan facilities; it never lets the flow
//! bypass them. No provider, network, or wall-clock randomness participates.

use crate::persistence::setup_tasks::{
    DoneOutcomeV1, FlowPhaseV1, HaltReasonV1, SetupFlowStateV1, SetupTaskV1, State,
    UserActionReasonV1,
};

use super::{
    order, policy,
    routes::{self, FixRoute},
};

/// Throttle between authoritative rechecks while the flow makes progress.
pub const DEFAULT_RESCAN_COOLDOWN_MS: u64 = 15_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SlotPhase {
    /// Nothing in this controller family.
    #[default]
    Idle,
    /// A consent review is presented for `task_id`.
    Preview,
    /// An approved operation is executing.
    Running,
    /// The operation settled; verification evidence is outstanding.
    Settling,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SlotStatus {
    pub family: Option<SlotFamily>,
    pub task_id: Option<String>,
    pub phase: SlotPhase,
}

/// What an operation should feel like once approved (informational; the
/// real execution policy lives in each controller).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotFamily {
    Remediation,
    Image,
    ClaudeInstall,
}

impl From<FixRoute> for SlotFamily {
    fn from(route: FixRoute) -> Self {
        match route {
            FixRoute::Remediation => Self::Remediation,
            FixRoute::Image => Self::Image,
            FixRoute::ClaudeInstall => Self::ClaudeInstall,
        }
    }
}

#[derive(Debug)]
pub struct BeatInput<'a> {
    pub flow: Option<&'a SetupFlowStateV1>,
    pub tasks: &'a [SetupTaskV1],
    pub scan_generation: u64,
    pub probe_busy: bool,
    pub slots: &'a [SlotStatus],
    /// Set when a repair just changed a task and a fresh authoritative scan
    /// is needed to trust downstream prerequisites.
    pub needs_fresh_scan: bool,
    pub now_unix_ms: u64,
    pub rescan_cooldown_ms: u64,
    pub last_rescan_at: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlowAction {
    /// Stay put; nothing new to do this tick.
    Hold,
    /// Ask the existing repair dispatch to open a scoped consent review.
    BeginFix { task_id: String, route: FixRoute },
    /// Remain in the consent wait and keep the decision in the user's face.
    ConsentOutstanding { task_id: String },
    /// Persist a phase bookkeeping change (no host effect).
    SyncPhase {
        phase: FlowPhaseV1,
        detail_task: Option<String>,
    },
    /// Run an authoritative readiness recheck through the scanner.
    TriggerRescan,
    /// Declare the flow finished with its measured outcome.
    Finish(DoneOutcomeV1),
    /// Terminate the flow unsuccessfully but safely.
    Halt {
        reason: HaltReasonV1,
        detail_task: Option<String>,
    },
}

pub fn beat(input: &BeatInput) -> FlowAction {
    let Some(flow) = input.flow else {
        return FlowAction::Hold;
    };
    if flow.phase.is_terminal() {
        return FlowAction::Hold;
    }
    if let Some(regressed) = readiness_regression(flow, input.tasks) {
        return FlowAction::Halt {
            reason: HaltReasonV1::ReadinessChanged,
            detail_task: Some(regressed),
        };
    }
    match &flow.phase {
        FlowPhaseV1::Paused => FlowAction::Hold,
        FlowPhaseV1::WaitingForConsent { task_id } => consent_watch(input, task_id),
        FlowPhaseV1::WaitingForUser { task_id, .. } => user_watch(input, task_id),
        FlowPhaseV1::Finishing => finishing(input, flow),
        FlowPhaseV1::Running => running(input, flow),
        FlowPhaseV1::Complete(_) | FlowPhaseV1::Halted { .. } => FlowAction::Hold,
    }
}

fn readiness_regression(flow: &SetupFlowStateV1, tasks: &[SetupTaskV1]) -> Option<String> {
    tasks
        .iter()
        .find(|task| {
            flow.seen_verified.contains(&task.id)
                && !matches!(task.state, State::Verified | State::Superseded)
        })
        .map(|task| task.id.clone())
}

fn slot_busy(slots: &[SlotStatus], route: FixRoute, task_id: &str) -> bool {
    let family: SlotFamily = route.into();
    slots.iter().any(|slot| {
        slot.family == Some(family)
            && slot.task_id.as_deref() == Some(task_id)
            && matches!(
                slot.phase,
                SlotPhase::Preview | SlotPhase::Running | SlotPhase::Settling
            )
    })
}

fn consent_watch(input: &BeatInput, task_id: &str) -> FlowAction {
    let Some(task) = input.tasks.iter().find(|t| t.id == task_id) else {
        return back_to_running();
    };
    if task.state == State::NeedsApproval && routes::can_decide_fix(task) {
        return FlowAction::ConsentOutstanding {
            task_id: task.id.clone(),
        };
    }
    back_to_running()
}

fn user_watch(input: &BeatInput, task_id: &str) -> FlowAction {
    let Some(task) = input.tasks.iter().find(|t| t.id == task_id) else {
        return back_to_running();
    };
    if matches!(
        task.state,
        State::Verified | State::Unsupported | State::Superseded
    ) {
        return back_to_running();
    }
    FlowAction::Hold
}

fn back_to_running() -> FlowAction {
    FlowAction::SyncPhase {
        phase: FlowPhaseV1::Running,
        detail_task: None,
    }
}

fn running(input: &BeatInput, flow: &SetupFlowStateV1) -> FlowAction {
    // 1. An operation already in motion owns the beat.
    if input
        .tasks
        .iter()
        .any(|task| matches!(task.state, State::InProgress | State::AwaitingVerification))
    {
        return FlowAction::Hold;
    }
    // 2. A consent review is presented; the decision belongs to the person.
    if let Some(queued) = input
        .tasks
        .iter()
        .find(|task| task.state == State::NeedsApproval && routes::can_decide_fix(task))
    {
        return FlowAction::SyncPhase {
            phase: FlowPhaseV1::WaitingForConsent {
                task_id: queued.id.clone(),
            },
            detail_task: None,
        };
    }
    // 3. Fresh truth before trusting prerequisite edges.
    if input.needs_fresh_scan {
        return rescan_or_hold(input);
    }

    let eligible = eligible_view(input.tasks);
    let Ok(planned) = order::sequence(&eligible) else {
        return FlowAction::Halt {
            reason: HaltReasonV1::InternalError,
            detail_task: None,
        };
    };
    let mut barrier: Option<(String, UserActionReasonV1)> = None;
    let mut promotion_due = false;
    for id in planned {
        let Some(task) = input.tasks.iter().find(|t| t.id == id) else {
            continue;
        };
        if is_skipped(flow, &id) {
            continue;
        }
        let management = policy::classify(task);
        match management {
            policy::Management::Unavailable => {}
            policy::Management::Exhausted | policy::Management::OperatorStep { .. } => {
                let reason = management
                    .operator_reason()
                    .unwrap_or(UserActionReasonV1::ManualInstructions);
                barrier.get_or_insert((id, reason));
            }
            policy::Management::Repairable { .. } => {
                // Retries drained: asking again is the operator's judgment
                // call, never the loop's.
                if task.state == State::Failed
                    && policy::failed_attempt_count(task) >= policy::RETRY_LIMIT
                {
                    barrier.get_or_insert((id, UserActionReasonV1::RepeatedFailure));
                    continue;
                }
                if !routes::can_request_fix(task) {
                    if task.state == State::Detected {
                        promotion_due = true;
                    }
                    continue;
                }
                let route = routes::fix_route(task).expect("repairable has a route");
                if slot_busy(input.slots, route, &id) {
                    return FlowAction::Hold;
                }
                return FlowAction::BeginFix { task_id: id, route };
            }
        }
    }
    if let Some((id, reason)) = barrier {
        return FlowAction::SyncPhase {
            phase: FlowPhaseV1::WaitingForUser {
                task_id: id,
                reason,
            },
            detail_task: None,
        };
    }
    if promotion_due {
        return rescan_or_hold(input);
    }
    // 4. Nothing left to repair or defer: gate completion on a fresh scan.
    if input.scan_generation > flow.last_scan_generation {
        return rescan_or_hold(input);
    }
    FlowAction::SyncPhase {
        phase: FlowPhaseV1::Finishing,
        detail_task: None,
    }
}

fn finishing(input: &BeatInput, flow: &SetupFlowStateV1) -> FlowAction {
    if input.scan_generation <= flow.last_scan_generation {
        if unresolved_active_count(input.tasks) == 0 {
            return FlowAction::Finish(done_outcome(input.tasks));
        }
        return back_to_running();
    }
    if input.probe_busy {
        return FlowAction::Hold;
    }
    rescan_or_hold(input)
}

fn rescan_or_hold(input: &BeatInput) -> FlowAction {
    if input.probe_busy {
        return FlowAction::Hold;
    }
    if input
        .last_rescan_at
        .is_some_and(|last| input.now_unix_ms.saturating_sub(last) < input.rescan_cooldown_ms)
    {
        return FlowAction::Hold;
    }
    FlowAction::TriggerRescan
}

/// Tasks the flow may still move: nothing verified, unsupported, or
/// superseded.
fn eligible_view(tasks: &[SetupTaskV1]) -> Vec<SetupTaskV1> {
    tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.state,
                State::Verified | State::Unsupported | State::Superseded
            )
        })
        .cloned()
        .collect()
}

fn is_skipped(flow: &SetupFlowStateV1, task_id: &str) -> bool {
    flow.skipped.iter().any(|id| id == task_id)
}

fn unresolved_active_count(tasks: &[SetupTaskV1]) -> usize {
    tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.state,
                State::Verified | State::Unsupported | State::Superseded
            )
        })
        .count()
}

fn done_outcome(tasks: &[SetupTaskV1]) -> DoneOutcomeV1 {
    let unavailable = tasks
        .iter()
        .filter(|task| task.state == State::Unsupported)
        .count() as u16;
    if unavailable == 0 {
        DoneOutcomeV1::FullyPrepared
    } else {
        DoneOutcomeV1::PreparedWithUnavailable {
            unavailable_count: unavailable,
        }
    }
}

#[cfg(test)]
mod tests;
