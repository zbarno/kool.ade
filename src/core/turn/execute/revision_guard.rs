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

pub(super) fn unchanged_specification_target(
    state: &PlannerState,
    target: &crate::core::specification_chat::Target,
) -> Result<(), String> {
    let current =
        PlannerState::load_with_store(&state.repo_root, &state.planning_store).map_err(|_| {
            "The published specification could not be refreshed. Retry after reconnecting."
                .to_owned()
        })?;
    crate::core::specification_chat::validate_target(&current, target)
        .map_err(|error| error.to_string())
}

pub(super) fn rejected(final_text: &str, elapsed: Duration) -> TurnOutcome {
    TurnOutcome::Rejected {
        problems: vec!["Planning files changed on disk while this turn was running, so nothing from it was saved. The submitted message remains in its conversation for an internal retry; do not ask the user to resend it.".into()],
        final_text: final_text.to_owned(),
        elapsed,
    }
}
