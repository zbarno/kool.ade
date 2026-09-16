//! Task activity presentation, backed by observed stream updates.
use crate::{harness::LiveProgress, ui::theme};
use egui::RichText;

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

/// A gap is a measured zero, never an interpolated or invented update.
pub fn graph(ui: &mut egui::Ui, samples: &[(i64, u64)], live: bool, height: f32) {
    let now = chrono::Utc::now().timestamp_millis() / 10_000;
    let end = if live {
        now
    } else {
        samples
            .iter()
            .map(|(bucket, _)| *bucket)
            .max()
            .unwrap_or(now)
    };
    let values = window(samples, end);
    let peak = values.iter().copied().max().unwrap_or(0).max(1);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let plot = rect.shrink2(egui::vec2(2.0, 4.0));
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
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(1.8, theme::DANGER),
    ));
    response.on_hover_text(format!("Observed updates / 10s · 10-minute window · peak {peak}/10s. {} Empty buckets mean no update arrived, not that a worker stopped. Activity is not completion percentage; token usage is not reported.", if live { "Rolling live window." } else { "Last recorded window." }));
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
    fn red_line_has_sixty_aligned_points_and_zero_gaps() {
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
            if path.points.len() == 60 && path.stroke.color == egui::epaint::ColorMode::Solid(theme::DANGER))));
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
}
