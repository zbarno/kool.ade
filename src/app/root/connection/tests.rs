use crate::core::planning_work::{Work, WorkKind};
use crate::persistence::harness_settings::{
    DISCOVERY_FRESHNESS_MS, DetectedHarness, HarnessSettings, current_unix_ms, save,
    with_settings_path,
};

/// Production boundary: stale route rejection must happen before a task reply
/// is appended, its draft cleared, or a turn started.
/// Routing tests alone cannot prove this app-level preservation invariant.
/// Uses temporary planning/settings files only; no Docker, SSH, or network;
/// setup cost is low.
#[test]
fn stale_discovery_keeps_task_reply_as_a_draft_until_refresh() {
    let settings_dir = tempfile::tempdir().unwrap();
    let settings_path = settings_dir.path().join("harness-settings.json");
    with_settings_path(settings_path, || {
        let now = current_unix_ms();
        let stale = DetectedHarness {
            status: "pi 0.9.0".into(),
            version: Some("0.9.0".into()),
            executable: Some("/example/pi".into()),
            diagnostic: None,
            ready: true,
            models: vec!["example-model".into()],
            default_model: Some("example-model".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            implementation_available: true,
            capability_snapshot: None,
            checked_at_unix_ms: now.saturating_sub(DISCOVERY_FRESHNESS_MS + 1),
        };
        save(&HarnessSettings {
            default_harness: Some("pi".into()),
            discovered: std::collections::BTreeMap::from([("pi".into(), stale)]),
            ..Default::default()
        })
        .unwrap();

        let mut app = crate::app::root::board_tests::fixture();
        let key = "planning:doc-refresh";
        let mut work = Work::new(
            key.into(),
            "Refresh Documentation".into(),
            "Refresh the project documentation from repository evidence.".into(),
            "Continue the existing documentation refresh.".into(),
        );
        work.kind = WorkKind::DocumentationRefresh;
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture must be connected")
        };
        crate::core::planning_work::save(&project.state.planning_store, &[work]).unwrap();
        project
            .task_chats
            .drafts
            .insert(key.into(), "Continue the documentation refresh.".into());

        app.submit_task_reply(key);

        let crate::app::root::Screen::Connected(project) = &app.screen else {
            panic!("fixture must remain connected")
        };
        assert_eq!(
            project.task_chats.drafts.get(key).map(String::as_str),
            Some("Continue the documentation refresh.")
        );
        assert!(!project.task_chats.messages.contains_key(key));
        assert!(!project.task_turns.contains_key(key));
        assert!(
            project
                .activity
                .pending
                .iter()
                .all(|event| !event.contains("User replied in task planning:doc-refresh"))
        );
    });
}
use super::*;

#[path = "tests/exhausted_retry.rs"]
mod exhausted_retry;

struct WaitingHarness;

impl crate::harness::AiHarness for WaitingHarness {
    fn label(&self) -> String {
        "retry fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        while !request.cancel.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Err(crate::error::AppError::Other("cancelled fixture".into()))
    }
}

/// Production boundary: a persisted final planning-revision conflict must
/// restart the same task reply without appending a duplicate user answer.
/// Store predicate tests alone cannot establish that the app reconstructs the
/// original task-scoped turn. Uses temporary local planning files; no Docker,
/// SSH, or network; setup cost is low.
#[test]
fn pending_conflict_reply_restarts_once_in_its_task_conversation() {
    let mut app = crate::app::root::board_tests::fixture();
    app.task_harness = Some(Box::new(WaitingHarness));
    let key = "planning:internal-retry";
    let slug;
    if let Screen::Connected(project) = &mut app.screen {
        let mut work = crate::core::planning_work::Work::new(
            key.into(),
            "Continue internal retry".into(),
            "Continue using the already saved decision.".into(),
            "The planning files changed while the turn was running.".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::Bug;
        project.planning_work.push(work);
        project.save_planning_work().unwrap();
        slug = project.chat_slug.clone();
        project.task_chats.ensure_loaded(&slug);
        let user = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::User,
            "Use the verified release branch.",
            Some(key.into()),
        );
        let mut conflict = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::System,
            "Planning change was not applied; the submitted message is preserved for an internal retry.",
            None,
        );
        crate::persistence::task_chats::TaskChats::mark_planning_conflict_retry(&mut conflict);
        project
            .task_chats
            .append(&slug, key, vec![user, conflict])
            .unwrap();
    } else {
        panic!("fixture must be connected");
    }

    app.retry_pending_task_reply();

    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must remain connected");
    };
    assert!(
        project.task_turns.contains_key(key),
        "retry did not start: error={:?}, pending={:?}, conversations={:?}",
        project.queue.last_error,
        project.task_chats.pending_planning_conflict_retries(key),
        project.task_chats.messages.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        project.task_chats.messages[key]
            .iter()
            .filter(|message| message.role == crate::domain::ChatRole::User)
            .count(),
        1
    );
    assert!(project.task_chats.messages[key].iter().any(|message| {
        message.text == "Kool.ad/e is retrying this saved task reply internally."
    }));
    assert_eq!(
        project.task_chats.pending_planning_conflict_retries(key),
        None
    );
    project.task_turns[key].request_cancel();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if matches!(
            project.task_turns[key].poll(std::time::Duration::from_millis(10)),
            Some(crate::core::turn::TurnEvt::Done(_))
        ) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "retry worker did not stop"
        );
    }
    project.task_turns.remove(key);
    std::fs::remove_dir_all(slug).unwrap();
}
