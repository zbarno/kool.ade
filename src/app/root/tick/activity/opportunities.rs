use crate::{
    app::session::Project,
    core::opportunity::{OpportunityExploreModeV1, OpportunityStore},
};

const EXPLORATION_INTERVAL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

/// Queue one bounded Manager exploration per opted-in week, and only while
/// the workspace is otherwise idle. The normal Manager path still validates,
/// deduplicates, and applies the daily proposal quota.
pub(super) fn schedule_due(project: &mut Project) {
    if !is_idle(project) {
        return;
    }
    let store = OpportunityStore::new(&project.state.planning_store);
    let Ok(mut settings) = store.discovery_settings() else {
        return;
    };
    if settings.exploration != OpportunityExploreModeV1::OptedIn {
        return;
    }
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    if !exploration_due(&settings, now_ms) {
        return;
    }

    settings.last_exploration_at_ms = Some(now_ms);
    if store
        .save_discovery_settings(&settings, settings.revision)
        .is_err()
    {
        return;
    }
    project.activity.pending.push(
        "The user opted in to a bounded weekly opportunity exploration. Review only typed persisted workspace evidence and the validated generic capability inventory; propose at most the configured daily quota, and do not start implementation or planning without a separate user decision.".into(),
    );
}

fn exploration_due(
    settings: &crate::core::opportunity::OpportunityDiscoverySettingsV1,
    now_ms: u64,
) -> bool {
    settings.exploration == OpportunityExploreModeV1::OptedIn
        && settings
            .last_exploration_at_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= EXPLORATION_INTERVAL_MS)
}

fn is_idle(project: &Project) -> bool {
    project.active_turn.is_none()
        && project.active_planning_work.is_none()
        && project.activity.manager.is_none()
        && project.active_implementations.is_empty()
        && project.activity.pending.is_empty()
        && project.activity.pending_dependency_reviews.is_empty()
        && project.activity.pending_clarification_planner.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::opportunity::OpportunityDiscoverySettingsV1;

    #[test]
    fn weekly_scheduler_respects_opt_in_and_cooldown() {
        let mut settings = OpportunityDiscoverySettingsV1::default();
        let now = EXPLORATION_INTERVAL_MS * 2;
        assert!(!exploration_due(&settings, now));

        settings.exploration = OpportunityExploreModeV1::OptedIn;
        assert!(exploration_due(&settings, now));
        settings.last_exploration_at_ms = Some(now);
        assert!(!exploration_due(
            &settings,
            now + EXPLORATION_INTERVAL_MS - 1
        ));
        assert!(exploration_due(&settings, now + EXPLORATION_INTERVAL_MS));
    }

    #[test]
    fn application_scheduler_queues_exploration_only_after_explicit_opt_in() {
        let temp = tempfile::tempdir().unwrap();
        let mut project = crate::app::session::test_support::project_from(temp.path());
        schedule_due(&mut project);
        assert!(project.activity.pending.is_empty());

        let defaults = OpportunityStore::new(&project.state.planning_store)
            .discovery_settings()
            .unwrap();
        let opted_in = OpportunityDiscoverySettingsV1 {
            exploration: OpportunityExploreModeV1::OptedIn,
            ..defaults.clone()
        };
        OpportunityStore::new(&project.state.planning_store)
            .save_discovery_settings(&opted_in, defaults.revision)
            .unwrap();

        schedule_due(&mut project);
        assert_eq!(project.activity.pending.len(), 1);
        assert!(project.activity.pending[0].contains("opted in"));
        let settings = OpportunityStore::new(&project.state.planning_store)
            .discovery_settings()
            .unwrap();
        assert!(settings.last_exploration_at_ms.is_some());

        project.activity.pending.clear();
        schedule_due(&mut project);
        assert!(project.activity.pending.is_empty());
    }
}
