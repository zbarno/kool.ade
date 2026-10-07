//! Live-rendered specification view (SPECIFICATION.md §6, §21–§22).
//!
//! READ-ONLY: mutations travel exclusively through planning turns.
//! Implementation notes:
//!   * Markdown conversion is delegated to the shared pulldown-cmark→egui
//!     fold in [`crate::ui::markdown`], themed via
//!     [`crate::ui::markdown::PAPER`] (white paper, dark ink)
//!   * the reading surface, line length and bounds are owned here

const NO_CONTENT_HINT: &str = "Nothing has been planned yet — describe the product in the chat and the living specification grows here.";

/// Render the current specification markdown (or a friendly placeholder).
pub fn render(ui: &mut egui::Ui, spec: Option<&str>) {
    // Constrain the child rectangle, not just its preferred width. Centered
    // parent layouts otherwise allow long documents to grow beyond the panel.
    let width = ui.available_width().min(800.0);
    let inset = ((ui.available_width() - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(inset);
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                egui::Frame::NONE
                    .fill(crate::ui::theme::PANEL_ALT)
                    .inner_margin(28)
                    .show(ui, |ui| {
                        ui.set_width((width - 58.0).max(40.0));
                        ui.set_min_height(200.0);
                        ui.visuals_mut().override_text_color = Some(crate::ui::theme::TEXT);
                        ui.visuals_mut().widgets.noninteractive.fg_stroke.color =
                            crate::ui::theme::TEXT_DIM;
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        render_markdown(ui, spec);
                    });
            },
        );
    });
}

fn render_markdown(ui: &mut egui::Ui, spec: Option<&str>) {
    let Some(md) = spec.filter(|s| !s.trim().is_empty()) else {
        ui.vertical_centered_justified(|ui| {
            ui.add_space(60.0);
            ui.label(
                egui::RichText::new(NO_CONTENT_HINT)
                    .weak()
                    .size(13.0)
                    .color(crate::ui::markdown::PAPER.muted),
            );
            ui.add_space(30.0);
        });
        return;
    };
    crate::ui::markdown::paint(ui, md, crate::ui::markdown::CHAT);
}

#[cfg(test)]
mod paper_tests {
    use super::*;
    #[test]
    fn long_paper_stays_centered_and_inside_wide_and_narrow_panels() {
        for width in [1080.0, 1800.0, 2560.0] {
            let ctx = egui::Context::default();
            let paragraph = "A long specification with **strong text**, file paths and details that must remain inside the paper. ".repeat(20);
            let spec = format!(
                "# Specification\n\n{paragraph}\n\n| Field | Description |\n| --- | --- |\n| Configuration | {paragraph} |\n\n```rust\n{}\n```",
                "x".repeat(300)
            );
            for _ in 0..3 {
                let mut panel = egui::Rect::NOTHING;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::Panel::left("chat").exact_size(380.0).show(ui, |_| {});
                        egui::CentralPanel::default().show(ui, |ui| {
                            panel = ui.available_rect_before_wrap();
                            egui::ScrollArea::vertical().show(ui, |ui| render(ui, Some(&spec)));
                        });
                    },
                );
                let paper = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == crate::ui::theme::PANEL_ALT
                                && rect.rect.width() >= 600.0 =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .unwrap();
                assert!(
                    paper.left() >= panel.left() - 1.0 && paper.right() <= panel.right() + 1.0,
                    "paper {paper:?}, panel {panel:?}"
                );
                assert!(paper.width() <= 1002.0);
                assert!((paper.center().x - panel.center().x).abs() < 10.0);
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn specification_uses_native_dark_surface_and_readable_text() {
        let ctx = egui::Context::default();
        ctx.set_visuals(crate::ui::theme::koolade_visuals());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            render(ui, Some("# Specification\n\nReadable body with **emphasis**.\n\n```rust\nlet ok = true;\n```"));
        });
        assert!(output.shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == crate::ui::theme::PANEL_ALT)
        ));
        let body = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().contains("Readable body") => Some(t),
                _ => None,
            })
            .unwrap();
        assert!(
            body.galley
                .job
                .sections
                .iter()
                .all(|s| s.format.color == crate::ui::theme::TEXT)
        );
        output.textures_delta.clear();
    }
}
