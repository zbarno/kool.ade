use super::*;
use crate::app::root::board_tests::text_contains;

fn finish_setup_probe(app: &mut KooladeApp) {
    let ctx = egui::Context::default();
    // Bootstrap detection probes six CLIs in two serial batches. In test builds,
    // Codex checks can take about 55 seconds before the other batch is started.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        if app
            .setup_probe
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
        {
            app.poll_setup_attention(&ctx);
            if app.setup_probe.is_none() && app.setup_refresh_pending.is_none() {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("provider setup probe did not finish within 120 seconds");
}

struct SetupTaskStoreFixture {
    root: std::path::PathBuf,
    state_path: std::path::PathBuf,
}

impl SetupTaskStoreFixture {
    fn attach(app: &mut KooladeApp) -> Self {
        let root =
            std::env::temp_dir().join(format!("koolade-setup-task-store-{}", uuid::Uuid::new_v4()));
        let state_path = root.join("machine-setup").join("tasks.json");
        app.set_setup_task_state_path(state_path.clone());
        Self { root, state_path }
    }
}

impl Drop for SetupTaskStoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn persisted_machine_setup_tasks(
    path: &std::path::Path,
) -> Vec<crate::persistence::setup_tasks::SetupTaskV1> {
    let persisted: crate::persistence::setup_tasks::SetupTaskStoreV1 =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    crate::persistence::setup_tasks::ordered_task_ids(&persisted)
        .unwrap()
        .iter()
        .filter_map(|id| persisted.tasks.get(id).cloned())
        .collect()
}

#[test]
fn failed_project_planning_preserves_setup_cause_and_retry_on_its_board_card() {
    let root = std::env::temp_dir().join(format!(
        "koolade-planning-setup-failure-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
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
    let work_title = work.title.clone();
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
    click_text(&mut app, &ctx, &work_title);
    let output = click_text(&mut app, &ctx, "Why am I seeing this?");
    assert!(text_contains(&output, "provider unavailable"));
    assert!(text_contains(&output, "Needs attention"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_issue_is_a_board_attention_card_with_a_working_recheck_action() {
    let state_home = tempfile::tempdir().unwrap();
    let _state_home = crate::test_support::KooladeHomeGuard::set(state_home.path());
    let mut app = fixture();
    let setup_store = SetupTaskStoreFixture::attach(&mut app);
    let issue = crate::app::setup_attention::SetupIssue::provider(
        "Selected provider uses an unsupported API protocol.",
    );
    app.setup_attention.push(issue.clone());
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, issue.title);
    for text in [
        issue.title,
        "Why this matters",
        issue.recommendation,
        issue.impact,
        issue.next_action,
        "Check again",
    ] {
        assert!(
            text_contains(&output, text),
            "missing setup guidance: {text}"
        );
    }
    click_text(&mut app, &ctx, "Check again");
    assert!(app.setup_probe.is_some(), "retry must start a fresh scan");
    finish_setup_probe(&mut app);
    assert!(
        app.setup_attention
            .iter()
            .all(|current| current.issue != issue.issue),
        "the recheck must remove the synthetic issue that opened the card"
    );
    assert_eq!(
        app.machine_setup_tasks,
        persisted_machine_setup_tasks(&setup_store.state_path),
        "retry must refresh current persisted prerequisite state"
    );
    assert!(setup_store.state_path.is_file());
}

#[test]
fn setup_attention_card_opens_workspace_settings() {
    let mut app = fixture();
    app.setup_attention
        .push(crate::app::setup_attention::SetupIssue::provider(
            "Selected provider uses an unsupported API protocol.",
        ));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    let title = app.setup_attention.first().unwrap().title;
    click_text(&mut app, &ctx, title);
    click_text(&mut app, &ctx, "Open workspace settings");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_contains(&output, "Workspace settings"));
    click_text(&mut app, &ctx, "Automation");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_contains(&output, "Automation"));
}

#[test]
fn project_connection_computes_the_local_setup_attention_state() {
    let state_home = tempfile::tempdir().unwrap();
    let _state_home = crate::test_support::KooladeHomeGuard::set(state_home.path());
    let root = std::env::temp_dir().join(format!(
        "koolade-setup-connect-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Kool.ad/e Setup Test"],
        vec!["config", "user.email", "koolade-setup@example.invalid"],
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
    let setup_store = SetupTaskStoreFixture::attach(&mut app);
    app.conn_path = root.to_string_lossy().into_owned();
    app.submit_connect();
    assert!(
        app.setup_probe.is_some(),
        "project connection must start a scan"
    );
    finish_setup_probe(&mut app);
    assert_eq!(
        app.machine_setup_tasks,
        persisted_machine_setup_tasks(&setup_store.state_path),
        "project connection must project its own persisted prerequisite state"
    );
    assert!(setup_store.state_path.is_file());
    std::fs::remove_dir_all(root).unwrap();
}
