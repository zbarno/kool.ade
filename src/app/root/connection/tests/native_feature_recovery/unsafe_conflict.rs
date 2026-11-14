use super::*;
use sha2::Digest;

/// Production boundary: a saved answer, conflicting active feature identity,
/// and native repair survive app restart without relinking or losing the
/// answer. Core refusal tests cannot prove the app leaves one actionable
/// recovery task. Local Git/planning/chat stores only; no provider, Docker, or
/// network; moderate app fixture setup.
#[test]
fn replacement_status_conflict_keeps_saved_answer_and_one_actionable_recovery() {
    for replacement_status in [
        crate::domain::ChangeStatus::Draft,
        crate::domain::ChangeStatus::Ready,
        crate::domain::ChangeStatus::Implemented,
        crate::domain::ChangeStatus::Abandoned,
    ] {
        assert_conflict_is_blocked(replacement_status);
    }
}

/// Production boundary: startup loads a valid project draft, then native
/// recovery scans a malformed immutable feature snapshot and must retain the
/// original answer while creating one durable blocker. Validator tests cannot
/// prove app restart, async triage ownership, and saved-answer retention
/// together. Local synthetic stores only; no provider, Docker, or network;
/// setup is one app fixture and restart.
#[test]
fn malformed_historical_identity_snapshot_keeps_answer_and_surfaces_one_blocker() {
    let (mut app, source_key, source_uid, _feature_uid, answer_id) =
        failed_saved_answer_recovery(None);
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let store = &project.state.planning_store;
    let snapshots = store.list_files("state/spec_drafts").unwrap();
    let mut original = None;
    for entry in snapshots
        .into_iter()
        .filter(|entry| entry.name.ends_with(".json") && entry.name.len() == 69)
    {
        let bytes = store
            .read(format!("state/spec_drafts/{}", entry.name))
            .unwrap();
        let mut snapshot: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let Some(change) = snapshot
            .get_mut("changes")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|changes| {
                changes.iter_mut().find(|change| {
                    change.get("documentId").and_then(serde_json::Value::as_str)
                        == Some("feature:F1")
                })
            })
        else {
            continue;
        };
        let content = change
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .to_owned();
        original = Some((snapshot, content));
        break;
    }
    let original =
        original.expect("the exact feature snapshot remains available after current draft removal");
    let (mut snapshot, content) = original;
    let malformed = content.replacen(
        "<!-- koolade-artifact-id:v1 ",
        "<!-- koolade-artifact-id:v1 {",
        1,
    );
    assert_ne!(
        malformed, content,
        "fixture contains a feature identity marker"
    );
    snapshot["changes"][0]["content"] = serde_json::Value::String(malformed);
    let bytes = serde_json::to_vec_pretty(&snapshot).unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    let malformed_path = format!("state/spec_drafts/{digest}.json");
    store.atomic_write(&malformed_path, &bytes).unwrap();
    let mut restarted = restart_fixture(app);
    let Screen::Connected(project) = &mut restarted.screen else {
        unreachable!()
    };
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    project.task_chats.ensure_loaded(&project.chat_slug);
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
            .expect("one recovery task owns the answer");
        if recovery.status == WorkStatus::NeedsAttention {
            assert!(
                recovery
                    .detail
                    .contains("Native investigation could not safely repair")
            );
            assert!(recovery.detail.contains("key must be a string"));
            break;
        }
        assert!(Instant::now() < deadline, "native triage timed out");
        std::thread::sleep(Duration::from_millis(5));
    }

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
    assert_eq!(recoveries.len(), 1);
    assert_eq!(recoveries[0].status, WorkStatus::NeedsAttention);
    assert!(
        recoveries[0]
            .detail
            .contains("Native investigation could not safely repair")
    );
    assert!(recoveries[0].detail.contains("key must be a string"));
    let recovery_uid = recoveries[0].uid.clone();
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
        [answer_id.as_str()]
    );
    assert_eq!(
        project.state.planning_store.read(&malformed_path).unwrap(),
        bytes,
        "unsafe snapshot is preserved for diagnosis"
    );
    crate::app::manager::operational_recovery::advance(project);
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    let retained = project
        .planning_work
        .iter()
        .filter(|work| {
            work.operational_recovery
                .as_ref()
                .is_some_and(|recovery| recovery.source_uid == source_uid)
        })
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 1, "malformed evidence does not retry-loop");
    assert_eq!(retained[0].uid, recovery_uid);
    assert_eq!(retained[0].status, WorkStatus::NeedsAttention);
}

fn assert_conflict_is_blocked(replacement_status: crate::domain::ChangeStatus) {
    let (app, source_key, source_uid, feature_uid, answer_id) =
        failed_saved_answer_recovery(Some(replacement_status));
    let mut restarted = restart_fixture(app);
    let Screen::Connected(project) = &mut restarted.screen else {
        unreachable!()
    };
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    project.task_chats.ensure_loaded(&project.chat_slug);
    let conflicting_metadata = replacement_metadata(project);
    let conflicting_uid = conflicting_metadata.uid.clone();
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
    let retained_metadata = replacement_metadata(project);
    assert_eq!(retained_metadata.uid, conflicting_uid);
    assert_eq!(retained_metadata.status, replacement_status);
    if replacement_status.is_terminal() {
        assert!(project.state.active_features.iter().all(|(_, body)| {
            crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .is_none_or(|identity| identity.uid != conflicting_uid)
        }));
    }
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

fn replacement_metadata(project: &crate::app::session::Project) -> crate::domain::ChangeMetadata {
    let draft = crate::core::apply::load_current(&project.state.planning_store)
        .unwrap()
        .expect("conflicting replacement draft remains durable");
    let markdown = draft
        .changes
        .iter()
        .find(|change| change.document_id == "feature:F1")
        .expect("replacement feature remains in the draft")
        .content
        .as_str();
    crate::domain::ChangeMetadata::require_markdown(markdown).unwrap()
}
