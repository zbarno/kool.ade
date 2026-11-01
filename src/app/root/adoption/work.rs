use super::*;

pub(super) fn update(
    project: &mut Project,
    key: &str,
    outcome: &TurnOutcome,
    will_continue: bool,
    superseded_reply: bool,
) {
    let Some(work) = project.planning_work.iter_mut().find(|work| {
        work.key == key
            && !project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
    }) else {
        return;
    };

    match outcome {
        TurnOutcome::Applied {
            state, normalized, ..
        } => {
            let model_offer = normalized.follow_up_task.clone().map(|offer| {
                crate::core::planning_work::FollowUpTaskOffer {
                    title: offer.title,
                    description: offer.description,
                }
            });
            work.follow_up_task = model_offer
                .or_else(|| crate::app::root::planning_work::provider_support_follow_up(work));
            if work.kind != crate::core::planning_work::WorkKind::TaskGeneration
                && let Some((feature_id, feature_uid)) = normalized
                    .document_updates
                    .iter()
                    .find_map(|(document_id, _)| {
                        let feature_id = document_id.strip_prefix("feature:")?;
                        let markdown = &state
                            .active_features
                            .iter()
                            .find(|(id, _)| id == feature_id)?
                            .1;
                        let identity =
                            crate::domain::ArtifactIdentity::from_markdown(markdown).ok()??;
                        Some((feature_id.to_owned(), identity.uid))
                    })
            {
                let link_is_safe = match work.feature_uid.as_deref() {
                    Some(expected_uid) => expected_uid == feature_uid,
                    None => work.feature_id.is_none(),
                };
                if link_is_safe {
                    work.feature_id = Some(feature_id);
                    work.feature_uid = Some(feature_uid);
                }
            }
            let needs_input = normalized.next_question_id.is_some()
                || normalized.planning_tasks.iter().any(|task| {
                    task.status == crate::core::planning_work::WorkStatus::NeedsAttention
                })
                || normalized.added.iter().any(|item| {
                    matches!(
                        item.authority,
                        crate::domain::Authority::Human | crate::domain::Authority::Review
                    )
                });
            work.status = if superseded_reply {
                crate::core::planning_work::WorkStatus::NeedsAttention
            } else {
                crate::core::planning_work::completed_turn_status(
                    work.kind,
                    work.feature_id.is_some(),
                    needs_input,
                    normalized.task_batch.is_some(),
                    will_continue,
                )
            };
            work.detail = if superseded_reply {
                "An earlier saved reply was applied before a newer reply arrived. The newer reply remains saved for internal processing.".into()
            } else {
                if work.kind == crate::core::planning_work::WorkKind::PlanReviewDisposition {
                    format!(
                        "{}\n\nPlanner update: {}",
                        work.detail, normalized.assistant_message
                    )
                } else if work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                    && normalized.task_batch.is_some()
                {
                    "Implementation tasks were generated and added to the Kanban board.".into()
                } else {
                    normalized.assistant_message.clone()
                }
            };
        }
        TurnOutcome::HarnessFailed { error, .. } => {
            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
            work.detail = format!(
                "Planning needs attention before it can continue.\n\n{}\n\nNext action: fix the reported setup or provider issue, then retry this task.",
                error.detail()
            );
        }
        TurnOutcome::Rejected { problems, .. }
            if problems
                .iter()
                .any(|problem| problem.starts_with("Planning task feature identity mismatch:")) =>
        {
            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
            work.detail = "This task's linked feature was replaced or removed. Kool.ad/e did not retarget this conversation by display ID. To continue against the current feature, create a new planning task from that feature; this task remains available for reference.".into();
        }
        _ => {
            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
            work.detail =
                "Planning needs attention; continue this request in its conversation.".into();
        }
    }
}
