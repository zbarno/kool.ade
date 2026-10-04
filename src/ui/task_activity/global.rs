use super::{hover_copy, line_chart, window};

/// Header-only live chart: preserves the rolling measurement window, hover
/// explanation, and update cadence while letting the header own its label.
pub(crate) fn header_graph(
    ui: &mut egui::Ui,
    samples: &[(i64, u64)],
    color: egui::Color32,
    height: f32,
) {
    let anchor = chrono::Utc::now().timestamp_millis() / 10_000;
    let response = line_chart(
        ui,
        samples,
        anchor,
        color,
        egui::vec2(ui.available_width(), height),
    );
    let peak = window(samples, anchor)
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(1);
    response.on_hover_text(hover_copy(peak, true));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs(1));
}
