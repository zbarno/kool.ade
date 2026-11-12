use super::*;
use crate::{
    app::root::board_tests::restart_fixture,
    core::planning_work::{Work, WorkStatus},
    domain::{ChatMessage, ChatRole},
    persistence::task_chats::TaskChats,
};
use std::time::{Duration, Instant};

#[path = "native_feature_recovery/unsafe_conflict.rs"]
mod unsafe_conflict;

fn failed_saved_answer_recovery(
    conflicting_replacement: bool,
) -> (crate::app::root::KooladeApp, String, String, String, String) {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let source_text = "# F1: Original feature\n\n## Intent\n\nKeep this feature.\n\n## Current Behavior\n\nThe original behavior exists.\n\n## Desired Behavior\n\nPreserve the original direction.\n\n## Scope\n\nIdentity recovery only.\n\n## Affected Product Areas\n\nPlanning.\n\n## Requirements\n\n- Keep its immutable feature UID.\n\n## Decisions and Assumptions\n\nNo new decisions.\n\n## Acceptance Criteria\n\nThe saved answer remains attached to this feature.\n";
    let snapshot = crate::test_support::current_change_fixture(
        source_text,
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    let feature_uid = crate::domain::ArtifactIdentity::from_markdown(&snapshot)
        .unwrap()
        .unwrap()
        .uid;
    let changes = crate::core::apply::private_document_draft_changes(
        &project.state.planning_store,
        &[("feature:F1".into(), snapshot)],
    )
    .unwrap();
    project
        .state
        .planning_store
        .transaction(&changes, Some(&project.state.baseline_planning_revision))
        .unwrap();
    project.state = crate::core::state::PlannerState::load_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
    )
    .unwrap();
    if conflicting_replacement {
        let replacement = crate::test_support::current_change_fixture(
            source_text,
            crate::domain::ChangeStatus::Draft,
        )
        .unwrap();
        let changes = crate::core::apply::private_document_draft_changes(
            &project.state.planning_store,
            &[("feature:F1".into(), replacement)],
        )
        .unwrap();
        project
            .state
            .planning_store
            .transaction(&changes, Some(&project.state.baseline_planning_revision))
            .unwrap();
    } else {
        std::fs::remove_file(
            project
                .state
                .planning_store
                .root
                .join("state/spec_drafts/current.json"),
        )
        .unwrap();
    }
    project.state = crate::core::state::PlannerState::load_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
    )
    .unwrap();
    let mut source = Work::new(
        format!("task:{}", uuid::Uuid::new_v4()),
        "Original saved-answer task".into(),
        "Preserve the original feature direction".into(),
        String::new(),
    );
    source.uid = source.key.trim_start_matches("task:").to_owned();
    source.feature_id = Some("F1".into());
    source.feature_uid = Some(feature_uid.clone());
    source.task_intent_uid = Some(source.uid.clone());
    let source_key = source.key.clone();
    let source_uid = source.uid.clone();
    project.planning_work.push(source);
    project.save_planning_work().unwrap();
    project.bind_task_conversation_identities();

    let slug = project.chat_slug.clone();
    project.task_chats.ensure_loaded(&slug);
    let answer = ChatMessage::new(
        ChatRole::User,
        "Keep the submitted option unchanged",
        Some(source_key.clone()),
    );
    let answer_id = answer.id.clone();
    let mut failed = ChatMessage::new(
        ChatRole::System,
        format!("Planning task feature identity mismatch: expected {feature_uid}"),
        Some(source_key.clone()),
    );
    TaskChats::mark_task_reply_outcome(&mut failed);
    project
        .task_chats
        .append(&slug, &source_key, vec![answer, failed])
        .unwrap();
    (app, source_key, source_uid, feature_uid, answer_id)
}

/// Production boundary: a failed saved answer, private feature snapshot, and
/// source identity survive an app restart before the Manager's native repair
/// worker restores the exact UID. A reducer test cannot prove startup reload,
/// durable chat ownership, async worker adoption, and answer retention together.
/// Local file-backed planning/chat stores only; no provider, Docker, or network;
/// setup is one app fixture and one store reload.
#[test]
fn restart_restores_exact_feature_and_retains_saved_answer_for_guarded_replay() {
    let (app, source_key, source_uid, feature_uid, answer_id) = failed_saved_answer_recovery(false);
    assert!(
        matches!(&app.screen, Screen::Connected(project) if project.state.active_features.is_empty())
    );

    let mut restarted = restart_fixture(app);
    let Screen::Connected(project) = &mut restarted.screen else {
        unreachable!()
    };
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    project.task_chats.ensure_loaded(&project.chat_slug);
    crate::app::manager::operational_recovery::advance(project);
    assert!(project.activity.operational_recovery_worker.is_some());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        crate::app::manager::operational_recovery::advance(project);
        let restored = project.state.active_features.iter().any(|(id, markdown)| {
            id == "F1"
                && crate::domain::ArtifactIdentity::from_markdown(markdown)
                    .ok()
                    .flatten()
                    .is_some_and(|identity| identity.uid == feature_uid)
        });
        if restored {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native repair did not restore the exact persisted feature UID"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    project.task_chats.refresh_now(&project.chat_slug);
    let messages = project
        .task_chats
        .messages
        .get(&source_key)
        .expect("original saved conversation remains available");
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.role == ChatRole::User)
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        [answer_id.as_str()],
        "native recovery preserves one exact user answer without asking again"
    );
    let recovery = project
        .planning_work
        .iter()
        .find(|work| {
            work.operational_recovery
                .as_ref()
                .is_some_and(|recovery| recovery.source_uid == source_uid)
        })
        .expect("one durable recovery task remains the answer owner");
    let recovery_uid = recovery.uid.clone();
    assert_eq!(recovery.status, WorkStatus::InProgress);
    assert_eq!(
        recovery
            .operational_recovery
            .as_ref()
            .unwrap()
            .repair_attempts,
        1
    );
    crate::app::manager::operational_recovery::advance(project);
    let recoveries = project
        .planning_work
        .iter()
        .filter(|work| {
            work.operational_recovery
                .as_ref()
                .is_some_and(|recovery| recovery.source_uid == source_uid)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        recoveries.len(),
        1,
        "restart replay cannot duplicate recovery"
    );
    assert_eq!(recoveries[0].uid, recovery_uid);
    assert_eq!(
        recoveries[0]
            .operational_recovery
            .as_ref()
            .unwrap()
            .repair_attempts,
        1,
        "restored exact UID is not sent into a native repair loop"
    );
}
