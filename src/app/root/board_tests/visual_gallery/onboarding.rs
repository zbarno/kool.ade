use super::*;
use crate::app::welcome::onboarding::Step;

#[test]
#[ignore = "GPU onboarding review; requires isolated KOOLADE_HOME and KOOLADE_UI_REVIEW_DIR"]
fn render_onboarding_review_gallery() {
    // Production painters, pointer events and GPU output, with synthetic state.
    // Only read-only launcher workers run; no provider/network/OS repair. A state-only test cannot
    // prove visible layout; this does not replace a native novice walkthrough.
    let dir = std::path::PathBuf::from(std::env::var("KOOLADE_UI_REVIEW_DIR").unwrap());
    assert!(std::env::var_os("KOOLADE_HOME").is_some());
    std::fs::create_dir_all(&dir).unwrap();
    for size in [[1280, 800], [360, 640]] {
        let mut gallery = Gallery::new(size, dir.clone());
        gallery.app = KooladeApp::default();
        gallery.app.onboarding.step = Step::Start;
        gallery.save("welcome");
        gallery.click("Get started");
        assert_eq!(gallery.app.onboarding.step, Step::Manager);
        gallery.save("manager");
        gallery.click("Need help? Ask Man.ager");
        gallery.save("manager-help");
        // Close help through state here because wide/narrow use different
        // affordances; the remaining click journey must stay unobscured.
        gallery.app.onboarding.help_open = false;
        gallery.click("Continue setup");
        gallery.save("readiness");
        gallery.click("Continue to a code repository");
        gallery.save("projects");
        gallery.save("code-repository-entry");
    }
}
