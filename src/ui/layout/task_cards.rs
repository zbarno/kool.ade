pub(super) mod motion;
use super::ChatTabs;

use crate::core::implementation::ImplementationStatus;
use crate::ui::{Surface, theme};

pub(crate) fn board_card(
    ui: &mut egui::Ui,
    key: &str,
    kind: Option<crate::domain::ItemKind>,
    active: bool,
    attention: bool,
    done: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(key, |ui| {
        let progress = motion::enter(ui);
        let shape_start = motion::shape_count(ui);
        let frame = theme::board_state_frame(kind, active, attention, done);
        let corners = frame.corner_radius;
        let base_fill = frame.fill;
        let halo = ui.painter().add(egui::Shape::Noop);
        let background = ui.painter().add(egui::Shape::Noop);
        let accent = ui.painter().add(egui::Shape::Noop);
        let border = ui.painter().add(egui::Shape::Noop);
        let response = frame
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, egui::Color32::TRANSPARENT))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.spacing_mut().item_spacing.y = 8.0;
                body(ui);
            });
        let hovered = ui
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|position| response.response.rect.contains(position));
        let relation_id = egui::Id::new("koolade_board_relationship_map");
        let relationships = ui.ctx().data_mut(|data| {
            data.get_temp::<std::collections::HashMap<String, std::collections::HashSet<String>>>(
                relation_id,
            )
        });
        let mut related_keys = relationships
            .and_then(|map| map.get(key).cloned())
            .unwrap_or_default();
        related_keys.insert(key.to_owned());
        if hovered {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_board_hover_next"), related_keys);
            });
        }
        let active_related = ui
            .ctx()
            .data_mut(|data| {
                data.get_temp::<std::collections::HashSet<String>>(egui::Id::new(
                    "koolade_board_hover_active",
                ))
            })
            .is_some_and(|keys| keys.len() > 1 && keys.contains(key));
        let active_mix = ui
            .ctx()
            .animate_bool(egui::Id::new(("card_active", key)), active);
        let attention_mix = ui
            .ctx()
            .animate_bool(egui::Id::new(("card_attention", key)), attention);
        let done_mix = ui
            .ctx()
            .animate_bool(egui::Id::new(("card_done", key)), done);
        let idle_fill = if active { theme::PANEL_ALT } else { base_fill };
        let fill = idle_fill.lerp_to_gamma(theme::CARD_HOVER, active_mix);
        let fill = if hovered {
            fill.lerp_to_gamma(theme::CARD_HOVER, 1.0)
        } else {
            fill
        };
        let edge_color = theme::board_hue(kind);
        let emphasis = active_mix.max(attention_mix).max(done_mix);
        let edge_width = 1.0 + emphasis;
        if emphasis > 0.0 || hovered {
            let strength = if hovered { 0.8 } else { emphasis };
            ui.painter().set(
                halo,
                egui::Shape::Vec(
                    (1..=4)
                        .rev()
                        .map(|step| {
                            egui::Shape::rect_stroke(
                                response.response.rect.expand(step as f32),
                                corners,
                                egui::Stroke::new(
                                    2.0,
                                    edge_color.gamma_multiply(strength * 0.035 * (5 - step) as f32),
                                ),
                                egui::StrokeKind::Outside,
                            )
                        })
                        .collect(),
                ),
            );
        }
        ui.painter().set(
            background,
            egui::Shape::rect_filled(response.response.rect, corners, fill),
        );
        ui.painter().set(
            border,
            egui::Shape::rect_stroke(
                response.response.rect,
                corners,
                egui::Stroke::new(edge_width, edge_color),
                egui::StrokeKind::Inside,
            ),
        );
        let rect = response.response.rect;
        let cap = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(12.0, 0.0),
            egui::pos2(rect.right() - 12.0, rect.top() + 2.0),
        );
        ui.painter()
            .set(accent, egui::Shape::rect_filled(cap, 1, edge_color));
        motion::flourish(ui, rect, progress, attention, done);
        motion::remember(ui, shape_start, rect);
        if active_related {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                corners,
                egui::Stroke::new(2.0, theme::BLUE_BRIGHT),
                egui::StrokeKind::Outside,
            );
        }
        ui.add_space(10.0);
    });
}

pub(crate) fn task_key(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    if name
        .split_once("-TASK-")
        .is_some_and(|(id, _)| crate::artifacts::product_docs::valid_feature_id(id))
    {
        return name.to_owned();
    }
    let prefix = name.split('-').next().unwrap_or(name);
    format!("TASK-{}", prefix.to_uppercase())
}

pub(crate) fn task_conversation(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    key: &str,
    expanded: bool,
) -> bool {
    if !crate::artifacts::layout::ArtifactLayout::is_task_ticket_path(key)
        && crate::ui::task_chat::paint_with_board(ui, s, key, expanded, board)
    {
        let id = egui::Id::new("koolade_chat_tabs");
        ui.ctx().data_mut(|data| {
            let mut tabs = data.get_temp::<ChatTabs>(id).unwrap_or_default();
            tabs.open(key);
            data.insert_temp(id, tabs);
        });
        ui.ctx().request_repaint();
    }
    false
}

pub(crate) fn paint_task_failure(ui: &mut egui::Ui, s: &dyn Surface, ticket: &str) {
    if s.implementation_active(ticket) {
        return;
    }
    let record = s.implementation_state(ticket);
    if let Some(error) = record.and_then(|r| r.cleanup.error.as_deref()) {
        ui.colored_label(theme::DANGER, "Cleanup needs attention");
        ui.label(card_summary(error)).on_hover_text(error);
        ui.collapsing("Cleanup details", |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    ui.label(error);
                });
            if ui.small_button("Copy cleanup failure").clicked() {
                ui.ctx().copy_text(error.to_owned());
            }
        });
        ui.label("Task completed. Worktrees are preserved where cleanup was unsafe or failed. Cleanup retries automatically every minute while this project is open.");
    }
    let failure = s.implementation_failure(ticket).or_else(|| {
        record
            .filter(|r| r.status == ImplementationStatus::Blocked)
            .map(|r| r.detail.as_str())
    });
    let interrupted = record.is_some_and(|r| {
        matches!(
            r.status,
            ImplementationStatus::Preparing
                | ImplementationStatus::Implementing
                | ImplementationStatus::Verifying
                | ImplementationStatus::Interrupted
                | ImplementationStatus::WaitingToMerge
                | ImplementationStatus::Publishing
                | ImplementationStatus::ReadyToPublish
        )
    });
    if let Some(error) = failure {
        ui.colored_label(theme::WARNING, "Needs attention");
        let summary = failure_summary(error);
        ui.label(
            egui::RichText::new(crate::core::context_build::clip(&summary, 140))
                .size(13.0)
                .color(theme::TEXT_DIM),
        )
        .on_hover_text(error);
    } else if interrupted {
        ui.colored_label(theme::DANGER, "Interrupted — no worker is running");
        ui.label("Resume implementation to continue preserved work.");
    }
}

pub(crate) fn card_summary(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 160 {
        flat
    } else {
        format!("{}…", flat.chars().take(160).collect::<String>())
    }
}

pub(crate) fn failure_summary(text: &str) -> String {
    text.split_once("### Next action(s)")
        .and_then(|(_, next)| next.lines().find(|line| !line.trim().is_empty()))
        .map(|line| {
            format!(
                "Next: {}",
                card_summary(line.trim().trim_start_matches("- "))
            )
        })
        .unwrap_or_else(|| card_summary(text))
}
