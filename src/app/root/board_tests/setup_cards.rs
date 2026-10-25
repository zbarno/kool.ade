use super::*;

fn machine_setup_task(
    id: &str,
    title: &str,
    state: crate::persistence::setup_tasks::SetupTaskStateV1,
) -> crate::persistence::setup_tasks::SetupTaskV1 {
    use crate::persistence::setup_tasks::SetupTaskEventV1;

    crate::persistence::setup_tasks::SetupTaskV1 {
        schema_version: 1,
        id: id.into(),
        title: title.into(),
        explanation: "Synthetic setup observation.".into(),
        next_step: "Review this setup task.".into(),
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
        legacy_source_id: None,
        superseded_by: Vec::new(),
    }
}

#[test]
fn superseded_setup_history_is_hidden_and_setup_cards_are_counted() {
    use crate::persistence::setup_tasks::SetupTaskStateV1 as State;

    let mut app = fixture();
    app.machine_setup_tasks = vec![
        machine_setup_task(
            "setup:docker:rootless",
            "Configure rootless Docker",
            State::ReadyToFix,
        ),
        machine_setup_task(
            "setup:legacy:docker",
            "Legacy Docker setup",
            State::Superseded,
        ),
    ];
    app.setup_attention
        .push(crate::app::setup_attention::SetupIssue::docker(
            "Docker Engine is unavailable",
        ));

    let ctx = super::mockup_layout::styled_context();
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let output = frame(&mut app, &ctx, vec![]);

    assert!(text_position(&output, "Allow coding tasks to run safely").is_some());
    assert!(text_position(&output, "Kool.ad/e needs Docker").is_some());
    assert!(text_position(&output, "Legacy Docker setup").is_none());
    assert!(text_position(&output, "Board  5").is_some());
}

/// Covers the connected Kanban card-to-shared-Task-Details route. Testing the
/// card renderer alone cannot prove the board stores the selection and opens
/// the same details shell. It uses the synthetic Git/UI fixture only; no CLI,
/// Docker, or network is required and setup cost is low.
#[test]
fn kanban_setup_card_opens_guided_details_in_the_shared_task_shell() {
    use crate::persistence::setup_tasks::{
        SetupTaskEventV1, SetupTaskStateV1 as State, SetupTaskV1,
    };

    let mut app = fixture();
    let state = State::ReadyToFix;
    app.machine_setup_tasks.push(SetupTaskV1 {
        schema_version: 1,
        id: "setup:docker:rootless".into(),
        title: "Configure secure Docker".into(),
        explanation: "Docker needs a safer user-level configuration.".into(),
        next_step: "Set up rootless Docker, then check again.".into(),
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
        legacy_source_id: None,
        superseded_by: Vec::new(),
    });
    let ctx = super::mockup_layout::styled_context();
    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }

    let output = click_text(&mut app, &ctx, "Guide me");

    assert!(text_contains(&output, "Setup guide"));
    assert!(text_contains(&output, "Setup status"));
    assert!(text_contains(&output, "Setup history"));
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("setup:docker:rootless")
    );
}

/// Exercises the connected-board details boundary for a task blocked by two
/// prerequisites. A catalog-only assertion cannot prove both prerequisites
/// are rendered as readable names in the shared modal. Uses synthetic tasks;
/// no CLI, Docker, network, or external fixture is required.
#[test]
fn kanban_setup_details_name_each_blocking_prerequisite() {
    use crate::persistence::setup_tasks::SetupTaskStateV1 as State;

    let mut app = fixture();
    let mut image = machine_setup_task(
        "setup:images:general:amd64",
        "Prepare the approved runtime image",
        State::BlockedByPrerequisite,
    );
    image.blocked_by = vec![
        "setup:docker:rootless".into(),
        "setup:docker:capabilities".into(),
    ];
    app.machine_setup_tasks = vec![
        image,
        machine_setup_task(
            "setup:docker:rootless",
            "Configure secure Docker",
            State::ReadyToFix,
        ),
        machine_setup_task(
            "setup:docker:capabilities",
            "Resolve Docker compatibility",
            State::ReadyToFix,
        ),
    ];
    let ctx = super::mockup_layout::styled_context();
    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }

    let output = click_text(&mut app, &ctx, "Prepare the safe coding environment");

    assert!(text_contains(&output, "Waiting for"));
    assert!(text_contains(&output, "Allow coding tasks to run safely"));
    assert!(text_contains(&output, "Check coding task requirements"));
    assert!(!text_contains(&output, "setup:docker:rootless"));
}
