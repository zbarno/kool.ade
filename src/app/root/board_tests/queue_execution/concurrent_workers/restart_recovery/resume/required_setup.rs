use crate::{
    app::root::KooladeApp,
    persistence::setup_tasks::{
        FlowPhaseV1, SetupFlowStateV1, SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1,
        SetupTaskV1, UserActionReasonV1,
    },
};

/// Persist a required machine setup task waiting on an OS action, then let the
/// production setup pump load it and verify that it remains pending. The
/// caller can dispatch unrelated project work while this flow is waiting.
pub(super) fn persist_waiting_required_setup(app: &mut KooladeApp) -> &'static str {
    let id = "setup:computer:kvm";
    let state = SetupTaskStateV1::ReadyToFix;
    let mut flow = SetupFlowStateV1::start_new("flow-required-dispatch".into(), 1).unwrap();
    flow.last_scan_generation = 1;
    flow.phase = FlowPhaseV1::WaitingForUser {
        task_id: id.into(),
        reason: UserActionReasonV1::OperatingSystemPrompt,
    };
    let task = SetupTaskV1 {
        schema_version: 1,
        id: id.into(),
        title: "Enable virtualization".into(),
        explanation: "A required runtime needs hardware virtualization.".into(),
        next_step: "Enable virtualization in system settings, then check again.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: vec![SetupTaskEventV1 {
            observed_at_unix_ms: 1,
            scan_generation: 1,
            state,
        }],
        superseded_by: Vec::new(),
    };
    let store = SetupTaskStoreV1 {
        latest_scan_generation: 1,
        tasks: [(id.into(), task)].into_iter().collect(),
        setup_flow: Some(flow),
        ..SetupTaskStoreV1::default()
    };
    store.validate().unwrap();
    let path = crate::persistence::setup_tasks::machine_setup_state_dir()
        .unwrap()
        .join("tasks.json");
    crate::persistence::setup_tasks::save_to_for_test(&path, &store).unwrap();
    app.setup_flow_boot_synced = true;
    app.machine_setup_scan_current = true;
    app.machine_setup_tasks = crate::persistence::setup_tasks::load()
        .unwrap()
        .tasks
        .into_values()
        .collect();
    assert!(
        app.machine_setup_tasks
            .iter()
            .any(|task| { task.id == id && task.state == SetupTaskStateV1::ReadyToFix })
    );
    app.pump_setup_flow();
    assert_waiting(app, id);
    id
}

pub(super) fn assert_waiting(app: &KooladeApp, id: &str) {
    assert!(matches!(
        app.setup_flow_cache.as_ref().map(|flow| &flow.phase),
        Some(FlowPhaseV1::WaitingForUser { task_id, .. }) if task_id == id
    ));
}
