use super::*;

/// Production boundary: a saved answer, conflicting active feature identity,
/// and native repair survive app restart without relinking or losing the
/// answer. Core refusal tests cannot prove the app leaves one actionable
/// recovery task. Local Git/planning/chat stores only; no provider, Docker, or
/// network; moderate app fixture setup.
#[test]
fn restart_conflict_keeps_saved_answer_and_one_actionable_recovery() {
    let (app, source_key, source_uid, feature_uid, answer_id) = failed_saved_answer_recovery(true);
    let mut restarted = restart_fixture(app);
    let Screen::Connected(project) = &mut restarted.screen else {
        unreachable!()
    };
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    project.task_chats.ensure_loaded(&project.chat_slug);
    let conflicting_uid =
        crate::domain::ArtifactIdentity::from_markdown(&project.state.active_features[0].1)
            .unwrap()
            .unwrap()
            .uid;
    assert_ne!(conflicting_uid, feature_uid);

    crate::app::manager::operational_recovery::advance(project);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        crate::app::manager::operational_recovery::advance(project);
        let recovery = project
            .planning_work
            .iter()
            .find(|work| {
                work.operational_recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.source_uid == source_uid)
            })
            .expect("durable recovery task remains the answer owner");
        if recovery.status == WorkStatus::NeedsAttention {
            assert!(recovery.detail.contains("different feature identity"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native conflict recovery timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    let recovery = project
        .planning_work
        .iter()
        .find(|work| {
            work.operational_recovery
                .as_ref()
                .is_some_and(|recovery| recovery.source_uid == source_uid)
        })
        .expect("saved blocker remains the answer owner after reload");
    let recovery_uid = recovery.uid.clone();
    assert_eq!(recovery.status, WorkStatus::NeedsAttention);
    assert!(recovery.detail.contains("different feature identity"));

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
        "unsafe identity recovery preserves the exact saved answer"
    );
    assert!(project.state.active_features.iter().any(|(_, body)| {
        crate::domain::ArtifactIdentity::from_markdown(body)
            .ok()
            .flatten()
            .is_some_and(|identity| identity.uid == conflicting_uid)
    }));
    crate::app::manager::operational_recovery::advance(project);
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    let recoveries = project
        .planning_work
        .iter()
        .filter(|work| {
            work.operational_recovery
                .as_ref()
                .is_some_and(|recovery| recovery.source_uid == source_uid)
        })
        .collect::<Vec<_>>();
    assert_eq!(recoveries.len(), 1, "identity conflict does not retry-loop");
    assert_eq!(recoveries[0].uid, recovery_uid);
    assert_eq!(recoveries[0].status, WorkStatus::NeedsAttention);
}
