use crate::core::state::PlannerState;

const MAX_FEATURE_TARGETS: usize = 40;

pub(super) fn render(state: &PlannerState) -> String {
    let active_id = state.active_feature.as_ref().map(|(id, _)| id.as_str());
    let ordered_features = state
        .active_features
        .iter()
        .filter(|(id, _)| Some(id.as_str()) == active_id)
        .chain(
            state
                .active_features
                .iter()
                .filter(|(id, _)| Some(id.as_str()) != active_id),
        )
        .take(MAX_FEATURE_TARGETS)
        .collect::<Vec<_>>();
    let omitted_features = state
        .active_features
        .len()
        .saturating_sub(ordered_features.len());
    let mut features = ordered_features
        .iter()
        .map(|(id, body)| {
            let uid = crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .map(|identity| identity.uid)
                .unwrap_or_else(|| "no stable ID".into());
            let approved =
                crate::core::workflow::feature_approved(&state.repo_root, &state.workflow, id);
            let status = crate::domain::ChangeMetadata::require_markdown(body)
                .map(|metadata| metadata.status.wire_name().to_owned())
                .unwrap_or_else(|_| "invalid structured status".into());
            format!(
                "change {id}; target={uid}; status={status}; approval={}; active={}",
                if approved { "current" } else { "needed" },
                state
                    .active_feature
                    .as_ref()
                    .is_some_and(|(active, _)| active == id)
            )
        })
        .collect::<Vec<_>>();
    if omitted_features > 0 {
        features.push(format!(
            "{omitted_features} additional active changes are omitted from this bounded target list; ask the user for the exact change if needed"
        ));
    }
    let states = crate::core::implementation::load_board_states(&state.repo_root);
    let tasks = crate::artifacts::task_docs::load_board(&state.repo_root, &state.workflow)
        .into_iter()
        .filter(|document| !document.path.ends_with("/README.md"))
        .take(80)
        .map(|document| {
            let uid = document
                .identity
                .as_ref()
                .map(|identity| identity.uid.as_str())
                .unwrap_or("no stable ID");
            let display_id = document
                .identity
                .as_ref()
                .map(|identity| identity.display_id.as_str())
                .unwrap_or("legacy task");
            let status = states
                .get(&document.path)
                .map(|state| state.status.label())
                .unwrap_or("Not started");
            format!(
                "task {display_id} ({}) ; target={uid}; status={status}",
                crate::core::context_build::clip(&document.title, 100)
            )
        })
        .collect::<Vec<_>>();
    format!(
        "CURRENT PROJECT ACTION TARGETS\nChanges:\n{}\nTasks:\n{}",
        if features.is_empty() {
            "(none)".into()
        } else {
            features.join("\n")
        },
        if tasks.is_empty() {
            "(none)".into()
        } else {
            tasks.join("\n")
        }
    )
}
