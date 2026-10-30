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
