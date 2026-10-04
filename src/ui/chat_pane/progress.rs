use crate::ui::theme;
use egui::RichText;
pub fn paint_progress(ui: &mut egui::Ui, progress: &crate::harness::LiveProgress) {
    ui.push_id("live_turn", |ui| {
        if !progress.posts.is_empty() {
            for post in &progress.posts {
                ui.push_id(post.id, |ui| {
                    if post.kind == "thinking" {
                        egui::CollapsingHeader::new(
                            RichText::new("Thinking").size(12.0).color(theme::TEXT_DIM),
                        )
                        .id_salt("thought_block")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(post.text.trim())
                                        .size(13.0)
                                        .line_height(Some(20.0))
                                        .color(theme::TEXT_DIM),
                                )
                                .wrap(),
                            );
                        });
                    } else if post.kind == "tool" {
                        egui::CollapsingHeader::new(format!(
                            "Tool output · {}",
                            post.text.lines().next().unwrap_or("tool")
                        ))
                        .id_salt("tool_output")
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&post.text).monospace().size(12.0))
                                    .wrap(),
                            );
                        });
                    } else {
                        // Streaming reply prose: painted progressively,
                        // degrading harmlessly on in-flight prefixes (the
                        // harness projection withholds envelopes and
                        // unterminated opening fences upstream).
                        crate::ui::markdown::paint(ui, post.text.trim(), crate::ui::markdown::CHAT);
                    }
                    ui.add_space(4.0);
                });
            }
            if let Some(activity) = &progress.activity {
                ui.label(RichText::new(activity).size(11.0).weak());
            }
            return;
        }

        if !progress.thoughts.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new("Thinking").size(12.0).color(theme::TEXT_DIM),
            )
            .default_open(true)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(progress.thoughts.trim())
                            .size(13.0)
                            .line_height(Some(20.0))
                            .color(theme::TEXT_DIM),
                    )
                    .wrap(),
                );
            });
            ui.add_space(4.0);
        }
        if let Some(activity) = &progress.activity {
            ui.add(
                egui::Label::new(RichText::new(activity).size(11.0).color(theme::TEXT_DIM)).wrap(),
            );
            ui.add_space(4.0);
        }
        if !progress.response.is_empty() {
            crate::ui::markdown::paint(ui, progress.response.trim(), crate::ui::markdown::CHAT);
            ui.add_space(4.0);
        }
    });
}
