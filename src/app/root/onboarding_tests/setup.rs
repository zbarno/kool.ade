use super::support::*;
use crate::app::root::{KooladeApp, Screen, setup_attention::ProbeOutput};
use crate::app::setup_attention::SetupScanResult;
use crate::persistence::setup_tasks::{SetupTaskEventV1, SetupTaskStateV1 as State, SetupTaskV1};
use std::path::Path;

fn task(id: &str, title: &str, state: State) -> SetupTaskV1 {
    SetupTaskV1 {
        schema_version: 1,
        id: id.into(),
        title: title.into(),
        explanation: "Synthetic machine finding for the Welcome flow.".into(),
        next_step: "Review the setup guide, then check again.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 10,
        observed_at_unix_ms: 20,
        scan_generation: 3,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: vec![SetupTaskEventV1 {
            observed_at_unix_ms: 20,
            scan_generation: 3,
            state,
        }],
        legacy_source_id: None,
        superseded_by: Vec::new(),
    }
}

fn position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == needle => {
            Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
        }
        _ => None,
    })
}

fn contains(output: &egui::FullOutput, needle: &str) -> bool {
    visible_text(output)
        .iter()
        .any(|text| text.contains(needle))
}

/// Exercises the pre-workspace boundary from Welcome card selection through
/// the shared setup details modal. A helper-only test cannot prove that the
/// root screen routes and retains the selection. Uses synthetic tasks only;
/// it needs no CLI, Docker, workspace clone, or network and has low setup cost.
#[test]
fn welcome_setup_board_opens_guidance_before_any_workspace_is_connected() {
    let home = std::env::temp_dir().join(format!(
        "koolade-setup-welcome-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(Path::new(&home));
    let mut app = KooladeApp {
        machine_setup_tasks: vec![
            task(
                "setup:docker:rootless",
                "Configure secure Docker",
                State::ReadyToFix,
            ),
            task(
                "setup:cli:install",
                "Choose a coding agent",
                State::Detected,
            ),
        ],
        ..KooladeApp::default()
    };
    let mut legacy_docker_notice =
        crate::app::setup_attention::SetupIssue::docker("Docker needs attention on this machine.");
    legacy_docker_notice.related_task_prefix = Some("setup:docker:".into());
    app.setup_attention.push(legacy_docker_notice);
    assert!(
        app.setup_probe.is_none(),
        "test fixture must not probe the host"
    );
    let ctx = egui::Context::default();

    let board = frame(&mut app, &ctx, Vec::new());
    assert!(contains(&board, "Allow coding tasks to run safely"));
    assert!(contains(&board, "Setup progress · 0 of 2 checks verified"));
    assert!(!contains(&board, "Kool.ad/e needs Docker"));
    let output = click(&mut app, &ctx, "Guide me");

    assert!(matches!(&app.screen, Screen::Welcome));
    assert!(
        contains(&output, "Setup guide"),
        "visible: {:?}",
        visible_text(&output)
    );
    assert!(contains(&output, "Setup status"));
    assert!(contains(&output, "Setup history"));
    assert!(contains(&output, "Setup conversation"));
    assert!(contains(&output, "Docker rootless Engine guide"));
    let guide = position(&output, "Setup guide").unwrap();
    let status = position(&output, "Setup status").unwrap();
    assert!(
        guide.x < status.x,
        "wide setup details should show help on the left"
    );
    assert!(
        !visible_text(&output)
            .iter()
            .any(|text| text.contains("sudo ") || text.contains("docker run")),
        "the default setup guide must not require terminal commands"
    );

    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let output = frame(&mut app, &ctx, Vec::new());
    assert!(!contains(&output, "Setup status"));
    assert!(
        ctx.data_mut(
            |data| data.get_temp::<String>(egui::Id::new("koolade_welcome_setup_selection"))
        )
        .is_none()
    );
    app.machine_setup_tasks = vec![task(
        "setup:cli:install",
        "Choose a coding agent",
        State::Detected,
    )];
    let output = click(&mut app, &ctx, "Guide me");
    assert!(contains(&output, "Setup conversation"));
    assert!(contains(&output, "You can explore and plan with Kool.ad/e"));
    assert!(contains(&output, "Open Coding Tools"));
    drop(app);
    let _ = std::fs::remove_dir_all(home);
}

/// Holds a fake probe open while rendering Welcome to prove setup discovery
/// stays off the egui frame path. It uses only an in-process channel (no CLI,
/// Docker, workspace, or network) and has negligible setup cost.
#[test]
fn a_slow_setup_probe_keeps_the_welcome_board_renderable() {
    let state_home = tempfile::tempdir().unwrap();
    let _state_home = crate::test_support::KooladeHomeGuard::set(state_home.path());
    let mut app = KooladeApp {
        machine_setup_tasks: vec![task(
            "setup:docker:start",
            "Start the container service",
            State::Detected,
        )],
        ..KooladeApp::default()
    };
    let (release, waiting) = std::sync::mpsc::channel::<()>();
    app.setup_probe = Some(std::thread::spawn(move || {
        waiting.recv().unwrap();
        ProbeOutput::Scanned(SetupScanResult {
            generation: 0,
            issue: None,
            snapshot: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
            docker_readiness_fingerprint: None,
            setup_provider: None,
            selected_provider: None,
            cancelled: true,
        })
    }));
    let ctx = egui::Context::default();

    let size = egui::vec2(900.0, 900.0);
    let mut output = frame_at(&mut app, &ctx, Vec::new(), size);
    for _ in 0..5 {
        if contains(&output, "Checking this machine") {
            break;
        }
        frame_at(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(450.0, 450.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -600.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
            size,
        );
        output = frame_at(&mut app, &ctx, Vec::new(), size);
    }

    release.send(()).unwrap();
    app.setup_probe.take().unwrap().join().unwrap();
    assert!(contains(&output, "Checking this machine"));
    assert!(contains(&output, "Start the safe coding environment"));
}

/// Covers the stale-selection boundary when a new readiness snapshot removes a
/// task while its details are open. A selection-helper test cannot prove the
/// Welcome screen closes the modal and clears its retained task key. Synthetic
/// state only; no CLI, Docker, workspace, network, or expensive setup is used.
#[test]
fn welcome_closes_setup_details_when_a_rescan_removes_the_selected_task() {
    let home = std::env::temp_dir().join(format!(
        "koolade-setup-stale-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(Path::new(&home));
    let mut app = KooladeApp {
        machine_setup_tasks: vec![task(
            "setup:docker:start",
            "Start the container service",
            State::Detected,
        )],
        ..KooladeApp::default()
    };
    let ctx = egui::Context::default();

    let _ = frame(&mut app, &ctx, Vec::new());
    let details = click(&mut app, &ctx, "Guide me");
    assert!(
        contains(&details, "Setup status"),
        "visible: {:?}",
        visible_text(&details)
    );
    app.machine_setup_tasks.clear();

    let updated = frame(&mut app, &ctx, Vec::new());
    assert!(!contains(&updated, "Setup status"));
    assert!(
        ctx.data_mut(
            |data| data.get_temp::<String>(egui::Id::new("koolade_welcome_setup_selection"))
        )
        .is_none()
    );
    drop(app);
    let _ = std::fs::remove_dir_all(home);
}
