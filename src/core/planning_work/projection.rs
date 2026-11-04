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
            card.status = WorkStatus::Done;
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
mod tests {
    use super::*;

    #[test]
    fn root_card_projects_the_current_follow_up_cycle_state() {
        let root_dir = std::env::temp_dir().join(format!(
            "koolade_intent_cycle_projection_{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        let state = crate::core::state::PlannerState::load(&root_dir).unwrap();
        let mut root = Work::new(
            "task:intent-root".into(),
            "Original request".into(),
            "Original goal".into(),
            String::new(),
        );
        root.kind = WorkKind::Question;
        root.status = WorkStatus::Done;
        root.task_intent_uid = Some(root.uid.clone());
        root.feature_uid = Some(uuid::Uuid::new_v4().to_string());
        let cycle_uid = uuid::Uuid::new_v4().to_string();
        let mut cycle_work = Work::new(
            "task:follow-up".into(),
            "Follow-up request".into(),
            "Current goal".into(),
            String::new(),
        );
        cycle_work.parent_uid = Some(root.uid.clone());
        cycle_work.task_intent_uid = Some(root.uid.clone());
        cycle_work.task_intent_cycle_uid = Some(cycle_uid.clone());
        cycle_work.status = WorkStatus::InProgress;
        root.task_intent_cycles = vec![
            crate::core::planning_work::TaskIntentCycleV1 {
                uid: root.uid.clone(),
                sequence: 1,
                goal: "Original goal".into(),
                work_uid: root.uid.clone(),
                started_at_ms: None,
            },
            crate::core::planning_work::TaskIntentCycleV1 {
                uid: cycle_uid.clone(),
                sequence: 2,
                goal: "Current goal".into(),
                work_uid: cycle_work.uid.clone(),
                started_at_ms: Some(1),
            },
        ];
        root.task_intent_cycle_uid = Some(cycle_uid);

        let cards = cards(&state, &[root.clone(), cycle_work]);
        let root_card = cards.iter().find(|card| card.uid == root.uid).unwrap();
        assert_eq!(root_card.status, WorkStatus::InProgress);
        assert_eq!(root_card.feature_uid, None);
        std::fs::remove_dir_all(root_dir).unwrap();
    }

    #[test]
    fn completed_draft_feature_is_review_not_active_work() {
        let root = std::env::temp_dir().join(format!(
            "koolade_planning_projection_{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut state = crate::core::state::PlannerState::load(&root).unwrap();
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
            crate::domain::ChangeStatus::Draft,
        )
        .unwrap();
        state.active_features.push(("CHG-001".into(), feature));
        let mut work = Work::new(
            "planning:example".into(),
            "Plan Example".into(),
            "Plan Example".into(),
            "Planning in progress".into(),
        );
        work.status = WorkStatus::InProgress;
        work.feature_id = Some("CHG-001".into());
        work.feature_uid = Some(identity.uid);

        let cards = cards(&state, &[work]);

        assert_eq!(cards[0].status, WorkStatus::InReview);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_feature_stays_open_while_code_implementation_is_unmerged() {
        let root = std::env::temp_dir().join(format!(
            "koolade_planning_implementation_{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut state = crate::core::state::PlannerState::load(&root).unwrap();
        let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
            "# CHG-002: Shared export\n\n**Status:** Ready\n",
            None,
            "CHG-002",
            "Shared export",
        )
        .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
            .unwrap()
            .unwrap();
        let feature = crate::domain::ChangeMetadata::write_markdown(
            &markdown,
            &identity,
            crate::domain::ChangeStatus::Implementing,
        )
        .unwrap();
        state.workflow.approved_features.insert(
            "CHG-002".into(),
            crate::core::workflow::feature_contract(&feature),
        );
        state.active_features.push(("CHG-002".into(), feature));
        let mut work = Work::new(
            "feature:shared-export".into(),
            "Plan shared export".into(),
            "Build shared export".into(),
            String::new(),
        );
        work.feature_id = Some("CHG-002".into());
        work.feature_uid = Some(identity.uid);

        let cards = cards(&state, &[work]);

        assert_eq!(cards[0].status, WorkStatus::InProgress);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_work_does_not_attach_to_a_reused_feature_id() {
        let root = std::env::temp_dir().join(format!(
            "koolade_planning_projection_identity_{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut state = crate::core::state::PlannerState::load(&root).unwrap();
        let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
            "# F1: Replacement feature\n\n**Status:** Draft\n",
            None,
            "F1",
            "Replacement feature",
        )
        .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
            .unwrap()
            .unwrap();
        let feature = crate::domain::ChangeMetadata::write_markdown(
            &markdown,
            &identity,
            crate::domain::ChangeStatus::Draft,
        )
        .unwrap();
        state.active_features.push(("F1".into(), feature));
        let mut legacy = Work::new(
            "planning:old-f1".into(),
            "Plan original F1".into(),
            "Original feature request".into(),
            "Historical planning work".into(),
        );
        legacy.feature_id = Some("F1".into());

        let cards = cards(&state, &[legacy]);

        assert_eq!(cards.len(), 2, "ambiguous legacy identity stays separate");
        assert_eq!(cards[0].key, "planning:old-f1");
        assert_eq!(cards[1].key, "feature:F1");
        std::fs::remove_dir_all(root).unwrap();
    }
}
