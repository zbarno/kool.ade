use super::super::*;
#[test]
fn task_activity_line_has_sixty_aligned_points_and_zero_gaps() {
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
        if path.points.len() == 60 && path.stroke.color == egui::epaint::ColorMode::Solid(theme::BLUE))));
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
