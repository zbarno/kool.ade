use super::super::*;
use super::test_helpers::*;
#[test]
fn task_graph_keeps_blue_activity_in_live_and_settled_modes() {
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
        [egui::epaint::ColorMode::Solid(theme::BLUE)],
        "live task strip keeps the blue 60-point line"
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
        [egui::epaint::ColorMode::Solid(theme::BLUE)],
        "settled task view keeps the blue 60-point line"
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

/// REQ-F19-3 guard: the All-activity / task-details `full` view retains
/// its header row, the "M SS · N updates" timing line, and the full
/// scrollable post stream while hosting NO chart — zero 60-point paths
/// and no solid-filled-rectangle cluster resembling an accent-bar
/// subchart, so at most one chart represents a task's activity at a time.
#[test]
fn full_view_keeps_header_timing_and_post_stream_without_any_subchart() {
    let progress = LiveProgress {
        telemetry: crate::harness::ActivityTelemetry {
            started_ms: Some(1000),
            finished_ms: Some(61_000),
            updates: 3,
            samples: vec![(100, 3)],
            ..Default::default()
        },
        posts: vec![crate::harness::LivePost {
            id: (1, 0),
            kind: "step".into(),
            text: "did a thing".into(),
        }],
        ..Default::default()
    };
    let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
        full(ui, &progress, false);
    });
    output.textures_delta.clear();
    let texts = labels(&output.shapes);
    assert!(
        texts.iter().any(|text| text.contains("3 updates")),
        "the \"M SS · N updates\" timing line survived, got {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "Recorded activity"),
        "the header row survived, got {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("did a thing")),
        "the full post stream survived via chat_pane::paint_progress, got {texts:?}"
    );
    assert!(
        polylines(&output.shapes).is_empty(),
        "no 60-point path: the subchart does not inhabit the full view"
    );
    let solid_rects = output
        .shapes
        .iter()
        .filter(|c| matches!(&c.shape, egui::Shape::Rect(rect) if rect.fill.a() > 0))
        .count();
    assert!(
        solid_rects < 15,
        "fewer than 15 solid-filled rectangles (a bar cluster would flood this count); got {solid_rects}"
    );
}
