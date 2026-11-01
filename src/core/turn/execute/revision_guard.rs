use super::*;

pub(super) fn unchanged(
    state: &PlannerState,
    base_snapshot: Option<&PlannerState>,
    generation_revision: Option<&str>,
) -> bool {
    PlannerState::load_with_store(&state.repo_root, &state.planning_store).is_ok_and(|current| {
        generation::state_matches_revision(&current, base_snapshot, generation_revision)
    })
}

pub(super) fn rejected(final_text: &str, elapsed: Duration) -> TurnOutcome {
    TurnOutcome::Rejected {
        problems: vec!["Planning files changed on disk while this turn was running, so nothing from it was saved. The submitted message remains in its conversation for an internal retry; do not ask the user to resend it.".into()],
        final_text: final_text.to_owned(),
        elapsed,
    }
}
