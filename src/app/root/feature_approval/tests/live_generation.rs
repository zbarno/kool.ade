use super::*;

struct LivePi;
impl crate::harness::AiHarness for LivePi {
    fn label(&self) -> String {
        crate::harness::PiHarness.label()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        crate::harness::PiHarness.check_available()
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        let mut request = request.clone();
        request.reasoning_level = "off".into();
        crate::harness::PiHarness.execute(&request)
    }
}

#[test]
fn temporary_live_fixture_prepare_and_generate() {
    let Ok(root) = std::env::var("PACKET_APPROVAL_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let id = std::env::var("PACKET_APPROVAL_FEATURE").unwrap();
    let mut app = super::super::super::board_tests::fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        project.task_documents.clear();
    }
    app.task_harness = Some(Box::new(LivePi));
    app.approve_and_prepare_feature(&id);
    let ctx = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12 * 60 * 60);
    while app.conversation_busy() {
        app.tick(0.016, &ctx);
        assert!(
            std::time::Instant::now() < deadline,
            "live feature preparation/generation timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(matches!(&app.screen, Screen::Connected(_)));
    // TaskGeneration commits through its worker while Packet's visible
    // project snapshot may still be stale; reload the persisted authority.
    let state = crate::core::state::PlannerState::load(&root).unwrap();
    assert!(crate::core::workflow::feature_approved(
        &root,
        &state.workflow,
        &id
    ));
    assert!(
        state
            .workflow
            .task_batches
            .iter()
            .any(|batch| batch.feature.contains(&id))
    );
}
