use super::super::*;
use super::test_helpers::*;
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
