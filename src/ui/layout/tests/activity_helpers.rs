use super::super::*;
/// Builds a settled-record fixture by mutating `LiveProgress::default()`'
/// public telemetry fields; no wall clock involved.
pub(super) fn settled_fixture(
    updated_ms: Option<i64>,
    samples: &[(i64, u64)],
) -> crate::harness::LiveProgress {
    let mut progress = crate::harness::LiveProgress::default();
    progress.telemetry.updated_ms = updated_ms;
    progress.telemetry.samples = samples.to_vec();
    progress
}

/// Runs `body` in a fresh default egui context and returns the emitted
/// shapes (texture deltas cleared, mirroring story 1's captures).
pub(super) fn capture(body: impl FnMut(&mut egui::Ui)) -> Vec<egui::epaint::ClippedShape> {
    let mut output = egui::Context::default().run_ui(Default::default(), body);
    output.textures_delta.clear();
    output.shapes
}

/// The 60-point polylines in the frame.
pub(super) fn polylines(shapes: &[egui::epaint::ClippedShape]) -> Vec<&egui::epaint::PathShape> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Path(path) if path.points.len() == 60 => Some(path),
            _ => None,
        })
        .collect()
}

/// The border-colored baseline segments in the frame (coordinate oracle).
pub(super) fn baselines(shapes: &[egui::epaint::ClippedShape]) -> Vec<[egui::Pos2; 2]> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == theme::BORDER => {
                Some(*points)
            }
            _ => None,
        })
        .collect()
}

/// Renders the card band through the mount seam
/// (`task_card_activity_band`), never the bare primitive.
pub(super) fn render_band(
    samples: &[(i64, u64)],
    active: bool,
    progress: Option<&crate::harness::LiveProgress>,
    now_ms: i64,
) -> Vec<egui::epaint::ClippedShape> {
    capture(move |ui| task_card_activity_band(ui, samples, active, progress, now_ms))
}
