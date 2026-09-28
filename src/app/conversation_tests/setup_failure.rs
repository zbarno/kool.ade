use super::*;
use crate::app::root::board_tests::text_contains;

fn finish_setup_probe(app: &mut PacketApp) {
    let ctx = egui::Context::default();
    for _ in 0..500 {
        if app
            .setup_probe
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
        {
            app.poll_setup_attention(&ctx);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("provider setup probe did not finish within five seconds");
}

#[test]
fn failed_project_planning_preserves_setup_cause_and_retry_on_its_board_card() {
    let root = std::env::temp_dir().join(format!(
        "packet-planning-setup-failure-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
    }
    app.task_harness = Some(Box::new(StoppedHarness {
        wait_for_cancel: false,
    }));
    app.start_turn("Investigate this project safely");
    complete(&mut app);

    let Screen::Connected(project) = &app.screen else {
        panic!("project should remain connected");
    };
    let work = project.planning_work.last().unwrap();
    assert_eq!(
        work.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(work.detail.contains("provider unavailable"));
    assert!(work.detail.contains("Next action:"));
    let persisted = crate::core::planning_work::load(&root).unwrap();
    assert_eq!(persisted.last().unwrap().detail, work.detail);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_contains(&output, "provider unavailable"));
    assert!(text_contains(&output, "Needs attention"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_issue_is_a_board_attention_card_with_a_working_recheck_action() {
    let mut app = fixture();
    let issue = crate::app::setup_attention::SetupIssue::provider(
        "Selected provider uses an unsupported API protocol.",
    );
    app.setup_attention = Some(issue.clone());
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for text in [
        issue.title,
        "Why this matters",
        issue.recommendation,
        issue.impact,
        issue.next_action,
        "Retry setup check",
    ] {
        assert!(
            text_contains(&output, text),
            "missing setup guidance: {text}"
        );
    }
    click_text(&mut app, &ctx, "Retry setup check");
    finish_setup_probe(&mut app);
    assert_eq!(
        app.setup_attention,
        crate::app::setup_attention::detect(),
        "retry must refresh current prerequisite and provider state"
    );
}

#[test]
fn setup_attention_card_opens_workspace_settings() {
    let mut app = fixture();
    app.setup_attention = Some(crate::app::setup_attention::SetupIssue::provider(
        "Selected provider uses an unsupported API protocol.",
    ));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_contains(&output, "Open settings"));
    click_text(&mut app, &ctx, "Open settings");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_contains(&output, "Workspace settings"));
    assert!(text_contains(&output, "Automation policy"));
}

#[test]
fn project_connection_computes_the_local_setup_attention_state() {
    let root = std::env::temp_dir().join(format!(
        "packet-setup-connect-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Packet Setup Test"],
        vec!["config", "user.email", "packet-setup@example.invalid"],
    ] {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let mut app = fixture();
    app.conn_path = root.to_string_lossy().into_owned();
    app.submit_connect();
    finish_setup_probe(&mut app);
    assert_eq!(app.setup_attention, crate::app::setup_attention::detect());
    std::fs::remove_dir_all(root).unwrap();
}
