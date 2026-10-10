use super::*;

pub(in crate::app::root) fn fixture() -> KooladeApp {
    static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let chat_root = std::env::temp_dir().join(format!(
        "koolade-board-chat-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let chat_slug = chat_root.to_string_lossy().into_owned();
    let test_root = std::env::temp_dir().join(format!(
        "koolade-board-ui-fixture-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&test_root).unwrap();
    let root = test_root.join("workspace");
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Koolade Test"]);
    git(&["config", "user.email", "koolade@example.test"]);
    std::fs::write(root.join("README.md"), "isolated UI fixture\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-q", "-m", "UI fixture"]);
    let docs = ["First task", "Review task", "Merged task"]
        .iter()
        .enumerate()
        .map(|(i, title)| crate::artifacts::task_docs::TaskDocument {
            path: format!(
                ".koolade-packet/planning/tasks/fixture/{:03}-task.md",
                i + 1
            ),
            title: title.to_string(),
            text: format!("# {title}\n\nUnique story detail {i}"),
            identity: None,
            metadata: None,
            task_state: None,

            metadata_error: None,
        })
        .collect::<Vec<_>>();
    let mut states = std::collections::BTreeMap::new();
    for (i, pr_state) in [(1, "OPEN"), (2, "MERGED")] {
        let record = crate::core::implementation::Implementation {
            ticket: docs[i].path.clone(),
            task_uid: None,
            ticket_text: docs[i].text.clone(),
            approved_specification: None,
            approved_product_context: None,
            completed_dependency_context: None,
            branch: "koolade/fixture".into(),
            source_branch: None,
            destination_branch: None,
            source_ref: None,
            source_commit: None,
            repository_id: None,
            project_id: None,
            repository_identity: None,
            push_repository: None,
            repository_cache: None,
            task_repository_allocation_key: None,
            base: "main".into(),
            base_commit: "fixture".into(),
            task_repository: root.join("worktree"),
            task_repository_kind: crate::core::implementation::TaskRepositoryKind::LegacyWorktree,
            task_repository_ready: false,
            task_repositories: vec![root.join("worktree")],
            task_repository_commits: std::collections::BTreeMap::new(),
            status: if i == 1 {
                ImplementationStatus::AwaitingReview
            } else {
                ImplementationStatus::Completed
            },
            detail: String::new(),
            pr_url: Some(format!("https://github.com/fixture/repo/pull/{i}")),
            verified_head: Some("fixture".into()),
            auto_merge: false,
            merged_commit: None,
            pr_state: PullRequestState::parse_legacy(pr_state),
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        };
        states.insert(record.ticket.clone(), record);
    }
    KooladeApp {
        _test_temp_roots: vec![
            crate::app::root::TestTempRoot(test_root),
            crate::app::root::TestTempRoot(chat_root),
        ],
        screen: Screen::Connected(Box::new(Project {
            task_chats: Default::default(),
            activity: Default::default(),
            state: crate::core::state::PlannerState::load(&root).unwrap(),
            chat_slug,
            chat: Vec::new(),
            draft: String::new(),
            queue: Default::default(),
            queue_lock: None,
            active_implementations: Default::default(),
            implementation_states: states,
            pr_refresh: None,
            reconciliation: Default::default(),
            investigation: None,
            investigation_attempted: Default::default(),
            investigation_cooldown_until: None,
            last_pr_refresh: None,
            active_turn: None,
            task_turns: Default::default(),
            task_live: Default::default(),
            planning_work: Default::default(),
            active_planning_work: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: docs,
            archived_tasks: Default::default(),
            cancelled_work: Default::default(),
        })),
        ..Default::default()
    }
}

pub(in crate::app::root) fn frame(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    frame_at(app, ctx, events, egui::vec2(1800.0, 900.0))
}

pub(in crate::app::root) fn frame_at(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    size: egui::Vec2,
) -> egui::FullOutput {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            crate::ui::layout::paint(ui, app);
            app.render_queued_settings_dialog(ui);
        },
    );
    output.textures_delta.clear();
    output
}
pub(in crate::app::root) fn text_position(
    output: &egui::FullOutput,
    needle: &str,
) -> Option<egui::Pos2> {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == needle
            {
                return Some(text.pos + text.galley.mesh_bounds.center().to_vec2());
            }
            None
        })
        .or_else(|| super::mockup_layout::lane_position(output, needle))
}
pub(in crate::app::root) fn text_contains(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(
            &shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains(needle)
        )
    })
}

pub(in crate::app::root) fn click_text(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    label: &str,
) -> egui::FullOutput {
    click_text_at(app, ctx, label, egui::vec2(1800.0, 900.0))
}

pub(in crate::app::root) fn click_text_at(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    label: &str,
    size: egui::Vec2,
) -> egui::FullOutput {
    let output = frame_at(app, ctx, vec![], size);
    let click = text_position(&output, label)
        .or_else(|| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains(label) => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| {
            let visible = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            panic!("missing clickable {label}; visible: {visible:?}")
        });
    frame_at(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(click),
            egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        size,
    );
    frame_at(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        size,
    );
    frame_at(app, ctx, vec![], size)
}

pub(in crate::app::root) struct HangingTurnHarness;
impl crate::harness::AiHarness for HangingTurnHarness {
    fn label(&self) -> String {
        "hanging-fixture 0".into()
    }
    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok("present".into())
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        while !request.cancel.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(crate::AppError::Other(String::from(
            "fixture turn cancelled",
        )))
    }
}

/// All on-screen text, galley-concatenated without separators so a
/// phrase spanning a toast's soft line wrap still matches as a whole.
pub(in crate::app::root) fn canvas_text(output: &egui::FullOutput) -> String {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Paint ONE SETTLED frame with the app's toast layer included — the
/// test helper paints `layout::paint` directly, bypassing
/// `KooladeApp::ui`, so the same `toasts.show(..)` call is replayed
/// INSIDE the layout pass, in the same order the native app uses it.
/// A freshly introduced toast area needs one settle pass before its
/// content reaches the paint output, so one pass is discarded and the
/// settled second pass is returned.
pub(in crate::app::root) fn frame_toasting(
    app: &mut KooladeApp,
    ctx: &egui::Context,
) -> egui::FullOutput {
    let paint_one_pass = |app: &mut KooladeApp, ctx: &egui::Context| -> egui::FullOutput {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 900.0),
                )),
                events: vec![],
                ..Default::default()
            },
            |ui| {
                crate::ui::layout::paint(ui, app);
                app.toasts().show(ui.ctx());
            },
        );
        output.textures_delta.clear();
        output
    };
    let _ = paint_one_pass(app, ctx);
    paint_one_pass(app, ctx)
}

// Mirrors the production pattern deliberately: the UI owns the live turn
// handle locally, and the assertion needs exactly that Rc's identity.
pub(in crate::app::root) fn park_fixture_with_unapproved_feature(lapsed: bool) -> KooladeApp {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.queue.running = true;
        if lapsed {
            p.state
                .workflow
                .approved_features
                .insert("CHG-999".into(), "fixture stale contract".into());
        }
        for doc in &mut p.task_documents {
            doc.text.push_str("\nFeature ID: CHG-999\n");
        }
    }
    app
}
