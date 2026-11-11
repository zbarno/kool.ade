use super::{Work, WorkKind, WorkStatus, load};

/// Feature documents are durable board identity even after the chat is gone.
pub fn cards(state: &crate::core::state::PlannerState, work: &[Work]) -> Vec<Work> {
    let mut cards = work.to_vec();
    let active_uids = state
        .active_features
        .iter()
        .filter_map(|(id, body)| {
            crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .map(|identity| (id.as_str(), identity.uid))
        })
        .collect::<Vec<_>>();
    for card in &mut cards {
        let identity_match = card
            .feature_uid
            .as_ref()
            .is_some_and(|uid| active_uids.iter().any(|(_, active_uid)| active_uid == uid));
        if card.feature_uid.is_some()
            && !identity_match
            && card.status != WorkStatus::NeedsAttention
        {
            let hidden_draft = state.local_spec_draft.as_ref().is_some_and(|draft| {
                draft.base_published_spec_pin != state.published_spec_pin
                    && draft.changes.iter().any(|change| {
                        change.document_id.starts_with("feature:")
                            && crate::domain::ArtifactIdentity::from_markdown(&change.content)
                                .ok()
                                .flatten()
                                .is_some_and(|identity| {
                                    Some(&identity.uid) == card.feature_uid.as_ref()
                                })
                    })
            });
            if hidden_draft {
                card.status = WorkStatus::NeedsAttention;
                card.detail = "Agent recovery blocked: the private feature draft is based on an older published revision. Reconcile that draft before resuming; the original task and saved answers are preserved.".into();
            } else {
                card.status = WorkStatus::Done;
            }
        }
    }
    for (id, body) in &state.active_features {
        let metadata = crate::domain::ChangeMetadata::require_markdown(body)
            .expect("loaded active changes always have validated structured status");
        let approved =
            crate::core::workflow::feature_approved(&state.planning_store, &state.workflow, id);
        let status = match metadata.status {
            crate::domain::ChangeStatus::Draft | crate::domain::ChangeStatus::Ready if approved => {
                WorkStatus::Done
            }
            crate::domain::ChangeStatus::Draft | crate::domain::ChangeStatus::Ready => {
                WorkStatus::InReview
            }
            crate::domain::ChangeStatus::Implementing => WorkStatus::InProgress,
            crate::domain::ChangeStatus::Reconciliation => WorkStatus::NeedsAttention,
            crate::domain::ChangeStatus::Implemented | crate::domain::ChangeStatus::Abandoned => {
                WorkStatus::Done
            }
        };
        let title = body
            .lines()
            .next()
            .unwrap_or(id)
            .trim_start_matches('#')
            .trim();
        if let Some(card) = cards.iter_mut().find(|work| {
            !matches!(
                work.kind,
                WorkKind::TaskGeneration
                    | WorkKind::PlanReviewDisposition
                    | WorkKind::PlanConfirmation
                    | WorkKind::CodeReviewDisposition
                    | WorkKind::QaFindingDisposition
            ) && work.feature_uid.as_deref() == Some(&metadata.uid)
        }) {
            if card.status != WorkStatus::NeedsAttention {
                card.status = status;
            }
            card.feature_id = Some(id.clone());
            card.feature_uid = Some(metadata.uid.clone());
            card.title = format!("Plan {title}");
        } else {
            cards.push(Work {
                uid: metadata.uid.clone(),
                key: format!("feature:{id}"),
                kind: WorkKind::Feature,
                title: format!("Plan {title}"),
                original_goal: Some(body.clone()),
                request: body.clone(),
                status,
                feature_id: Some(id.clone()),
                feature_uid: Some(metadata.uid),
                parent_uid: None,
                task_intent_uid: None,
                task_intent_history: Vec::new(),
                task_decisions: Vec::new(),
                task_intent_cycles: Vec::new(),
                task_intent_cycle_uid: None,
                task_intent_archived_at_ms: None,
                git_intent: None,
                git_intents: std::collections::BTreeMap::new(),
                source_branch: None,
                destination_branch: None,
                routing_overrides: Default::default(),
                routing_inherited_from: None,
                follow_up_task: None,
                code_review_disposition: None,
                operational_recovery: None,
                plan_confirmation: None,
                detail: "Feature planning".into(),
                record_revision: 0,
                record_baseline: None,
            });
        }
    }
    for card in cards.iter_mut().filter(|work| {
        work.parent_uid.is_none() && work.task_intent_root_uid() == Some(work.uid.as_str())
    }) {
        let Some(cycle) = card.task_intent_cycles.last() else {
            continue;
        };
        if cycle.work_uid == card.uid {
            continue;
        }
        if let Some(current) = work.iter().find(|work| work.uid == cycle.work_uid) {
            card.status = current.status;
            card.feature_id = current.feature_id.clone();
            card.feature_uid = current.feature_uid.clone();
        }
    }
    cards
}

pub fn context(state: &crate::core::state::PlannerState, key: &str) -> Option<String> {
    let card = find_card(state, key)?;
    let feature = card.feature_uid.as_ref().and_then(|uid| {
        state.active_features.iter().find(|(_, body)| {
            crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .is_some_and(|identity| &identity.uid == uid)
        })
    });
    Some(format!(
        "Task type: {}\n{}\n{}\n{}\n{}",
        card.kind.label(),
        card.title,
        card.request,
        card.detail,
        feature
            .map(|(_, body)| body.as_str())
            .unwrap_or("No feature specification recorded yet.")
    ))
}

/// Resolve a visible planning card by its conversation key, including feature
/// cards projected directly from feature documents.
pub fn find_card(state: &crate::core::state::PlannerState, key: &str) -> Option<Work> {
    let work = load(&state.planning_store).ok()?;
    cards(state, &work).into_iter().find(|work| work.key == key)
}

#[cfg(test)]
mod tests;
