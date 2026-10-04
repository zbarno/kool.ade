use super::activity_helpers::*;
use crate::ui::layout::activity::task_card_activity_anchor;
#[test]
fn active_window_advances_six_ticks_at_the_minute_boundary_and_profile_shifts() {
    // 96 -> 102 across the 1_020_000 boundary: exactly +6 ticks...
    assert_eq!(
        task_card_activity_anchor(true, None, 1_020_000)
            - task_card_activity_anchor(true, None, 1_019_999),
        6
    );
    // ...and the drawn profile slides one minute (six slots) right:
    // bucket 43 sits in slot 6 under anchor 96 and slot 0 under 102.
    let samples: &[(i64, u64)] = &[(43, 7)];
    let before = render_band(samples, true, None, 1_019_999);
    let after = render_band(samples, true, None, 1_020_000);
    let (before_seg, after_seg) = (baselines(&before)[0], baselines(&after)[0]);
    assert_eq!(before_seg, after_seg, "the band geometry itself is fixed");
    let bottom = after_seg[0].y;
    let (pt_before, pt_after) = (
        polylines(&before)[0].points.clone(),
        polylines(&after)[0].points.clone(),
    );
    assert!(
        (bottom - pt_before[6].y - 40.0).abs() <= 0.5,
        "pre-boundary: bucket 43 fills slot 6 to the plot top"
    );
    assert!(
        (bottom - pt_after[0].y - 40.0).abs() <= 0.5,
        "post-boundary: bucket 43 fills slot 0 to the plot top"
    );
    for (index, p) in pt_before.iter().enumerate() {
        if index != 6 {
            assert!(
                (p.y - bottom).abs() <= 0.01,
                "slot {index} flat pre-boundary"
            );
        }
    }
    for (index, p) in pt_after.iter().enumerate() {
        if index != 0 {
            assert!(
                (p.y - bottom).abs() <= 0.01,
                "slot {index} flat post-boundary"
            );
        }
    }
}
