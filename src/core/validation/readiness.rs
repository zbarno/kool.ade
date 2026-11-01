//! Keep change metadata consistent with a readiness assessment for that feature.
use super::NormalizedTurn;
use crate::{core::state::PlannerState, domain::ChangeStatus};

pub(super) fn align_change_status(state: &PlannerState, turn: &mut NormalizedTurn) {
    let workflow = turn.workflow.as_ref().unwrap_or(&state.workflow);
    for (id, status) in &mut turn.change_status_updates {
        if *status != ChangeStatus::Ready {
            continue;
        }
        let existing = state
            .active_features
            .iter()
            .find(|(feature, _)| feature == id)
            .map(|(_, body)| body.as_str());
        let body = turn
            .document_updates
            .iter()
            .find(|(document, _)| document == &format!("feature:{id}"))
            .map(|(_, body)| body.as_str())
            .or(existing)
            .unwrap_or("");
        let title = feature_title(body);
        let unique_title = title.is_some_and(|title| {
            !state.active_features.iter().any(|(other, body)| {
                other != id
                    && feature_title(body).is_some_and(|other| title.eq_ignore_ascii_case(other))
            }) && !turn.document_updates.iter().any(|(document, body)| {
                document != &format!("feature:{id}")
                    && document.starts_with("feature:")
                    && feature_title(body).is_some_and(|other| title.eq_ignore_ascii_case(other))
            })
        });
        let matching = workflow.brief.as_ref().filter(|brief| {
            let ids = crate::core::workflow::feature_ids_in(&brief.feature_name);
            if ids.is_empty() {
                unique_title
                    && title.is_some_and(|title| brief.feature_name.eq_ignore_ascii_case(title))
            } else {
                ids == [id.clone()]
            }
        });
        let ready = matching.is_some_and(|brief| brief.ready_for_tasks);
        let unresolved =
            state.items.iter().any(|item| {
                item.feature_id.as_ref() == Some(id) && !turn.resolved.contains(&item.id)
            }) || turn
                .added
                .iter()
                .any(|item| item.feature_id.as_ref() == Some(id));
        // An unrelated global interview cannot invalidate an unchanged Ready
        // assessment. Changed scope or remaining work still invalidates it.
        if matching.is_none()
            && !unresolved
            && existing.is_some_and(|old| {
                crate::domain::ChangeMetadata::require_markdown(old)
                    .is_ok_and(|metadata| metadata.status == ChangeStatus::Ready)
                    && crate::core::workflow::feature_contract(old)
                        == crate::core::workflow::feature_contract(body)
            })
        {
            continue;
        }
        if !ready || unresolved {
            *status = ChangeStatus::Draft;
            turn.warnings.push(format!("{id} remains Draft: scoped interview readiness and unresolved planning work must be reconciled before it is Ready. Saved answers were applied; no implementation approval was granted."));
        }
    }
}

fn feature_title(body: &str) -> Option<&str> {
    body.lines()
        .find(|line| line.starts_with("# "))?
        .split_once(':')
        .map(|(_, title)| title.trim())
}

#[cfg(test)]
mod tests;
