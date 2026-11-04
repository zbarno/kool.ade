use super::super::support::{frame, visible_text};
use crate::app::{root::KooladeApp, welcome::onboarding::Step};

/// Production welcome screen to visible repository actions. Rendering the
/// real egui surface catches legacy paired-workspace prompts that a model
/// assertion cannot; no Git, network, Docker, or SSH setup is needed.
#[test]
fn welcome_offers_code_checkout_open_and_clone_without_planning_pair_actions() {
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::Advanced;
    let output = frame(&mut app, &egui::Context::default(), Vec::new());
    let text = visible_text(&output).join("\n");

    assert!(text.contains("Clone a code repository"), "{text}");
    assert!(text.contains("Open a local code repository"), "{text}");
    assert!(!text.contains("planning repository"), "{text}");
    assert!(!text.contains("managed workspace"), "{text}");
    assert!(!text.contains("blank project"), "{text}");
}
