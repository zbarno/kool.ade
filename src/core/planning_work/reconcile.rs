use super::{Work, WorkKind, WorkStatus};
use crate::{core::state::PlannerState, domain::ChangeStatus};

/// Select the settled board status for a planning turn that just completed.
pub fn completed_turn_status(
    kind: WorkKind,
    has_feature: bool,
    needs_input: bool,
    task_batch_created: bool,
    will_continue: bool,
) -> WorkStatus {
    if kind == WorkKind::TaskGeneration {
        if task_batch_created {
            WorkStatus::Done
        } else if needs_input {
            WorkStatus::NeedsAttention
        } else if will_continue {
            WorkStatus::InProgress
        } else {
            WorkStatus::NeedsAttention
        }
    } else if needs_input {
        WorkStatus::NeedsAttention
    } else if has_feature {
        WorkStatus::InReview
    } else {
        WorkStatus::Done
    }
}

/// Move persisted planning work out of In Progress when no live turn owns it.
/// Results are the user-visible summaries that the project manager can report.
pub fn reconcile_inactive(
    state: &PlannerState,
    work: &mut [Work],
    active_key: Option<&str>,
) -> Vec<String> {
    let mut changes = Vec::new();
    for item in work.iter_mut().filter(|item| {
        item.status == WorkStatus::InProgress && Some(item.key.as_str()) != active_key
    }) {
        let next = inactive_status(state, item);
        item.status = next;
        item.detail = match next {
            WorkStatus::Done => "The task already has a completed result.".into(),
            WorkStatus::InReview => {
                "Planning produced a draft specification. Review it or continue planning if it needs more detail.".into()
            }
            WorkStatus::NeedsAttention if item.kind == WorkKind::TaskGeneration => {
                "Task generation stopped before creating stories. Use Generate tasks to retry.".into()
            }
            WorkStatus::NeedsAttention => {
                "The previous planning turn ended without a final result. Continue this task in its conversation.".into()
            }
            WorkStatus::Todo | WorkStatus::InProgress => continue,
        };
        changes.push(format!("{} → {}", item.title, status_label(next)));
    }
    changes
}

fn inactive_status(state: &PlannerState, work: &Work) -> WorkStatus {
    let feature = state.active_features.iter().find_map(|(id, body)| {
        let metadata = crate::domain::ChangeMetadata::require_markdown(body).ok()?;
        let matches_id = work.feature_id.as_deref() == Some(id.as_str());
        let matches_uid = work.feature_uid.as_deref() == Some(metadata.uid.as_str());
        (matches_id || matches_uid).then_some((id.as_str(), body.as_str(), metadata.status))
    });

    if work.kind == WorkKind::TaskGeneration {
        return if feature.is_some_and(|(id, body, _)| has_task_batch(state, id, body)) {
            WorkStatus::Done
        } else {
            WorkStatus::NeedsAttention
        };
    }

    let Some((id, body, status)) = feature else {
        return WorkStatus::NeedsAttention;
    };
    let approved = state
        .workflow
        .approved_features
        .get(id)
        .is_some_and(|saved| *saved == crate::core::workflow::feature_contract(body));
    if approved
        || matches!(
            status,
            ChangeStatus::Implementing
                | ChangeStatus::Reconciliation
                | ChangeStatus::Implemented
                | ChangeStatus::Abandoned
        )
    {
        WorkStatus::Done
    } else if matches!(status, ChangeStatus::Draft | ChangeStatus::Ready) {
        WorkStatus::InReview
    } else {
        WorkStatus::NeedsAttention
    }
}

fn has_task_batch(state: &PlannerState, feature_id: &str, feature: &str) -> bool {
    let title = feature
        .lines()
        .next()
        .unwrap_or_default()
        .trim_start_matches('#')
        .split_once(':')
        .map(|(_, title)| title.trim());
    state.workflow.task_batches.iter().any(|batch| {
        batch.identity.as_ref().is_some_and(|identity| {
            state.active_features.iter().any(|(id, body)| {
                id == feature_id
                    && crate::domain::ArtifactIdentity::from_markdown(body)
                        .ok()
                        .flatten()
                        .is_some_and(|current| current.uid == identity.uid)
            })
        }) || crate::core::workflow::feature_ids_in(&batch.feature)
            .iter()
            .any(|id| id == feature_id)
            || title.is_some_and(|title| batch.feature.eq_ignore_ascii_case(title))
    })
}

fn status_label(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Todo => "To do",
        WorkStatus::InProgress => "In Progress",
        WorkStatus::InReview => "In Review",
        WorkStatus::NeedsAttention => "Needs Attention",
        WorkStatus::Done => "Done",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_planning_turns_leave_in_progress_only_for_generation_continuation() {
        assert_eq!(
            completed_turn_status(WorkKind::Feature, true, false, false, false),
            WorkStatus::InReview
        );
        assert_eq!(
            completed_turn_status(WorkKind::Question, false, false, false, false),
            WorkStatus::Done
        );
        assert_eq!(
            completed_turn_status(WorkKind::Feature, true, true, false, false),
            WorkStatus::NeedsAttention
        );
        assert_eq!(
            completed_turn_status(WorkKind::TaskGeneration, true, false, false, true),
            WorkStatus::InProgress
        );
        assert_eq!(
            completed_turn_status(WorkKind::TaskGeneration, true, false, true, false),
            WorkStatus::Done
        );
        assert_eq!(
            completed_turn_status(WorkKind::TaskGeneration, true, false, false, false),
            WorkStatus::NeedsAttention
        );
    }

    #[test]
    fn inactive_feature_moves_to_review_and_interrupted_work_needs_attention() {
        let root = std::env::temp_dir().join(format!(
            "koolade_stale_planning_{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut state = PlannerState::load(&root).unwrap();
        let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
            "# CHG-001: Example\n\n**Status:** Draft\n",
            None,
            "CHG-001",
            "Example",
        )
        .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
            .unwrap()
            .unwrap();
        let feature = crate::domain::ChangeMetadata::write_markdown(
            &markdown,
            &identity,
            ChangeStatus::Draft,
        )
        .unwrap();
        state.active_features.push(("CHG-001".into(), feature));

        let mut feature_work = Work::new(
            "planning:feature".into(),
            "Plan Example".into(),
            "Plan Example".into(),
            "Planning in progress".into(),
        );
        feature_work.feature_id = Some("CHG-001".into());
        let interrupted = Work::new(
            "planning:interrupted".into(),
            "Plan interrupted work".into(),
            "Plan interrupted work".into(),
            "Planning in progress".into(),
        );
        let active = Work::new(
            "planning:active".into(),
            "Plan active work".into(),
            "Plan active work".into(),
            "Planning in progress".into(),
        );
        let mut work = vec![feature_work, interrupted, active];

        let changes = reconcile_inactive(&state, &mut work, Some("planning:active"));

        assert_eq!(changes.len(), 2);
        assert_eq!(work[0].status, WorkStatus::InReview);
        assert_eq!(work[1].status, WorkStatus::NeedsAttention);
        assert_eq!(work[2].status, WorkStatus::InProgress);
        assert_eq!(work[2].key, "planning:active");
        super::super::save(&root, &work).unwrap();
        let persisted = super::super::load(&root).unwrap();
        assert_eq!(persisted[0].status, WorkStatus::InReview);
        assert_eq!(persisted[1].status, WorkStatus::NeedsAttention);
        assert_eq!(persisted[2].status, WorkStatus::InProgress);
        std::fs::remove_dir_all(root).unwrap();
    }
}
