use super::activity_helpers::*;
use crate::ui::layout::activity::task_card_activity_anchor;
#[test]
fn anchor_includes_current_live_bucket_and_freezes_settled_window() {
    // Active: right edge is the current minute's ten-second bucket; it advances at the bucket boundary.
    assert_eq!(task_card_activity_anchor(true, None, 1_015_000), 101);
    assert_eq!(task_card_activity_anchor(true, None, 1_019_999), 101);
    assert_eq!(task_card_activity_anchor(true, None, 1_020_000), 102);
    // Settled: minute-CLOSE of telemetry.updated_ms, wall-clock-blind.
    let stamped = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
    assert_eq!(task_card_activity_anchor(false, Some(&stamped), 0), 102);
    assert_eq!(
        task_card_activity_anchor(false, Some(&stamped), 9_000_000),
        102
    );
    // The minute-boundary instant 960_000 belongs to that minute.
    let boundary = settled_fixture(Some(960_000), &[]);
    assert_eq!(
        task_card_activity_anchor(false, Some(&boundary), 1_015_000),
        102
    );
    // Stamp-less record: the newest sample bucket 100 proxies the last
    // update (end proxy 1_000_000 ms) and lands in-window at slot 57
    // through the shared window math.
    let unstamped = settled_fixture(None, &[(100, 5), (97, 2)]);
    assert_eq!(task_card_activity_anchor(false, Some(&unstamped), 7), 102);
    assert_eq!(
        crate::ui::task_activity::window(&[(100, 5), (97, 2)], 102)[57],
        5
    );
    // Record-less card: falls back to now_ms, still minute-CLOSE aligned.
    assert_eq!(task_card_activity_anchor(false, None, 1_015_000), 102);
}

#[test]
fn settled_anchor_is_static_across_repaints_and_relaunch() {
    // Same record bytes at widely differing wall clocks: the frozen
    // window depends only on persisted record fields.
    let progress = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
    let anchors: Vec<i64> = [500_000i64, 1_000_000, 9_000_000]
        .iter()
        .map(|&now_ms| task_card_activity_anchor(false, Some(&progress), now_ms))
        .collect();
    assert_eq!(anchors, [102, 102, 102], "same record, differing now_ms");
    // Two successive calls agree bit-for-bit (relaunch-survival proxy).
    assert_eq!(
        task_card_activity_anchor(false, Some(&progress), 42),
        task_card_activity_anchor(false, Some(&progress), 43)
    );
    // And the rendered point clouds agree element-wise.
    let samples: &[(i64, u64)] = &[(100, 4), (97, 1)];
    let first = polylines(&render_band(samples, false, Some(&progress), 500_000))[0]
        .points
        .clone();
    let second = polylines(&render_band(samples, false, Some(&progress), 9_000_000))[0]
        .points
        .clone();
    assert_eq!(
        first, second,
        "the final 10-minute window stays permanently static"
    );
}
