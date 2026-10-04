use super::super::*;
/// Shared capture helper: finds the first 60-point polyline in a frame.
pub(super) fn line_points(shapes: &[egui::epaint::ClippedShape]) -> Option<&[egui::Pos2]> {
    shapes.iter().find_map(|clip| match &clip.shape {
        egui::Shape::Path(path) if path.points.len() == 60 => Some(path.points.as_slice()),
        _ => None,
    })
}

/// Renders `line_chart` (200x48) in a fresh default context; returns the
/// exact allocated rect and the frame's shapes.
pub(super) fn render_line(
    samples: &[(i64, u64)],
    anchor: i64,
    color: egui::Color32,
) -> (egui::Rect, Vec<egui::epaint::ClippedShape>) {
    let mut placed = egui::Rect::NOTHING;
    let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
        placed = line_chart(ui, samples, anchor, color, egui::vec2(200.0, 48.0)).rect;
    });
    output.textures_delta.clear();
    (placed, output.shapes)
}

/// `(stroke width, endpoints)` of each border-colored baseline segment.
pub(super) fn baselines(shapes: &[egui::epaint::ClippedShape]) -> Vec<(f32, [egui::Pos2; 2])> {
    shapes
        .iter()
        .filter_map(|clip| match &clip.shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == theme::BORDER => {
                Some((stroke.width, *points))
            }
            _ => None,
        })
        .collect()
}

/// `(stroke width, color)` of each 60-point polyline in the frame.
pub(super) fn polylines(
    shapes: &[egui::epaint::ClippedShape],
) -> Vec<(f32, egui::epaint::ColorMode)> {
    shapes
        .iter()
        .filter_map(|clip| match &clip.shape {
            egui::Shape::Path(path) if path.points.len() == 60 => {
                Some((path.stroke.width, path.stroke.color.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn labels(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|clip| match &clip.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}
