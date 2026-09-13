//! Task activity presentation, backed by observed stream updates.
use crate::{harness::LiveProgress, ui::theme};
use egui::RichText;

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
    let now = if active {
        chrono::Utc::now().timestamp_millis()
    } else {
        progress.telemetry.updated_ms.unwrap_or_default()
    } / 10_000;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 58.0), egui::Sense::hover());
    let max = progress
        .telemetry
        .samples
        .iter()
        .map(|(_, n)| *n)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let width = rect.width() / 60.0;
    for index in 0..60 {
        let count = progress
            .telemetry
            .samples
            .iter()
            .find(|(bucket, _)| *bucket == now - 59 + index)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        if count > 0 {
            let x = rect.left() + index as f32 * width;
            let bar = egui::Rect::from_min_max(
                egui::pos2(x, rect.bottom() - 54.0 * count as f32 / max),
                egui::pos2(x + (width - 2.0).max(1.0), rect.bottom()),
            );
            ui.painter().rect_filled(bar, 2, theme::ACCENT);
        }
    }
    response.on_hover_text("Observed activity updates in ten-second buckets. Empty space means no update arrived; it does not mean the worker stopped.");
    ui.label(
        RichText::new("Activity updates / 10s · last 10 minutes · token usage is not reported")
            .small()
            .weak(),
    );
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
}
