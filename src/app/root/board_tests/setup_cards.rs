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
        superseded_by: Vec::new(),
    });
    let ctx = super::mockup_layout::styled_context();
    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }

    // The member row stands in for the former per-card "Guide me" action:
    // selecting the row surfaces the same shared task-details shell.
    let output = click_text(&mut app, &ctx, "Allow coding tasks to run safely");

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
        "setup:images:general:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
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

    let output = click_text(&mut app, &ctx, "Prepare the general runtime image");

    assert!(text_contains(&output, "Waiting for"));
    assert!(text_contains(&output, "Allow coding tasks to run safely"));
    assert!(text_contains(&output, "Check coding task requirements"));
    assert!(!text_contains(&output, "setup:docker:rootless"));
}

/// Presentation boundary of the grouped "Prepare this computer" checklist:
/// whichever actor carries the next step must be legible from the row status,
/// and no step may defer agent work to the operator via stale action copy.
/// Rendering (vs. a state classifier) is required to prove the visible copy.
/// Synthetic local Git/UI fixtures only; no Docker/provider/network, low cost.
#[test]
fn setup_cards_assign_execution_and_verification_to_the_agent() {
    use crate::persistence::setup_tasks::SetupTaskStateV1 as State;
    for (state, _carried_by_agent) in [
        (State::InProgress, true),
        (State::AwaitingVerification, true),
        (State::NeedsApproval, false),
        (State::NeedsVerification, false),
        (State::BlockedByPrerequisite, false),
    ] {
        let label = match state {
            State::InProgress => "In progress",
            State::AwaitingVerification => "Awaiting verification",
            State::NeedsApproval => "Needs approval",
            State::NeedsVerification => "Needs verification",
            _ => "Blocked by prerequisite",
        };
        let mut app = fixture();
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.planning_work.clear();
        project.task_documents.clear();
        project.state.items.clear();
        project.state.resolved_items.clear();
        project.state.active_features.clear();
        app.setup_attention.clear();
        app.machine_setup_tasks = vec![machine_setup_task(
            "setup:docker:rootless",
            "Prepare secure execution",
            state,
        )];
        let ctx = super::mockup_layout::styled_context();
        for _ in 0..4 {
            frame(&mut app, &ctx, vec![]);
        }
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            text_position(&output, "Prepare this computer").is_some(),
            "the parent checklist hosts every step; state {state:?}"
        );
        assert!(
            text_position(&output, label).is_some(),
            "the checklist row carries the operational status; state {state:?}"
        );
        // Whoever carries the next action is legible at a glance: no
        // step defers agent work to the operator, and only genuinely
        // actionable rows advertise a fix.
        assert!(
            !text_position(&output, "Guide me").is_some(),
            "no legacy per-card delegation; state {state:?}"
        );
        assert!(
            !text_position(&output, "Fix").is_some(),
            "no step in these states offers a one-click fix; state {state:?}"
        );
    }
}
