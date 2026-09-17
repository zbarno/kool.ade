//! Task activity presentation, backed by observed stream updates.
use crate::{harness::LiveProgress, ui::theme};
use egui::RichText;

/// Maps raw ten-second ticks into the 60-slot window ending at `end`: slot `i`
/// accumulates bucket `end - 59 + i`. Duplicate buckets landing in one slot
/// are `saturating_add`ed, capping at `u64::MAX` (accepted; live counters are
/// small in practice).
pub fn window(samples: &[(i64, u64)], end: i64) -> [u64; 60] {
    let mut values = [0u64; 60];
    for &(bucket, count) in samples {
        let offset = bucket - (end - 59);
        if (0..60).contains(&offset) {
            values[offset as usize] = values[offset as usize].saturating_add(count);
        }
    }
    values
}

/// Renders a compact line chart of a 60-slot ten-second-bucket series.
///
/// `anchor` is a ten-second tick (`floor(epoch_ms / 10_000)`) denoting the
/// window's RIGHT-most slot: slot `i` covers bucket `anchor - 59 + i`, computed
/// by [`window`]. Values are peak-normalized by `max(values.max(), 1)`, so an
/// empty, all-zero, pre-window or post-window sample set renders a flat line
/// on the baseline with no panic and no invented spikes.
///
/// The primitive performs no clock reads, no repaint requests, and attaches no
/// labels or hover text; it emits no shapes at all when the shrunken plot
/// collapses to a zero (or negative) width or height.
pub fn line_chart(
    ui: &mut egui::Ui,
    samples: &[(i64, u64)],
    anchor: i64,
    color: egui::Color32,
    size: egui::Vec2,
) -> egui::Response {
    let values = window(samples, anchor);
    let peak = values.iter().copied().max().unwrap_or(0).max(1);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let plot = rect.shrink2(egui::vec2(2.0, 4.0));
    if plot.width() > 0.0 && plot.height() > 0.0 {
        ui.painter().line_segment(
            [plot.left_bottom(), plot.right_bottom()],
            egui::Stroke::new(1.0, theme::BORDER),
        );
        let points = values
            .iter()
            .enumerate()
            .map(|(i, count)| {
                egui::pos2(
                    plot.left() + plot.width() * i as f32 / 59.0,
                    plot.bottom() - plot.height() * (*count as f32 / peak as f32),
                )
            })
            .collect::<Vec<_>>();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(1.8, color)));
    }
    response
}

/// Verbatim legacy hover copy for the activity graph; wording is pinned by
/// tests and kept stable so incumbent chrome does not drift.
fn hover_copy(peak: u64, live: bool) -> String {
    format!(
        "Observed updates / 10s · 10-minute window · peak {peak}/10s. {} Empty buckets mean no update arrived, not that a worker stopped. Activity is not completion percentage; token usage is not reported.",
        if live {
            "Rolling live window."
        } else {
            "Last recorded window."
        }
    )
}

/// Compatibility adapter over [`line_chart`] preserving the incumbent
/// live-flag behaviour: the window anchor comes from the wall clock when
/// `live` and from the newest recorded bucket otherwise, all drawing is
/// delegated to [`line_chart`] in `theme::DANGER`, and the verbatim hover
/// copy, live-only one-second repaint schedule, and small weak label are
/// re-applied in the incumbent painting order.
///
/// A gap is a measured zero, never an interpolated or invented update.
pub fn graph(ui: &mut egui::Ui, samples: &[(i64, u64)], live: bool, height: f32) {
    let now = chrono::Utc::now().timestamp_millis() / 10_000;
    let anchor = if live {
        now
    } else {
        samples
            .iter()
            .map(|(bucket, _)| *bucket)
            .max()
            .unwrap_or(now)
    };
    let response = line_chart(
        ui,
        samples,
        anchor,
        theme::DANGER,
        egui::vec2(ui.available_width(), height),
    );
    // Deliberate mirror of line_chart's internal window scan: the hover copy
    // only needs the peak, and keeping line_chart's signature payload-free is
    // cheaper than threading values out of the primitive.
    let peak = window(samples, anchor)
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(1);
    response.on_hover_text(hover_copy(peak, live));
    if live {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
    }
    ui.label(
        RichText::new(if samples.is_empty() {
            "No activity yet"
        } else if live {
            "Live activity"
        } else {
            "Last recorded activity"
        })
        .small()
        .weak(),
    );
}

pub fn preview(progress: &LiveProgress) -> String {
    let text = progress
        .posts
        .last()
        .map(|p| p.text.as_str())
        .filter(|text| !text.trim().is_empty())
        .or_else(|| (!progress.response.trim().is_empty()).then_some(progress.response.as_str()))
        .or_else(|| (!progress.thoughts.trim().is_empty()).then_some(progress.thoughts.as_str()))
        .or(progress.activity.as_deref())
        .unwrap_or("Waiting for the first update…");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let len = flat.chars().count();
    if len > 200 {
        format!("…{}", flat.chars().skip(len - 200).collect::<String>())
    } else {
        flat
    }
}

pub fn timing(progress: &LiveProgress, active: bool) -> String {
    let t = &progress.telemetry;
    let end = t
        .finished_ms
        .or_else(|| active.then(|| chrono::Utc::now().timestamp_millis()));
    match (t.started_ms, end) {
        (Some(start), Some(end)) => {
            let seconds = (end - start).max(0) / 1000;
            format!(
                "{}m {:02}s · {} updates",
                seconds / 60,
                seconds % 60,
                t.updates
            )
        }
        _ => "Timing unavailable for this run".into(),
    }
}

/// Compact card preview with a distinct action (does not open item details).
pub fn compact(ui: &mut egui::Ui, progress: &LiveProgress, active: bool) -> bool {
    let mut expand = false;
    egui::Frame::NONE
        .fill(theme::BG)
        .corner_radius(5)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if active {
                    ui.spinner();
                }
                ui.label(
                    RichText::new(if active {
                        "LIVE ACTIVITY"
                    } else {
                        "LAST ACTIVITY"
                    })
                    .size(10.0)
                    .color(if active {
                        theme::ACCENT
                    } else {
                        theme::TEXT_DIM
                    }),
                );
            });
            ui.add(
                egui::Label::new(
                    RichText::new(preview(progress))
                        .size(11.5)
                        .line_height(Some(15.0)),
                )
                .wrap(),
            );
            ui.label(RichText::new(timing(progress, active)).size(10.0).weak());
            expand = ui.small_button("View all activity").clicked();
        });
    expand
}

pub fn full(ui: &mut egui::Ui, progress: &LiveProgress, active: bool) {
    ui.horizontal_wrapped(|ui| {
        if active {
            ui.spinner();
        }
        ui.label(
            RichText::new(if active {
                "Worker running"
            } else {
                "Recorded activity"
            })
            .strong(),
        );
        ui.label(timing(progress, active));
        if let Some(last) = progress.telemetry.updated_ms {
            let seconds = (chrono::Utc::now().timestamp_millis() - last).max(0) / 1000;
            if active {
                ui.label(format!("Last update {seconds}s ago"));
            }
        }
    });
    if let Some(activity) = &progress.activity {
        ui.label(activity);
    }
    ui.separator();
    let height = (ui.ctx().content_rect().height() - 290.0).max(140.0);
    egui::ScrollArea::vertical()
        .id_salt("full_task_activity_stream")
        .max_height(height)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            crate::ui::chat_pane::paint_progress(ui, progress);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn red_line_has_sixty_aligned_points_and_zero_gaps() {
        let samples = [(100, 2), (100, 3), (102, 7), (40, 99), (103, 99)];
        let values = window(&samples, 102);
        assert_eq!(values[57..], [5, 0, 7]);
        assert_eq!(window(&samples, 39), [0; 60]);
        assert_eq!(window(&[], 102), [0; 60]);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            graph(ui, &samples[..3], false, 34.0)
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Path(path)
            if path.points.len() == 60 && path.stroke.color == egui::epaint::ColorMode::Solid(theme::DANGER))));
    }
    #[test]
    fn activity_survives_old_records_and_reports_measured_time() {
        let mut progress: LiveProgress =
            serde_json::from_str(r#"{"thoughts":"old activity"}"#).unwrap();
        progress.update(LiveProgress {
            response: "new output".into(),
            ..Default::default()
        });
        assert_eq!(progress.telemetry.updates, 1);
        assert_eq!(progress.telemetry.samples.len(), 1);
        progress.telemetry.started_ms = Some(1000);
        progress.telemetry.finished_ms = Some(66000);
        assert_eq!(timing(&progress, false), "1m 05s · 1 updates");
        let restored: LiveProgress =
            serde_json::from_str(&serde_json::to_string(&progress).unwrap()).unwrap();
        assert_eq!(restored, progress);
        assert_eq!(preview(&restored), "new output");
    }

    /// Shared capture helper: finds the first 60-point polyline in a frame.
    fn line_points(shapes: &[egui::epaint::ClippedShape]) -> Option<&[egui::Pos2]> {
        shapes.iter().find_map(|clip| match &clip.shape {
            egui::Shape::Path(path) if path.points.len() == 60 => Some(path.points.as_slice()),
            _ => None,
        })
    }

    /// Renders `line_chart` (200x48) in a fresh default context; returns the
    /// exact allocated rect and the frame's shapes.
    fn render_line(
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
    fn baselines(shapes: &[egui::epaint::ClippedShape]) -> Vec<(f32, [egui::Pos2; 2])> {
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
    fn polylines(shapes: &[egui::epaint::ClippedShape]) -> Vec<(f32, egui::epaint::ColorMode)> {
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

    fn labels(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|clip| match &clip.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn window_math_aligns_slots_sums_duplicates_and_skips_out_of_range() {
        let samples: &[(i64, u64)] = &[(47, 2), (47, 5), (90, 99), (98, 9), (102, 7), (103, 1)];
        let values = window(samples, 102);
        assert_eq!(values.len(), 60);
        assert_eq!(
            values[4], 7,
            "duplicate buckets at 47 saturate-sum into slot 4"
        );
        assert_eq!(values[47], 99, "bucket 90 lands in slot 90 - (102 - 59)");
        assert_eq!(values[55], 9, "bucket 98 lands in slot 55");
        assert_eq!(values[59], 7, "the anchor bucket is the right-most slot");
        assert_eq!(
            values.iter().sum::<u64>(),
            122,
            "post-window bucket 103 is excluded"
        );
        assert_eq!(
            window(&[(10, 0), (11, 0)], 11),
            [0; 60],
            "zero-count rows contribute nothing"
        );
    }

    #[test]
    fn degenerate_series_render_one_flat_polyline_plus_one_baseline() {
        let cases: [(&[(i64, u64)], i64); 4] = [
            (&[], 100),                 // empty telemetry
            (&[(50, 0), (51, 0)], 100), // in-window, all zero
            (&[(40, 8)], 100),          // strictly before the window
            (&[(160, 9)], 100),         // strictly after the window
        ];
        for (samples, anchor) in cases {
            let (placed, shapes) = render_line(samples, anchor, theme::DANGER);
            assert_ne!(
                placed,
                egui::Rect::NOTHING,
                "{samples:?}@{anchor} allocated"
            );
            let bottom = placed.max.y - 4.0;
            let points = line_points(&shapes)
                .unwrap_or_else(|| panic!("{samples:?}@{anchor}: missing 60-point polyline"));
            assert!(
                points.iter().all(|p| !p.x.is_nan() && !p.y.is_nan()),
                "{samples:?}@{anchor}: no NaN ordinates"
            );
            assert!(
                points.iter().all(|p| (p.y - bottom).abs() <= 0.01),
                "{samples:?}@{anchor}: every vertex sits on the plot bottom, got {:?}",
                points.iter().map(|p| p.y).collect::<Vec<f32>>()
            );
            // The polyline spans the shrunken plot exactly.
            assert!((points[0].x - (placed.min.x + 2.0)).abs() <= 0.01);
            assert!((points[59].x - (placed.max.x - 2.0)).abs() <= 0.01);
            // Exactly one baseline, 1.0px wide, spanning the shrunken plot bottom.
            let baselines = baselines(&shapes);
            assert_eq!(
                baselines.len(),
                1,
                "{samples:?}@{anchor}: exactly one baseline"
            );
            let (bl_width, [bl_tl, bl_br]) = baselines[0];
            assert!(
                (bl_width - 1.0).abs() <= 0.01,
                "contractual 1.0px baseline width"
            );
            assert!((bl_br - egui::pos2(placed.max.x - 2.0, bottom)).length() <= 0.01);
            assert!((bl_tl - egui::pos2(placed.min.x + 2.0, bottom)).length() <= 0.01);
            // No other path-bearing (spike) shape exists; the lone polyline
            // carries the contractual 1.8px stroke.
            let path_count = shapes
                .iter()
                .filter(|clip| matches!(clip.shape, egui::Shape::Path(_)))
                .count();
            assert_eq!(
                path_count, 1,
                "{samples:?}@{anchor}: no extra path/spike shapes"
            );
            let (pl_width, _) = polylines(&shapes)[0];
            assert!(
                (pl_width - 1.8).abs() <= 0.01,
                "contractual 1.8px polyline width"
            );
        }
    }

    #[test]
    fn unique_peak_sits_at_plot_top_under_max_peak_one_scaling() {
        // Buckets 99 -> 3, 100 -> 5, 101 -> 9 with anchor 101 (slots 57/58/59).
        let (placed, shapes) = render_line(&[(99, 3), (100, 5), (101, 9)], 101, theme::DANGER);
        let top = placed.min.y + 4.0;
        let bottom = placed.max.y - 4.0;
        let height = bottom - top;
        let points = line_points(&shapes).expect("60-point polyline");
        // The argmax vertex (slot 59) reaches the plot top.
        assert!(
            (points[59].y - top).abs() <= 0.5,
            "argmax vertex at plot top"
        );
        // More counted vertices rise higher: slot 58 (5/9) sits strictly
        // below the top vertex, and slot 57 (3/9) strictly below slot 58.
        // (Screen y grows downward: lower on screen == larger y.)
        assert!(
            points[58].y > points[59].y,
            "slot 58 strictly below the top vertex"
        );
        assert!(
            points[57].y > points[58].y,
            "slot 57 strictly below slot 58"
        );
        assert!(
            (bottom - points[57].y) > 0.01 && (bottom - points[57].y) < height,
            "3-count vertex strictly between top and baseline"
        );
        // Exact linear peak-division (rules out sqrt/log-like scalings):
        // each filled vertex sits at count/peak of the usable plot height.
        let frac = |p: &egui::Pos2| (bottom - p.y) / height;
        assert!(
            (frac(&points[59]) - 1.0).abs() <= 0.02,
            "slot 59 fills 9/9 of the plot height"
        );
        assert!(
            (frac(&points[58]) - 5.0 / 9.0).abs() <= 0.02,
            "slot 58 fills 5/9 of the plot height"
        );
        assert!(
            (frac(&points[57]) - 3.0 / 9.0).abs() <= 0.02,
            "slot 57 fills 3/9 of the plot height"
        );
        // Slots 57/58/59 are the only nonzero buckets; every other slot is
        // flush with the baseline, and slot 59 is the unique highest vertex.
        for (j, p) in points.iter().enumerate() {
            if (57..=59).contains(&j) {
                continue;
            }
            let lift = (bottom - p.y).abs();
            assert!(
                lift <= 0.01,
                "slot {j} flush with the baseline, lift={lift}"
            );
        }
    }

    #[test]
    fn acceptance_two_vertices_place_counts_by_slot_mapping_and_peak_fractions() {
        // Acceptance-criterion fixture [(96,2),(101,7),(102,3)] at anchor 102:
        // slot i <-> bucket anchor-59+i = 43+i, so bucket 101 (the unique
        // peak, 7) lands in slot 58, bucket 102 in slot 59 (3), and bucket 96
        // in slot 53 (2).
        let (placed, shapes) = render_line(&[(96, 2), (101, 7), (102, 3)], 102, theme::DANGER);
        let top = placed.min.y + 4.0;
        let bottom = placed.max.y - 4.0;
        let height = bottom - top;
        let points = line_points(&shapes).expect("60-point polyline");
        assert!(
            (points[58].y - top).abs() <= 0.02,
            "unique-peak vertex (7/7) sits at the plot's top edge"
        );
        let frac = |p: &egui::Pos2| (bottom - p.y) / height;
        assert!(
            (frac(&points[58]) - 1.0).abs() <= 0.02,
            "peak 7 fills 7/7 of the plot height"
        );
        assert!(
            (frac(&points[59]) - 3.0 / 7.0).abs() <= 0.02,
            "slot 59 (count 3) rises 3/7 of the plot height"
        );
        assert!(
            (frac(&points[53]) - 2.0 / 7.0).abs() <= 0.02,
            "the count-2 vertex rises exactly 2/7 of the plot height"
        );
        // Every other vertex lies exactly on the baseline.
        for (j, p) in points.iter().enumerate() {
            if [53, 58, 59].contains(&j) {
                continue;
            }
            let lift = (bottom - p.y).abs();
            assert!(
                lift <= 0.01,
                "slot {j} flush with the baseline, lift={lift}"
            );
        }
        assert_eq!(
            polylines(&shapes).len(),
            1,
            "exactly one polyline for the fixture"
        );
        assert_eq!(
            baselines(&shapes).len(),
            1,
            "baseline accompanies the fixture"
        );
    }

    #[test]
    fn anchor_positions_the_window_without_any_clock_dependency() {
        // Bucket 441 sits on the window's left edge and 500 on its right edge
        // at anchor 500; identical inputs must give identical geometry.
        let samples: &[(i64, u64)] = &[(500, 7), (441, 3)];
        let (placed_a, shapes_a) = render_line(samples, 500, theme::DANGER);
        let (_, shapes_b) = render_line(samples, 500, theme::DANGER);
        let points_a = line_points(&shapes_a).expect("first capture");
        let points_b = line_points(&shapes_b).expect("second capture");
        assert_eq!(
            points_a, points_b,
            "fresh contexts give element-wise-equal point sets (no clock leak)"
        );

        let (placed_c, shapes_c) = render_line(samples, 501, theme::DANGER);
        let bottom_a = placed_a.max.y - 4.0;
        let bottom_c = placed_c.max.y - 4.0;
        assert_eq!(placed_a, placed_c, "same allocation across contexts");
        let points_c = line_points(&shapes_c).expect("shifted capture");

        // At anchor 500 both edge buckets are occupied...
        assert!(
            (bottom_a - points_a[0].y).abs() > 1.0,
            "slot 0 holds bucket 441"
        );
        assert!(
            (bottom_a - points_a[59].y).abs() > 1.0,
            "slot 59 holds bucket 500"
        );
        // ...and at anchor 501 the profile slides one slot left: bucket 441
        // falls out (slot 0 flat) and bucket 500 vacates the rightmost slot.
        assert!(
            (bottom_c - points_c[0].y).abs() <= 0.01,
            "left-edge bucket left the window"
        );
        assert!(
            (bottom_c - points_c[58].y).abs() > 1.0,
            "bucket 500 now sits in slot 58"
        );
        assert!(
            (bottom_c - points_c[59].y).abs() <= 0.01,
            "former rightmost slot zeroed"
        );
    }

    #[test]
    fn line_color_argument_selects_the_polyline_stroke() {
        let samples: &[(i64, u64)] = &[(100, 2), (101, 7), (102, 3)];
        let (_, purple_shapes) = render_line(samples, 102, theme::PURPLE);
        let (_, danger_shapes) = render_line(samples, 102, theme::DANGER);
        assert_eq!(
            polylines(&purple_shapes)
                .iter()
                .map(|(_, c)| c)
                .cloned()
                .collect::<Vec<_>>(),
            [egui::epaint::ColorMode::Solid(theme::PURPLE)],
            "PURPLE argument strokes the polyline PURPLE"
        );
        assert_eq!(
            polylines(&danger_shapes)
                .iter()
                .map(|(_, c)| c)
                .cloned()
                .collect::<Vec<_>>(),
            [egui::epaint::ColorMode::Solid(theme::DANGER)],
            "DANGER argument strokes the polyline DANGER"
        );
        assert_ne!(
            egui::epaint::ColorMode::Solid(theme::PURPLE),
            egui::epaint::ColorMode::Solid(theme::DANGER),
            "guard colors must differ so the singularity check is meaningful"
        );
    }

    #[test]
    fn legacy_graph_adapter_keeps_incumbent_chrome_in_live_and_settled_modes() {
        // Mirrors the layout.rs:108 main-chat strip: live mode, empty telemetry.
        let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
            graph(ui, &[], true, 24.0);
        });
        output.textures_delta.clear();
        assert_eq!(
            polylines(&output.shapes)
                .iter()
                .map(|(_, c)| c)
                .cloned()
                .collect::<Vec<_>>(),
            [egui::epaint::ColorMode::Solid(theme::DANGER)],
            "live strip keeps the DANGER-red 60-point line"
        );
        assert!(
            polylines(&output.shapes)
                .iter()
                .all(|(w, _)| (w - 1.8).abs() <= 0.01),
            "live strip keeps the 1.8px polyline"
        );
        let bls = baselines(&output.shapes);
        assert_eq!(bls.len(), 1, "live strip keeps the baseline");
        assert!(
            (bls[0].0 - 1.0).abs() <= 0.01,
            "live strip keeps the 1.0px baseline"
        );
        assert_eq!(labels(&output.shapes), ["No activity yet"]);

        // Mirrors the layout.rs:628 item-details collapsing: settled mode, 48px.
        let samples: &[(i64, u64)] = &[(100, 2), (102, 7)];
        let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
            graph(ui, samples, false, 48.0);
        });
        output.textures_delta.clear();
        assert_eq!(
            polylines(&output.shapes)
                .iter()
                .map(|(_, c)| c)
                .cloned()
                .collect::<Vec<_>>(),
            [egui::epaint::ColorMode::Solid(theme::DANGER)],
            "settled details view keeps the DANGER-red 60-point line"
        );
        assert!(
            polylines(&output.shapes)
                .iter()
                .all(|(w, _)| (w - 1.8).abs() <= 0.01),
            "settled details view keeps the 1.8px polyline"
        );
        let bls = baselines(&output.shapes);
        assert_eq!(bls.len(), 1, "settled details view keeps the baseline");
        assert!(
            (bls[0].0 - 1.0).abs() <= 0.01,
            "settled details view keeps the 1.0px baseline"
        );
        assert_eq!(labels(&output.shapes), ["Last recorded activity"]);
    }

    /// Sub-threshold allocations (the 2px x 4px shrink inset leaves width<=0 /
    /// height<=0) still allocate a Response sized as requested, but emit NEITHER
    /// a baseline NOR any polyline at all.
    #[test]
    fn collapsed_plot_allocates_but_emits_no_shapes() {
        let samples: &[(i64, u64)] = &[(100, 5)];
        for size in [egui::vec2(3.0, 5.0), egui::vec2(0.0, 0.0)] {
            let mut placed = egui::Rect::NOTHING;
            let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
                placed = line_chart(ui, samples, 100, theme::DANGER, size).rect;
            });
            output.textures_delta.clear();
            assert_eq!(
                placed.size(),
                size,
                "{size:?}: Response still allocates the requested size"
            );
            let drawn = output
                .shapes
                .iter()
                .filter(|c| {
                    matches!(
                        c.shape,
                        egui::Shape::LineSegment { .. } | egui::Shape::Path(_)
                    )
                })
                .count();
            assert_eq!(
                drawn, 0,
                "{size:?}: a shrunk plot at/below zero size must suppress every drawing"
            );
        }
    }

    #[test]
    fn legacy_hover_copy_is_byte_identical_for_every_mode() {
        let samples: &[(i64, u64)] = &[(100, 2), (102, 7)];
        let peak = window(samples, 102)
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .max(1);
        assert_eq!(peak, 7);
        assert_eq!(
            hover_copy(peak, false),
            "Observed updates / 10s · 10-minute window · peak 7/10s. Last recorded window. Empty buckets mean no update arrived, not that a worker stopped. Activity is not completion percentage; token usage is not reported."
        );
        assert_eq!(
            hover_copy(peak, true),
            "Observed updates / 10s · 10-minute window · peak 7/10s. Rolling live window. Empty buckets mean no update arrived, not that a worker stopped. Activity is not completion percentage; token usage is not reported."
        );
        // Retained cosmetic quirk: an all-zero/empty window prints peak 1/10s.
        assert!(hover_copy(1, false).starts_with(
            "Observed updates / 10s · 10-minute window · peak 1/10s. Last recorded window."
        ));
        assert!(hover_copy(1, true).starts_with(
            "Observed updates / 10s · 10-minute window · peak 1/10s. Rolling live window."
        ));
    }
}
