//! Task activity presentation, backed by observed stream updates.
use crate::{harness::LiveProgress, ui::theme};
use egui::RichText;

mod global;
pub(crate) use global::header_graph;

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
/// delegated to [`line_chart`] in Kool Blue, and the verbatim hover
/// copy, live-only one-second repaint schedule, and small weak label are
/// re-applied in the incumbent painting order.
///
/// A gap is a measured zero, never an interpolated or invented update.
pub fn graph(ui: &mut egui::Ui, samples: &[(i64, u64)], live: bool, height: f32) {
    graph_with_color(ui, samples, live, height, theme::BLUE);
}

pub fn graph_with_color(
    ui: &mut egui::Ui,
    samples: &[(i64, u64)],
    live: bool,
    height: f32,
    color: egui::Color32,
) {
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
        color,
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
                    theme::operation_indicator(ui);
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
            theme::operation_indicator(ui);
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
        ui.add(egui::Label::new(RichText::new(activity).weak()).wrap());
    }
    if !progress.response.trim().is_empty() {
        ui.add_space(6.0);
        ui.heading("Latest result");
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                crate::ui::markdown::paint(ui, progress.response.trim(), crate::ui::markdown::CHAT)
            });
    } else if let Some(post) = progress
        .posts
        .iter()
        .rev()
        .find(|post| post.kind != "thinking")
    {
        ui.add_space(6.0);
        ui.heading("Latest update");
        if post.kind == "assistant" {
            crate::ui::markdown::paint(ui, post.text.trim(), crate::ui::markdown::CHAT);
        } else {
            ui.add(egui::Label::new(RichText::new(&post.text).monospace()).wrap());
        }
    }
    if !progress.thoughts.trim().is_empty() {
        ui.collapsing("Worker notes", |ui| {
            crate::ui::markdown::paint(ui, progress.thoughts.trim(), crate::ui::markdown::CHAT);
        });
    }
    if !progress.posts.is_empty() || !progress.response.trim().is_empty() {
        ui.collapsing("Full activity log", |ui| {
            let height = (ui.ctx().content_rect().height() - 290.0).max(140.0);
            egui::ScrollArea::vertical()
                .id_salt("full_task_activity_stream")
                .max_height(height)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| crate::ui::chat_pane::paint_progress(ui, progress));
        });
    }
}

#[cfg(test)]
mod tests;
