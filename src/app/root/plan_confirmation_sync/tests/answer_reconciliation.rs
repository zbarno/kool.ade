use super::*;

/// Production boundary: the ordinary inactive-work sweep consumes a durable
/// applied answer, updates its TaskIntent parent, and creates the #568 plan
/// action from current spec/readiness evidence. Reducer tests cannot prove the
/// chat marker, resolved item, private work store, and app scheduler converge.
/// Dependencies: local Git/private-store fixture only; no provider or network.
/// Setup cost: one feature fixture per readiness branch.
#[test]
fn last_applied_answer_creates_one_ready_review_or_specific_missing_evidence_action() {
    for (tag, ready) in [("answer-ready", true), ("answer-blocked", false)] {
        let repo = repo(tag);
        let uid = feature_uid();
        let mut project = seed(&repo, &uid, ready);
        let root = project.planning_work.first_mut().expect("TaskIntent root");
        root.status = WorkStatus::NeedsAttention;
        root.task_intent_uid = Some(root.uid.clone());
        let root_uid = root.uid.clone();
        let question_key = "CLR-IT";
        let mut resolved = crate::domain::item::OpenItem::new(
            question_key.into(),
            crate::domain::item::Priority::Blocking,
            crate::domain::item::ItemKind::Question,
            "Product".into(),
            "Should views be local?".into(),
            "Storage scope blocks a safe implementation plan.".into(),
        );
        resolved.feature_uid = Some(uid.clone());
        resolved.status = crate::domain::item::ItemStatus::Resolved;
        project.state.resolved_items.push(resolved);
        let mut answer = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::User,
            "Store views on this device only.",
            None,
        );
        answer.clarification = false;
        let response = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::Agent,
            "The plan now treats saved views as local to this device.",
            None,
        );
        let mut applied = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::System,
            "The saved answer was applied to the current specification.",
            None,
        );
        applied.id = format!(
            "internal-task-reply-outcome:applied:{}",
            uuid::Uuid::new_v4()
        );
        project
            .task_chats
            .append(
                &project.chat_slug,
                question_key,
                vec![answer, response, applied],
            )
            .unwrap();
        assert!(!project.task_chats.has_pending_user_reply(question_key));
        assert!(!project.task_chats.has_failed_task_reply(question_key));
        assert!(project.state.resolved_items.iter().any(|item| {
            item.conversation_key() == question_key && item.feature_uid.as_deref() == Some(&uid)
        }));
        assert!(project.state.active_features.iter().any(|(_, body)| {
            crate::domain::ChangeMetadata::require_markdown(body).is_ok_and(|metadata| {
                metadata.uid == uid
                    && matches!(
                        metadata.status,
                        crate::domain::ChangeStatus::Draft | crate::domain::ChangeStatus::Ready
                    )
            })
        }));
        let mut direct = project.planning_work.clone();
        assert_eq!(direct[0].kind, WorkKind::Feature);
        assert_eq!(direct[0].status, WorkStatus::NeedsAttention);
        assert_eq!(direct[0].feature_uid.as_deref(), Some(uid.as_str()));
        assert!(crate::core::planning_work::reconcile_answered_child(
            &project.state,
            &project.state,
            question_key,
            &mut direct,
            &Default::default(),
        ));

        crate::app::root::tick::activity::reconcile_inactive_planning_work(&mut project);
        let refreshed_root = project
            .planning_work
            .iter()
            .find(|work| work.uid == root_uid)
            .unwrap();
        assert_eq!(refreshed_root.status, WorkStatus::InReview);
        let effects = crate::app::root::plan_confirmation_sync::sync(&mut project);
        assert_eq!(project.planning_work.len(), 2, "one canonical child action");
        let card = the_card(&project);
        assert_eq!(card.parent_uid.as_deref(), Some(root_uid.as_str()));
        let payload = card.plan_confirmation.as_ref().expect("typed plan payload");
        assert_eq!(payload.ready, ready);
        assert_eq!(payload.feature_uid, uid);
        let body = &project.state.active_features[0].1;
        let identity = crate::domain::ArtifactIdentity::from_markdown(body)
            .unwrap()
            .unwrap();
        let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "FEAT-IT",
            &identity,
            body,
            &crate::core::workflow::feature_contract(body),
        )
        .unwrap();
        assert_eq!(payload.snapshot_content_hash, snapshot.content_hash);
        assert_eq!(payload.snapshot_spec_revision, snapshot.spec_revision);
        if ready {
            assert_eq!(card.status, WorkStatus::InReview);
            assert!(payload.blockers.is_empty());
            assert!(effects.evaluations_wanted.is_empty());
        } else {
            assert_eq!(card.status, WorkStatus::NeedsAttention);
            assert!(!payload.blockers.is_empty());
            assert_eq!(effects.evaluations_wanted.len(), 1);
            assert_eq!(effects.evaluations_wanted[0].work_key, card.key);
        }
        let card_uid = card.uid.clone();
        let mut reloaded = project_from(&repo.root);
        reloaded.planning_work = crate::core::planning_work::load(&reloaded.state.planning_store)
            .expect("action task reloads from private state");
        assert_eq!(
            reloaded
                .planning_work
                .iter()
                .filter(|work| work.kind == WorkKind::PlanConfirmation)
                .count(),
            1
        );
        assert_eq!(the_card(&reloaded).uid, card_uid);
        remove_planning_store_for(&repo.root);
    }
}
