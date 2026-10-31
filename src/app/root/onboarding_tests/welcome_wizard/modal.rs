use super::super::support::{StateHome, click, frame, visible_text};
use crate::app::root::{KooladeApp, Screen, WelcomeEntry};

#[test]
fn escape_dismisses_the_open_clone_wizard() {
    let (base, _state_home, mut app, ctx) = setup("escape", WelcomeEntry::CloneRepository);
    let _ = frame(&mut app, &ctx, Vec::new());
    let output = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&output)
            .iter()
            .any(|line| line.contains("Step 1 of 2"))
    );
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(egui::pos2(900.0, 450.0)),
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            },
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            },
        ],
    );
    assert!(app.wiz.is_none());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn backdrop_click_dismisses_the_wizard_without_submitting_the_canvas() {
    let (base, _state_home, mut app, ctx) = setup("backdrop", WelcomeEntry::CloneRepository);
    let _ = frame(&mut app, &ctx, Vec::new());
    let output = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&output)
            .iter()
            .any(|line| line.contains("Step 1 of 2"))
    );
    let position = egui::pos2(180.0, 880.0);
    frame(
        &mut app,
        &ctx,
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
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(app.wiz.is_none());
    assert!(matches!(app.screen, Screen::Welcome));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn modal_veil_does_not_switch_to_the_blank_project_journey() {
    let (base, _state_home, mut app, ctx) = setup("veil", WelcomeEntry::CloneRepository);
    let _ = frame(&mut app, &ctx, Vec::new());
    let _ = frame(&mut app, &ctx, Vec::new());
    click(&mut app, &ctx, "Create a brand-new project");
    let output = frame(&mut app, &ctx, Vec::new());
    assert!(matches!(app.screen, Screen::Welcome));
    assert!(app.wiz.is_some());
    assert!(
        visible_text(&output)
            .iter()
            .any(|line| line.contains("Step 1 of 2")),
        "backdrop must leave the clone wizard active"
    );
    let _ = std::fs::remove_dir_all(base);
}

fn setup(
    suffix: &str,
    entry: WelcomeEntry,
) -> (std::path::PathBuf, StateHome, KooladeApp, egui::Context) {
    let base = std::env::temp_dir().join(format!(
        "koolade-wizard-modal-{suffix}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let mut app = KooladeApp::default();
    app.wizard_open(entry);
    (base, state_home, app, egui::Context::default())
}
