use super::Entry;
use egui::{Color32, RichText};

pub(super) fn paint_entry(ui: &mut egui::Ui, entry: &Entry) {
    ui.push_id(&entry.id, |ui| {
        egui::Frame::NONE
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(8)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(entry.actor)
                            .strong()
                            .color(actor_color(entry.actor)),
                    );
                    ui.label(RichText::new(&entry.kind).small().weak());
                    if let Some(status) = &entry.status {
                        ui.label(RichText::new(status).small().weak());
                    }
                    if let Some(attempt) = entry.run {
                        ui.label(RichText::new(format!("Run {}", attempt)).small().weak());
                    }
                    if let Some(origin) = entry.origin {
                        ui.label(RichText::new(origin).small().weak());
                    }
                    ui.label(RichText::new(time(entry.at_ms)).small().weak());
                });
                let safe = crate::harness::safe_activity_text(&entry.text);
                let summary = clip(&safe, 360);
                ui.add(egui::Label::new(summary).wrap());
                if safe.chars().count() > 360 {
                    ui.collapsing("Technical details", |ui| {
                        ui.add(egui::Label::new(RichText::new(safe).monospace()).wrap());
                    });
                }
            });
    });
    ui.add_space(6.0);
}

fn actor_color(actor: &str) -> Color32 {
    match actor {
        "You" => Color32::from_rgb(84, 166, 255),
        "Worker" => Color32::from_rgb(150, 203, 177),
        "Reviewer" => Color32::from_rgb(202, 179, 255),
        _ => Color32::from_rgb(190, 190, 190),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

fn time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|date| date.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "Time unavailable".into())
}
