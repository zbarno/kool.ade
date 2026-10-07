use super::*;

#[test]
fn new_task_can_override_and_persist_source_and_destination_branches() {
    let root = std::env::temp_dir().join(format!(
        "koolade-task-branches-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["config", "user.name", "Kool.ad/e Branch Test"],
        vec!["config", "user.email", "koolade-branch@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(root.join("README.md"), "branch fixture\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-qm", "fixture"]] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    for branch in ["release/2.1", "integration"] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["branch", branch])
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        project.refresh_git();
    }
    app.task_harness = Some(Box::new(StoppedHarness {
        wait_for_cancel: true,
    }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "+ New Task");
    click_branch_picker(&mut app, &ctx, "Source Branch");
    click_text(&mut app, &ctx, "release/2.1");
    click_branch_picker(&mut app, &ctx, "Destination Branch");
    click_text(&mut app, &ctx, "integration");
    click_text(&mut app, &ctx, "Describe what you want to do…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Ship the feature branch".into())],
    );
    click_text(&mut app, &ctx, "Create Task");
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    let created = project.planning_work.last().unwrap();
    assert_eq!(created.source_branch.as_deref(), Some("release/2.1"));
    assert_eq!(created.destination_branch.as_deref(), Some("integration"));
    let saved = crate::core::planning_work::load(&root).unwrap();
    assert_eq!(
        saved.last().unwrap().source_branch.as_deref(),
        Some("release/2.1")
    );
    assert_eq!(
        saved.last().unwrap().destination_branch.as_deref(),
        Some("integration")
    );
    if let Some(turn) = &project.active_turn {
        turn.request_cancel();
    }
    complete(&mut app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn origin_backed_destination_selection_rejects_local_only_branches() {
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.git.branches = vec!["main".into(), "local-only".into()];
        project.git.remote_branches = vec!["main".into()];
        project.git.has_origin = true;
    }
    assert_eq!(
        crate::ui::Surface::repository_destination_branches(&app),
        vec!["main"]
    );
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CreatePlanningTask {
        kind: crate::core::planning_work::WorkKind::Feature,
        description: "Use a local-only destination".into(),
        parent_uid: None,
        source_branch: Some("main".into()),
        destination_branch: Some("local-only".into()),
        routing_overrides: Default::default(),
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    assert!(project.planning_work.is_empty());
}

// Branch labels and values are separate so long names can truncate without
// hiding the field's purpose. Target the value directly below its label.
fn click_branch_picker(app: &mut KooladeApp, ctx: &egui::Context, label: &str) {
    let output = frame(app, ctx, vec![]);
    let label = text_position(&output, label).expect("branch field label");
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "main" => {
                let pos = text.pos + text.galley.mesh_bounds.center().to_vec2();
                (pos.y > label.y && pos.y < label.y + 50.0).then_some(pos)
            }
            _ => None,
        })
        .expect("branch selector below its label");
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}
