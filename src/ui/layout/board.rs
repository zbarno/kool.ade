use super::*;

mod cancellation;
mod columns;
mod empty_lane;
pub(super) mod presentation;
mod relationships;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
) {
    let viewport = ui.ctx().content_rect();
    let panel_bounds = egui::Rect::from_center_size(
        viewport.center(),
        egui::vec2(viewport.width().min(900.0), viewport.height()),
    );
    let activity_id = egui::Id::new("koolade_task_activity");
    let mut activity_path = ui.ctx().data_mut(|d| d.get_temp::<String>(activity_id));
    let show_activity = activity_path.is_some();
    let id = egui::Id::new("koolade_selected_task");
    let mut selected_path = ui.ctx().data_mut(|d| d.get_temp::<String>(id));
    let docs = &board.task_documents;
    let mut planning_selection = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(egui::Id::new("koolade_selected_planning")));
    let items = &board.planning_items;
    let eligible = &board.eligible_item_ids;
    let relationship_map = relationships::build(board);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_board_relationship_map"),
            relationship_map,
        );
        data.insert_temp(
            egui::Id::new("koolade_board_hover_next"),
            std::collections::HashSet::<String>::new(),
        );
    });
    columns::paint(ui, s, board, &mut selected_path, &mut planning_selection);
    cancellation::confirm(ui, s, board);
    let active_hover = egui::Id::new("koolade_board_hover_active");
    let next_hover = egui::Id::new("koolade_board_hover_next");
    let previous = ui
        .ctx()
        .data_mut(|data| data.get_temp::<std::collections::HashSet<String>>(active_hover))
        .unwrap_or_default();
    let next = ui
        .ctx()
        .data_mut(|data| data.get_temp::<std::collections::HashSet<String>>(next_hover))
        .unwrap_or_default();
    if previous != next {
        ui.ctx().data_mut(|data| {
            if next.is_empty() {
                data.remove::<std::collections::HashSet<String>>(active_hover);
            } else {
                data.insert_temp(active_hover, next);
            }
            data.remove::<std::collections::HashSet<String>>(next_hover);
            data.remove::<relationships::Map>(egui::Id::new("koolade_board_relationship_map"));
        });
        ui.ctx().request_repaint();
    } else {
        ui.ctx().data_mut(|data| {
            data.remove::<std::collections::HashSet<String>>(next_hover);
            data.remove::<relationships::Map>(egui::Id::new("koolade_board_relationship_map"));
        });
    }
    super::task_cards::motion::departures(ui);
    if activity_path.is_none() {
        if let Some(selected) = selected_path
            .as_ref()
            .and_then(|path| docs.iter().position(|doc| &doc.path == path))
        {
            let closed = crate::ui::overlays::show_task_modal(
                ui,
                &docs[selected].path,
                panel_bounds,
                |ui| {
                    task_details::paint(
                        ui,
                        s,
                        &docs[selected],
                        (panel_bounds.height() - 160.0).max(160.0),
                        &mut activity_path,
                    );
                },
            );
            if closed {
                selected_path = None;
            }
        } else {
            selected_path = None;
        }
        if let Some(item) = planning_selection
            .as_ref()
            .and_then(|id| items.iter().find(|i| &i.id == id))
        {
            let closed = crate::ui::overlays::show_panel_modal(
                ui,
                &format!(
                    "Planning · {}",
                    if item.id.starts_with("ownership:") {
                        "Ownership assignment"
                    } else {
                        &item.id
                    }
                ),
                panel_bounds,
                |ui| {
                    ui.heading(&item.question);
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {}",
                            item.kind,
                            item.category,
                            crate::core::implementation::BOARD_COLUMNS
                                [crate::ui::task_chat::board_column(
                                    planning_column(item, &board.planning_items),
                                    s.task_messages(item.conversation_key()),
                                    s.task_chat_active(item.conversation_key())
                                )]
                        ))
                        .small()
                        .weak(),
                    );
                    if !item.reason.is_empty() {
                        ui.label(crate::core::context_build::clip(&item.reason, 200));
                    }
                    task_conversation(ui, s, board, item.conversation_key(), true);
                    ui.add_space(12.0);
                    ui.collapsing("Background & evidence", |ui| {
                        ui.label(format!(
                            "{} · {} · {:?}",
                            item.kind, item.priority, item.status
                        ));
                        ui.label(format!("Authority: {}", item.authority));
                        ui.label(format!("Category: {}", item.category));
                        ui.label(format!(
                            "Owner: {}",
                            item.assigned_to.as_deref().unwrap_or("Unassigned")
                        ));
                        ui.add(egui::Label::new(&item.reason).wrap());
                        if let Some(feature) = &item.feature_id {
                            ui.label(format!("Feature: {feature}"));
                        }
                        if !item.evidence.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("Evidence").strong());
                            ui.add(egui::Label::new(&item.evidence).wrap());
                        }
                        if !item.recommendation.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("Recommended next step").strong());
                            ui.add(egui::Label::new(&item.recommendation).wrap());
                        }
                    });
                    ui.collapsing("Activity", |ui| {
                        crate::ui::task_activity::graph(
                            ui,
                            &s.activity_samples(Some(item.conversation_key())),
                            s.activity_active(item.conversation_key()),
                            48.0,
                        );
                        if let Some(progress) = s.task_progress(&item.id) {
                            ui.separator();
                            ui.heading("Agent investigation");
                            if let Some(activity) = &progress.activity {
                                ui.add(egui::Label::new(activity).wrap());
                            }
                            ui.label(format!("Activity updates: {}", progress.telemetry.updates));
                            if !progress.response.trim().is_empty() {
                                ui.label(RichText::new("Latest result").strong());
                                egui::ScrollArea::vertical()
                                    .max_height(240.0)
                                    .show(ui, |ui| {
                                        crate::ui::markdown::paint(
                                            ui,
                                            &crate::core::context_build::clip(
                                                &progress.response,
                                                4000,
                                            ),
                                            crate::ui::markdown::CHAT,
                                        )
                                    });
                            }
                            if !progress.thoughts.trim().is_empty() {
                                ui.collapsing("Worker notes", |ui| {
                                    crate::ui::markdown::paint(
                                        ui,
                                        &crate::core::context_build::clip(&progress.thoughts, 4000),
                                        crate::ui::markdown::CHAT,
                                    );
                                });
                            }
                        }
                        if eligible.contains(&item.id) {
                            ui.label("This item is in your planning queue.");
                        }
                        if s.next_question_id() == Some(item.id.as_str()) {
                            ui.label("The project manager is asking about this item now.");
                        }
                    });
                },
            );
            if closed {
                planning_selection = None;
            }
        } else {
            planning_selection = None;
        }
    }
    if let Some(ticket) = activity_path.clone().filter(|_| show_activity) {
        let bounds = ui.ctx().content_rect();
        let closed = crate::ui::overlays::show_panel_modal(
            ui,
            &format!("{} / All activity", task_key(&ticket)),
            bounds,
            |ui| {
                if let Some(doc) = docs.iter().find(|doc| doc.path == ticket) {
                    ui.heading(&doc.title);
                }
                if let Some(progress) = s.task_progress(&ticket) {
                    crate::ui::task_activity::full(ui, progress, s.implementation_active(&ticket));
                } else {
                    ui.label("No activity has been recorded yet.");
                }
            },
        );
        if closed {
            activity_path = None;
        }
    }
    ui.ctx().data_mut(|d| {
        if let Some(path) = activity_path {
            d.insert_temp(activity_id, path);
        } else {
            d.remove::<String>(activity_id);
        }
    });
    ui.ctx().data_mut(|d| {
        if let Some(path) = selected_path {
            d.insert_temp(id, path);
        } else {
            d.remove::<String>(id);
        }
        let id = egui::Id::new("koolade_selected_planning");
        if let Some(item) = planning_selection {
            d.insert_temp(id, item);
        } else {
            d.remove::<String>(id);
        }
    });
}
