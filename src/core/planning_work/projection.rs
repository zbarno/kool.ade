use super::{Work, WorkKind, WorkStatus, load};

/// Attach durable feature UIDs to migrated records using their legacy display
/// ID. Returns true when the caller should persist the updated records.
pub fn link_feature_identities(
    state: &crate::core::state::PlannerState,
    work: &mut [Work],
) -> bool {
    let mut changed = false;
    for item in work.iter_mut().filter(|item| item.feature_uid.is_none()) {
        let Some(feature_id) = &item.feature_id else {
            continue;
        };
        let Some((_, markdown)) = state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
        else {
            continue;
        };
        if let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(markdown) {
            item.feature_uid = Some(identity.uid);
            changed = true;
        }
    }
    changed
}

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
            card.status = WorkStatus::Done;
        }
    }
    for (id, body) in &state.active_features {
        let metadata = crate::domain::ChangeMetadata::require_markdown(body)
            .expect("loaded active changes always have validated structured status");
        let status = if matches!(
            metadata.status,
            crate::domain::ChangeStatus::Ready
                | crate::domain::ChangeStatus::Implementing
                | crate::domain::ChangeStatus::Reconciliation
        ) {
            WorkStatus::Done
        } else {
            WorkStatus::InProgress
        };
        let title = body
            .lines()
            .next()
            .unwrap_or(id)
            .trim_start_matches('#')
            .trim();
        if let Some(card) = cards.iter_mut().find(|work| {
            work.feature_uid.as_deref() == Some(&metadata.uid)
                || (work.feature_uid.is_none() && work.feature_id.as_deref() == Some(id))
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
                request: body.clone(),
                status,
                feature_id: Some(id.clone()),
                feature_uid: Some(metadata.uid),
                parent_uid: None,
                follow_up_task: None,
                detail: "Feature planning".into(),
            });
        }
    }
    cards
}

pub fn context(state: &crate::core::state::PlannerState, key: &str) -> Option<String> {
    let work = load(&state.repo_root).ok()?;
    let card = cards(state, &work).into_iter().find(|w| w.key == key)?;
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
