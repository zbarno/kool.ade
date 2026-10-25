use super::*;

#[test]
fn superseded_setup_history_is_hidden_and_setup_cards_are_counted() {
    use crate::persistence::setup_tasks::{SetupTaskStateV1 as State, SetupTaskV1};

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

    assert!(text_position(&output, "Configure rootless Docker").is_some());
    assert!(text_position(&output, "Kool.ad/e needs Docker").is_some());
    assert!(text_position(&output, "Legacy Docker setup").is_none());
    assert!(text_position(&output, "Board  5").is_some());

    fn machine_setup_task(id: &str, title: &str, state: State) -> SetupTaskV1 {
        use crate::persistence::setup_tasks::SetupTaskEventV1;

        SetupTaskV1 {
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
}
