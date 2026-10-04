use crate::ui::theme;

/// Height (pixels) of the task card's Kool Blue activity-line band, chosen inside
/// P3's bounded 40–56 px card-width band; the constant vertical reservation
/// keeps card geometry stable regardless of telemetry density.
pub(crate) const CARD_ACTIVITY_BAND_PX: f32 = 48.0;

/// The metric-meaningful hover legend (REQ-F19-2) carried by the task card's
/// activity band; attached unconditionally to the line chart's response so
/// flat-baseline cards explain the metric too.
pub(crate) const CARD_ACTIVITY_HOVER: &str = "Updates per 10-second bucket · Last 10 minutes · Token usage is not reported · Empty buckets do not mean the worker stopped.";

/// Window edge (rightmost ten-second tick) for a task card's 60-slot activity
/// line, derived from card state. Six ticks make a minute.
///
/// Active regime: `(now_ms / 60_000) * 6` — the right edge sits at the current
/// minute's START (story 1's sanctioned live anchor), so the in-progress
/// minute is never peered into and the window rolls forward exactly six ticks
/// per minute across repaints.
///
/// Settled regime: `((last_ms / 60_000) + 1) * 6` — the CLOSE of the record's
/// last-updated minute, with `last_ms` falling back through
/// `telemetry.updated_ms` to the newest recorded sample bucket's end proxy
/// (`bucket * 10_000`) to `now_ms`. Minute-close is deliberate: bucket `b`
/// covers `[b * 10_000, b * 10_000 + 9999]`, so minute-start alignment would
/// strand the final burst of the last updated minute outside the window
/// (e.g. updated_ms = 1_009_999 puts bucket 100 past tick 96's right edge),
/// while minute-close keeps every recorded sample of that minute in view, keeps
/// both regimes on the same six-tick grid, and — because `last_ms` is a pure
/// function of persisted record fields — makes the settled anchor identical
/// across repaints and relaunches (the static final-window guarantee).
pub(crate) fn task_card_activity_anchor(
    active: bool,
    progress: Option<&crate::harness::LiveProgress>,
    now_ms: i64,
) -> i64 {
    if active {
        (now_ms / 60_000) * 6
    } else {
        let last_ms = progress
            .and_then(|p| {
                p.telemetry.updated_ms.or_else(|| {
                    p.telemetry
                        .samples
                        .iter()
                        .map(|(bucket, _)| *bucket * 10_000)
                        .max()
                })
            })
            .unwrap_or(now_ms);
        ((last_ms / 60_000) + 1) * 6
    }
}

/// Mounts the task card's blue activity band directly below the card's
/// existing activity preview: computes the card-state-derived anchor
/// ([task_card_activity_anchor]), delegates all drawing to story 1's
/// payload-free primitive `crate::ui::task_activity::line_chart` in
/// `theme::BLUE` over a [CARD_ACTIVITY_BAND_PX]-tall card-width band,
/// attaches [CARD_ACTIVITY_HOVER] unconditionally to the primitive's returned
/// response (so the legend survives degenerate windows, REQ-F19-2), and — per
/// story 1's contract, repaint cadence lives at the mount, not the primitive
/// — schedules the one-second repaint only while this card's worker is
/// running (`active`). The clock arrives as an injected parameter so tests
/// stay wall-clock-free.
pub(crate) fn task_card_activity_band(
    ui: &mut egui::Ui,
    samples: &[(i64, u64)],
    active: bool,
    progress: Option<&crate::harness::LiveProgress>,
    now_ms: i64,
) {
    let anchor = task_card_activity_anchor(active, progress, now_ms);
    let response = crate::ui::task_activity::line_chart(
        ui,
        samples,
        anchor,
        theme::BLUE,
        egui::vec2(ui.available_width(), CARD_ACTIVITY_BAND_PX),
    );
    response.on_hover_text(CARD_ACTIVITY_HOVER);
    if active {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
    }
}
