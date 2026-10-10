use std::time::Instant;

use crate::core::state::PlannerState;
use crate::error::AppError;
use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest};

pub(super) fn execute(
    generate_tasks: bool,
    harness: &dyn AiHarness,
    request: &PlanningRequest,
    state: &PlannerState,
    started: Instant,
) -> Result<(HarnessOutcome, Option<String>), AppError> {
    if generate_tasks {
        return crate::core::task_generation::generate(harness, request, state, started)
            .map(|generated| (generated.outcome, Some(generated.planning_revision)));
    }

    harness.execute(request).map(|outcome| (outcome, None))
}

pub(super) fn state_matches_revision(
    current: &PlannerState,
    base_snapshot: Option<&PlannerState>,
    generated_revision: Option<&str>,
) -> bool {
    let stable_snapshot = match base_snapshot {
        Some(base) => PlannerState::drift_report(base, current).is_empty(),
        None => true,
    };
    stable_snapshot
        && generated_revision.is_none_or(|revision| current.baseline_planning_revision == revision)
}

pub(super) fn adopt_revision(state: &mut PlannerState, generated_revision: Option<String>) {
    if let Some(revision) = generated_revision {
        state.baseline_planning_revision = revision;
    }
}
