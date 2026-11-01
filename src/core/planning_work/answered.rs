//! Refresh the parent of an applied child answer using current feature identity.
use super::{Work, WorkKind, WorkStatus};
use crate::core::state::PlannerState;

pub fn reconcile_answered_child(
    before: &PlannerState,
    after: &PlannerState,
    key: &str,
    work: &mut [Work],
    excluded_keys: &std::collections::BTreeSet<String>,
) -> bool {
    let Some(child) = before
        .items
        .iter()
        .chain(&before.resolved_items)
        .find(|item| item.conversation_key() == key)
    else {
        return false;
    };
    let Some(uid) = child.feature_uid.as_deref() else {
        return false;
    };
    if !after.active_features.iter().any(|(_, body)| {
        crate::domain::ChangeMetadata::require_markdown(body).is_ok_and(|metadata| {
            metadata.uid == uid
                && matches!(
                    metadata.status,
                    crate::domain::ChangeStatus::Draft | crate::domain::ChangeStatus::Ready
                )
        })
    }) {
        return false;
    }
    let remaining = after
        .items
        .iter()
        .filter(|item| item.feature_uid.as_deref() == Some(uid))
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    let mut changed = false;
    for parent in work.iter_mut().filter(|parent| {
        parent.feature_uid.as_deref() == Some(uid)
            && matches!(
                parent.kind,
                WorkKind::Feature | WorkKind::Bug | WorkKind::NewProject
            )
            && matches!(
                parent.status,
                WorkStatus::NeedsAttention | WorkStatus::InReview
            )
            && !excluded_keys.contains(&parent.key)
    }) {
        let (status, detail) = if remaining.is_empty() {
            (WorkStatus::InReview, "Saved answers have been applied. Review the draft specification; implementation still requires explicit approval.".to_owned())
        } else {
            (
                WorkStatus::NeedsAttention,
                format!(
                    "Planning is waiting on unresolved items: {}. Saved answers remain in their conversations; review the remaining items for the next action.",
                    remaining.join(", ")
                ),
            )
        };
        if parent.status != status || parent.detail != detail {
            parent.status = status;
            parent.detail = detail;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests;
