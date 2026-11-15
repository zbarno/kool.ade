use super::super::*;
use crate::domain::{ManagerRunEventV1, ManagerRunOutcomeV1 as Outcome};
use std::collections::BTreeMap;

pub(super) fn advance_pending(project: &mut crate::app::session::Project) {
    if project.activity.manager.is_some() {
        return;
    }
    if project.activity.manager_history_error.is_some() {
        return;
    }
    if let Some(error) = sequence_gap(&project.activity.manager_history) {
        let notice = format!(
            "Manager checkpoint replay is paused because its persisted event stream is malformed: {error}. The original supervision records are retained for review."
        );
        if !project
            .activity
            .manager_history
            .iter()
            .any(|message| message.text == notice)
        {
            project.remember_manager_activity(crate::domain::ChatMessage::new(
                crate::domain::ChatRole::System,
                &notice,
                None,
            ));
        }
        project.activity.manager_history_error = Some(notice);
        return;
    }
    let mut latest = BTreeMap::<String, ManagerRunEventV1>::new();
    let mut checkpoints = BTreeMap::new();
    for message in &project.activity.manager_history {
        let Some(event) = &message.manager_run_event else {
            continue;
        };
        if event.validate().is_err() {
            continue;
        }
        if let Some(checkpoint) = &event.checkpoint {
            checkpoints.insert(event.checkpoint_sha256.clone(), checkpoint.clone());
        }
        latest.insert(event.checkpoint_sha256.clone(), event.clone());
    }
    let now = Instant::now();
    let now_unix_ms = crate::persistence::harness_settings::current_unix_ms();
    for (digest, event) in latest {
        if !matches!(
            event.outcome,
            Outcome::Queued | Outcome::Started | Outcome::Retried
        ) {
            project.activity.manager_recovery_checked.remove(&digest);
            continue;
        }
        if event
            .retry_after_unix_ms
            .is_some_and(|deadline| now_unix_ms < deadline)
        {
            continue;
        }
        let Some(checkpoint) = checkpoints.get(&digest) else {
            terminal(
                project,
                &event,
                "Manager checkpoint is missing; saved supervision events need operator review.",
            );
            continue;
        };
        if project
            .activity
            .manager_recovery_checked
            .get(&digest)
            .is_some_and(|last| {
                now.saturating_duration_since(*last) < std::time::Duration::from_secs(5)
            })
        {
            continue;
        }
        project
            .activity
            .manager_recovery_checked
            .insert(digest.clone(), now);
        let Some(binding) = checkpoint.route.as_ref() else {
            terminal(
                project,
                &event,
                "The Manager route could not be recorded, so this event batch cannot resume safely.",
            );
            continue;
        };
        if crate::domain::chatlog::checkpoint_attempts(&project.activity.manager_history, &digest)
            >= 2
        {
            terminal(
                project,
                &event,
                "Manager recovery reached its two-run limit. Review the recorded failure before retrying.",
            );
            continue;
        }
        match crate::app::root::harness_selection::capability_refresh_route_state(
            binding,
            &project.state.harness_routes.routes,
            &Default::default(),
        ) {
            crate::app::root::harness_selection::CapabilityRefreshRouteState::Pending => {
                let deferred = ManagerRunEventV1 {
                    run_id: event.run_id.clone(),
                    sequence: event.sequence.saturating_add(1),
                    outcome: Outcome::Queued,
                    checkpoint: None,
                    checkpoint_cursor: checkpoint.cursor,
                    checkpoint_sha256: digest.clone(),
                    retry_after_unix_ms: Some(now_unix_ms.saturating_add(30_000)),
                };
                if let Err(error) = project.persist_manager_run_event(
                    deferred,
                    "Manager is recovering selected capability evidence before replay.",
                ) {
                    project.activity.manager_history_error = Some(format!(
                        "Manager capability retry time could not be saved: {error}"
                    ));
                }
            }
            crate::app::root::harness_selection::CapabilityRefreshRouteState::Ready => {
                if event.outcome != Outcome::Retried {
                    let retry = ManagerRunEventV1 {
                        run_id: event.run_id.clone(),
                        sequence: event.sequence.saturating_add(1),
                        outcome: Outcome::Retried,
                        checkpoint: None,
                        checkpoint_cursor: checkpoint.cursor,
                        checkpoint_sha256: digest.clone(),
                        retry_after_unix_ms: None,
                    };
                    if let Err(error) = project.persist_manager_run_event(
                        retry,
                        "Manager checkpoint reconciled; replaying the exact saved event batch.",
                    ) {
                        project.activity.manager_history_error = Some(format!(
                            "Manager checkpoint is ready but replay could not be recorded: {error}"
                        ));
                        continue;
                    }
                }
                for input in &checkpoint.events {
                    if !project.activity.pending.contains(&input.text) {
                        project.activity.pending.push(input.text.clone());
                    }
                }
                if project.activity.pending_baseline_review.is_none()
                    && let Some(review_id) = &checkpoint.baseline_review_id
                    && let Some(repository_id) = &checkpoint.baseline_repository_id
                {
                    project.activity.pending_baseline_review =
                        Some(crate::app::manager::BaselineReviewRequest {
                            review_id: review_id.clone(),
                            repository_id: repository_id.clone(),
                        });
                }
                project.activity.manager_recovery_checked.remove(&digest);
                project.activity.last_update = now.checked_sub(std::time::Duration::from_secs(31));
            }
            crate::app::root::harness_selection::CapabilityRefreshRouteState::RouteChanged => {
                terminal(
                    project,
                    &event,
                    "The selected Manager provider, role, or model changed while recovery was pending. Review and explicitly authorize the current route before retrying.",
                );
            }
            crate::app::root::harness_selection::CapabilityRefreshRouteState::Failed => {
                terminal(
                    project,
                    &event,
                    "The selected Manager provider did not produce current capability proof. Resolve its Coding tools diagnostic before retrying.",
                );
            }
        }
    }
}

fn sequence_gap(history: &[crate::domain::ChatMessage]) -> Option<String> {
    let mut previous = BTreeMap::<String, ManagerRunEventV1>::new();
    for event in history
        .iter()
        .filter_map(|message| message.manager_run_event.as_ref())
    {
        if let Err(error) = event.validate_after(previous.get(&event.run_id)) {
            return Some(format!(
                "run {} sequence {}: {error}",
                event.run_id, event.sequence
            ));
        }
        previous.insert(event.run_id.clone(), event.clone());
    }
    None
}

fn terminal(project: &mut crate::app::session::Project, event: &ManagerRunEventV1, detail: &str) {
    let failed = ManagerRunEventV1 {
        run_id: event.run_id.clone(),
        sequence: event.sequence.saturating_add(1),
        outcome: Outcome::Failed,
        checkpoint: None,
        checkpoint_cursor: event.checkpoint_cursor,
        checkpoint_sha256: event.checkpoint_sha256.clone(),
        retry_after_unix_ms: None,
    };
    if let Err(error) = project.persist_manager_run_event(failed, detail) {
        project.activity.manager_history_error = Some(format!(
            "Manager recovery blocker could not be persisted: {error}"
        ));
    }
    project.activity.last_manager_error = Some(detail.into());
    project
        .activity
        .manager_recovery_checked
        .remove(&event.checkpoint_sha256);
}
