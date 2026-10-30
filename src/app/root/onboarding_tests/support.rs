use super::super::super::{KooladeApp, Screen};
use std::{path::Path, process::Command};

pub(super) struct StateHome {
    _environment: crate::test_support::ProcessEnvironmentGuard,
}

impl StateHome {
    pub(super) fn new() -> Self {
        Self {
            _environment: crate::test_support::ProcessEnvironmentGuard::new(),
        }
    }

    pub(super) fn set(&mut self, path: &Path) {
        self._environment.set("KOOLADE_HOME", path);
    }
}

pub(super) fn frame(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    frame_at(app, ctx, events, egui::vec2(1800.0, 900.0))
}

pub(super) fn frame_at(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    size: egui::Vec2,
) -> egui::FullOutput {
    // These existing journey tests explicitly exercise the Advanced routes.
    app.onboarding.step = crate::app::welcome::onboarding::Step::Advanced;
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
            app.paint_screen(ui);
            app.render_queued_settings_dialog(ui);
        },
    );
    output.textures_delta.clear();
    output
}

fn text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == needle => {
                Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
            }
            _ => None,
        })
        .or_else(|| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains(needle) => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
        })
}

/// Locate `needle`'s visible position, scrolling toward it until the point
/// falls inside the stable viewport band. Shared by `click` and `type_into`
/// so deep layout content reachable only after scrolling stays interactable.
fn resolve_position(app: &mut KooladeApp, ctx: &egui::Context, needle: &str) -> egui::Pos2 {
    let mut output = frame(app, ctx, Vec::new());
    let mut position = text_position(&output, needle);
    for _ in 0..8 {
        let on_screen = position.is_some_and(|point| point.y >= 32.0 && point.y <= 868.0);
        if on_screen {
            break;
        }
        let delta = if position.is_some_and(|point| point.y < 32.0) {
            egui::vec2(0.0, 500.0)
        } else {
            egui::vec2(0.0, -500.0)
        };
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(900.0, 450.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
        );
        output = frame(app, ctx, Vec::new());
        position = text_position(&output, needle);
    }
    position
        .filter(|point| point.y >= 32.0 && point.y <= 868.0)
        .unwrap_or_else(|| {
            let visible = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            panic!("missing UI text {needle}; visible: {visible:?}")
        })
}

pub(super) fn click(app: &mut KooladeApp, ctx: &egui::Context, label: &str) -> egui::FullOutput {
    let position = resolve_position(app, ctx, label);
    frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    frame(app, ctx, Vec::new())
}

pub(super) fn type_into(app: &mut KooladeApp, ctx: &egui::Context, placeholder: &str, value: &str) {
    let position = resolve_position(app, ctx, placeholder);
    frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    frame(app, ctx, vec![egui::Event::Text(value.to_owned())]);
}

pub(super) fn visible_text(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

pub(super) fn git(cwd: Option<&Path>, args: &[&str]) {
    let mut command = Command::new("git");
    command
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn clone_bare_remote(source: &Path, path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    git(
        None,
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            "--",
            source.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    );
}

pub(super) fn wait_for_cancelled_turn(app: &mut KooladeApp) {
    let Screen::Connected(project) = &mut app.screen else {
        panic!("workspace should remain open")
    };
    let turn = project.active_turn.take().unwrap();
    turn.request_cancel();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if matches!(
            turn.poll(std::time::Duration::from_millis(20)),
            Some(crate::core::turn::TurnEvt::Done(_))
        ) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture turn timed out"
        );
    }
}
