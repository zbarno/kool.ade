use crate::ui::theme;

/// Shared telemetry footer for implementation tasks and active planning work.
pub(super) fn paint_card(
    ui: &mut egui::Ui,
    surface: &dyn crate::ui::Surface,
    key: &str,
    always_show: bool,
) {
    let samples = surface.activity_samples(Some(key));
    let active = surface.activity_active(key) || surface.active_planning_work() == Some(key);
    if !always_show && !active && samples.is_empty() {
        return;
    }
    let label = if active {
        "LIVE ACTIVITY"
    } else if samples.is_empty() {
        "NO ACTIVITY YET"
    } else {
        "RECORDED ACTIVITY"
    };
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(theme::metadata_text("10 min")).extend());
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(theme::metadata_text(label)).truncate())
                    .on_hover_text(label);
            });
        });
    });
    task_card_activity_band(
        ui,
        &samples,
        active,
        surface.task_progress(key),
        chrono::Utc::now().timestamp_millis(),
    );
}

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
/// Active regime: the current ten-second bucket, including new updates as they
/// arrive. A live graph must not hide the current minute's activity.
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
        now_ms / 10_000
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
    let mut anchor = task_card_activity_anchor(active, progress, now_ms);
    if !active && let Some(latest) = samples.iter().map(|(bucket, _)| *bucket).max() {
        let recorded = ((latest / 6) + 1) * 6;
        anchor = if progress.is_some() {
            anchor.max(recorded)
        } else {
            recorded
        };
    }
    let fill = ui.painter().add(egui::Shape::Noop);
    let response = crate::ui::task_activity::line_chart(
        ui,
        samples,
        anchor,
        theme::BLUE,
        egui::vec2(ui.available_width(), CARD_ACTIVITY_BAND_PX),
    );
    let plot = response.rect.shrink2(egui::vec2(2.0, 4.0));
    let values = crate::ui::task_activity::window(samples, anchor);
    let peak = values.iter().copied().max().unwrap_or(0);
    if peak > 0 && plot.is_positive() {
        let points: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                egui::pos2(
                    plot.left() + plot.width() * i as f32 / 59.0,
                    plot.bottom() - plot.height() * *value as f32 / peak as f32,
                )
            })
            .collect();
        // A translucent gradient follows measured samples down to the baseline.
        // Empty buckets stay flat; decoration never invents worker activity.
        let mut mesh = egui::Mesh::default();
        for point in &points {
            mesh.colored_vertex(*point, theme::BLUE.gamma_multiply(0.28));
            mesh.colored_vertex(
                egui::pos2(point.x, plot.bottom()),
                theme::BLUE.gamma_multiply(0.015),
            );
        }
        for i in 0..59_u32 {
            mesh.add_triangle(i * 2, i * 2 + 1, i * 2 + 2);
            mesh.add_triangle(i * 2 + 1, i * 2 + 3, i * 2 + 2);
        }
        ui.painter().set(fill, egui::Shape::mesh(mesh));
        if let Some(index) = values.iter().rposition(|value| *value > 0) {
            let point = points[index];
            let pulse = if active && !theme::reduced_motion(ui.ctx()) {
                ui.input(|i| (i.time * 2.0).sin() as f32 * 0.5 + 0.5)
            } else {
                0.0
            };
            ui.painter()
                .circle_filled(point, 5.0 + pulse * 2.0, theme::BLUE.gamma_multiply(0.15));
            ui.painter().circle_filled(point, 2.5, theme::BLUE_BRIGHT);
        }
    }
    response.on_hover_text(CARD_ACTIVITY_HOVER);
    if active {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
    }
}
