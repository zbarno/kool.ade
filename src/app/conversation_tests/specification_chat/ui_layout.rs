use super::*;

/// Production UI frame boundary: the real workspace layout must render the
/// Specification conversation and composer in compact and desktop viewports.
/// A widget unit test cannot validate the combined panels. Uses
/// headless egui frames only; no desktop server or external service (low cost).
#[test]
fn specification_chat_composer_renders_in_compact_and_desktop_layouts() {
    for (width, height) in [(640.0, 820.0), (1440.0, 900.0)] {
        let mut app = fixture();
        let ctx = egui::Context::default();
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_document_tab"), false);
            data.insert_temp(egui::Id::new("koolade_spec_chat_collapsed"), false);
        });
        let output =
            super::super::board_tests::frame_at(&mut app, &ctx, vec![], egui::vec2(width, height));
        assert!(
            text_position(&output, "Specification conversation").is_some(),
            "conversation heading is missing at viewport width {width}"
        );
        assert!(
            text_position(&output, "What should change in this specification?").is_some(),
            "Specification composer is missing at viewport width {width}"
        );
    }
}
