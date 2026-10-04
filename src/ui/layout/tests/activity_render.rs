use super::super::*;
use super::activity_helpers::*;
use crate::ui::layout::activity::CARD_ACTIVITY_HOVER;
#[test]
fn band_mount_paints_one_danger_polyline_peak_slot_57_quarter_lift_slot_54() {
    // Settled fixture (updated_ms 1_009_999, samples [(100,4),(97,1)]) ->
    // anchor 102: bucket 100 -> slot 57 (peak 4 -> plot top), bucket 97
    // -> slot 54 (a quarter of the 40 px plot -> 10 px of lift).
    let progress = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
    let samples: &[(i64, u64)] = &[(100, 4), (97, 1)];
    let shapes = render_band(samples, false, Some(&progress), 1_500_000);

    let lines = polylines(&shapes);
    assert_eq!(
        lines.len(),
        1,
        "exactly one chart: a single 60-point polyline"
    );
    let path = lines[0];
    assert!(
        matches!(path.stroke.color, egui::epaint::ColorMode::Solid(color) if color == theme::BLUE),
        "solid theme::BLUE stroke, got {:?}",
        path.stroke.color
    );
    assert!(
        (path.stroke.width - 1.8).abs() <= 0.01,
        "story-1 contractual 1.8px stroke"
    );
    assert_eq!(
        shapes
            .iter()
            .filter(|c| matches!(c.shape, egui::Shape::Path(_)))
            .count(),
        1,
        "no second chart on the band"
    );
    assert!(
        shapes
            .iter()
            .all(|c| !matches!(&c.shape, egui::Shape::Rect(rect) if rect.fill.a() > 0)),
        "no bar fills on the card band"
    );

    // The baseline serves as the coordinate oracle.
    let segs = baselines(&shapes);
    assert_eq!(segs.len(), 1, "exactly one baseline");
    let segment = segs[0];
    let (left, right, bottom) = (segment[0].x, segment[1].x, segment[0].y);

    let points = &path.points;
    // Plot height is exactly 40 px (48 px band minus the 2x4 px insets);
    // only the peak vertex climbs near the top.
    let elevated: Vec<usize> = (0..60).filter(|&i| points[i].y <= bottom - 39.0).collect();
    assert_eq!(elevated, [57], "only slot 57 reaches the plot top");
    let expected_x = left + (right - left) * 57.0 / 59.0;
    assert!(
        (points[57].x - expected_x).abs() <= 0.5,
        "peak x pins the slot-57 anchor placement"
    );
    assert!(
        (points[57].y - (bottom - 40.0)).abs() <= 0.5,
        "peak sits at the 40 px plot top"
    );
    assert!(
        (points[54].y - (bottom - 10.0)).abs() <= 0.5,
        "count-1 slot rises exactly 1/4 of the plot"
    );
    for (index, p) in points.iter().enumerate() {
        if index == 54 || index == 57 {
            continue;
        }
        assert!(
            (p.y - bottom).abs() <= 0.01,
            "slot {index} rests on the baseline"
        );
    }
}

#[test]
fn degenerate_telemetry_yields_one_flat_danger_line_through_the_seam() {
    let stamped = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
    let no_samples: &[(i64, u64)] = &[];
    let zero_buckets: &[(i64, u64)] = &[(5, 0), (6, 0)];
    let pre_window: &[(i64, u64)] = &[(1, 7)];
    let cases: [DegenerateCase<'_>; 4] = [
        (no_samples, None),             // (a) no LiveProgress record at all
        (no_samples, Some(&stamped)),   // (b) record present, samples = []
        (zero_buckets, Some(&stamped)), // (b) all-zero buckets
        (pre_window, Some(&stamped)),   // (c) samples strictly pre-window
    ];
    for (samples, progress) in cases {
        let shapes = render_band(samples, false, progress, 1_015_000);
        let lines = polylines(&shapes);
        assert_eq!(lines.len(), 1, "{samples:?}: exactly one 60-point polyline");
        let path = lines[0];
        assert!(
            matches!(path.stroke.color, egui::epaint::ColorMode::Solid(color) if color == theme::BLUE),
            "{samples:?}: still stroked solid theme::BLUE"
        );
        let segs = baselines(&shapes);
        assert_eq!(segs.len(), 1, "{samples:?}: exactly one border baseline");
        let bottom = segs[0][0].y;
        for (index, p) in path.points.iter().enumerate() {
            assert!(
                p.x.is_finite() && p.y.is_finite(),
                "{samples:?}: no NaN/infinite ordinate at slot {index}"
            );
            assert!(
                (p.y - bottom).abs() <= 0.01,
                "{samples:?}: slot {index} flat on the baseline — no fabricated spike"
            );
        }
        assert_eq!(
            shapes
                .iter()
                .filter(|c| matches!(c.shape, egui::Shape::Path(_)))
                .count(),
            1,
            "{samples:?}: no additional spike-bearing shape"
        );
        assert!(
            shapes
                .iter()
                .all(|c| !matches!(&c.shape, egui::Shape::Rect(rect) if rect.fill.a() > 0)),
            "{samples:?}: the band still reserves its 48 px with no fills"
        );
    }
}

/// One degenerate-sweep case: the band's samples and the record it
/// references (aliased to keep the case-table type out of clippy's
/// type_complexity complaint range).
type DegenerateCase<'a> = (&'a [(i64, u64)], Option<&'a crate::harness::LiveProgress>);

#[test]
fn band_legend_is_the_verbatim_four_phrase_metric_explanation() {
    assert_eq!(
        CARD_ACTIVITY_HOVER,
        "Updates per 10-second bucket · Last 10 minutes · Token usage is not reported · Empty buckets do not mean the worker stopped."
    );
    for phrase in [
        "Updates per 10-second bucket",
        "Last 10 minutes",
        "Token usage is not reported",
        "Empty buckets do not mean the worker stopped",
    ] {
        assert!(CARD_ACTIVITY_HOVER.contains(phrase), "missing: {phrase}");
    }
}
