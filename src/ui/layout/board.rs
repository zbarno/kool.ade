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
    // Board work items carry conversation, state, approvals, and history. Give
    // their shared modal shell workspace proportions on desktop, while keeping
    // a small gutter on laptop and narrow viewports.
    let panel_bounds = egui::Rect::from_center_size(
        viewport.center(),
        egui::vec2(
            (viewport.width() - 24.0).clamp(320.0, 1440.0),
            viewport.height(),
        ),
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
            let closed = crate::ui::overlays::show_task_modal(
                ui,
                item.conversation_key(),
                panel_bounds,
                |ui| {
                    task_details::paint_planning_item(
                        ui,
                        s,
                        board,
                        item,
                        (panel_bounds.height() - 160.0).max(160.0),
                    );
                },
            );
            if closed {
                planning_selection = None;
            }
        } else if let Some(work) = planning_selection
            .as_ref()
            .and_then(|key| board.planning_work.iter().find(|work| &work.key == key))
        {
            let closed = crate::ui::overlays::show_task_modal(ui, &work.key, panel_bounds, |ui| {
                task_details::paint_planning_work(
                    ui,
                    s,
                    board,
                    work,
                    (panel_bounds.height() - 160.0).max(160.0),
                );
            });
            if closed {
                planning_selection = None;
            }
        } else if let Some(action) = planning_selection
            .as_deref()
            .and_then(|key| key.strip_prefix("feature-approval:"))
            .and_then(|id| {
                s.feature_actions(None)
                    .into_iter()
                    .find(|action| action.id == id)
            })
        {
            let id = action.id.clone();
            let closed = crate::ui::overlays::show_task_modal(ui, &id, panel_bounds, |ui| {
                task_details::paint_feature_approval(ui, s, &action);
            });
            if closed {
                planning_selection = None;
            }
        } else if let Some(issue) = board
            .setup_attention
            .as_ref()
            .filter(|issue| planning_selection.as_deref() == Some(issue.id))
        {
            let closed = crate::ui::overlays::show_task_modal(ui, issue.id, panel_bounds, |ui| {
                task_details::paint_setup_issue(ui, s, board, issue)
            });
            if closed {
                planning_selection = None;
            }
        } else {
            planning_selection = None;
        }
    }
    if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        selected_path = None;
        planning_selection = None;
        activity_path = None;
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
    if ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(egui::Id::new("koolade_workspace_settings_open")))
        == Some(true)
        && (selected_path.is_some() || planning_selection.is_some())
    {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_defer_settings_one_frame"), true);
        });
        selected_path = None;
        planning_selection = None;
        activity_path = None;
    }
    if ui.ctx().data_mut(|data| {
        data.remove_temp::<bool>(egui::Id::new("koolade_task_details_close"))
            .unwrap_or(false)
    }) {
        selected_path = None;
        planning_selection = None;
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
