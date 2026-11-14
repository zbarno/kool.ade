/// Production boundary: failed proof refresh through Auto queue recovery must
/// persist a blocker and leave the ticket excluded from dispatch. Route-state
/// tests alone cannot prove the production queue keeps this ticket blocked.
/// Uses local app/store state and synthetic provider metadata; no network or
/// Docker; moderate headless fixture setup.
#[test]
fn failed_capability_refresh_persists_blocker_and_keeps_ticket_undispatched() {
    use crate::{
        app::root::Screen,
        core::implementation::{Failure, RecoveryDisposition},
        persistence::harness_settings::{
            DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
        },
    };

    let mut app = crate::app::root::board_tests::fixture();
    let settings_path = std::env::temp_dir().join(format!(
        "capability-refresh-failed-{}.json",
        uuid::Uuid::new_v4()
    ));
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("pinned-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                verified_at_unix_ms: None,
                status: "pi selected; proof refresh failed".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: Some("synthetic capability probe failure".into()),
                ready: false,
                models: vec!["pinned-model".into()],
                default_model: Some("pinned-model".into()),
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: 1,
                capability_snapshot: None,
            },
        )]),
        ..HarnessSettings::default()
    };
    let ticket;
    {
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        ticket = project.task_documents[0].path.clone();
        project.state.harness_routes.routes = settings.work_routes.clone();
        let binding = crate::harness::CapabilityRefreshBindingV1 {
            provider_id: "pi".into(),
            role: crate::execution::contract::HarnessRole::Implementer,
            work_category: IMPLEMENTATION.into(),
            selected_model: "pinned-model".into(),
            requested_model: Some("pinned-model".into()),
            observed_at_unix_ms: 1,
        };
        project.queue.auto_build = true;
        project.queue.blocked.insert(
            ticket.clone(),
            Failure::awaiting_capability_refresh("selected route proof expired", binding),
        );
        project
            .queue
            .blocked
            .get_mut(&ticket)
            .unwrap()
            .retry_after_unix_ms = None;
        project
            .queue
            .bind_task_documents(&project.task_documents)
            .unwrap();
        project
            .queue
            .save(&project.state.planning_store.root)
            .unwrap();
    }

    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        app.advance_auto_queue();
    });
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let blocked = project
        .queue
        .blocked
        .get(&ticket)
        .expect("ticket stays blocked");
    assert_eq!(blocked.recovery, RecoveryDisposition::UserAction);
    assert!(
        blocked
            .message
            .contains("did not produce fresh valid capability proof")
    );
    assert!(!project.active_implementations.contains_key(&ticket));
    assert!(!project.queue.in_flight.contains(&ticket));
    let mut persisted =
        crate::core::implementation_queue::Queue::load(&project.state.planning_store.root).unwrap();
    persisted
        .bind_task_documents(&project.task_documents)
        .unwrap();
    assert_eq!(persisted.blocked.get(&ticket), Some(blocked));
    let _ = std::fs::remove_file(settings_path);
}
