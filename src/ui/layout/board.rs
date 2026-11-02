use super::*;

mod cancellation;
mod columns;
mod empty_lane;
mod lifecycle_summary;
mod opportunities;
mod planning_modal;
pub(super) mod presentation;
mod relationships;
mod task_chat;

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
    let relationship_map = relationships::build(board);
    let task_paths = relationships::unique_task_paths(board);
    let task_titles = board
        .task_documents
        .iter()
        .map(|doc| (doc.path.clone(), doc.title.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let dependents = relationships::task_dependents(board);
    let task_children = relationships::task_children(board);
    let planning_work_relations = relationships::planning_work_relations(&board.planning_work);
    let planning_work_keys = relationships::unique_planning_work_keys(&board.planning_work);
    let feature_uids_by_id = relationships::unique_feature_uids_by_id(&board.planning_work);
    let planning_item_relations = relationships::planning_item_relations(
        &board.planning_items,
        &feature_uids_by_id,
        |item| {
            crate::core::task_conversation::answer_receipt::is_saved(
                item,
                s.task_messages(item.conversation_key()),
            )
        },
    );
    let planning_item_keys = relationships::unique_planning_item_keys(&board.planning_items);
    let task_uids_by_path = relationships::unique_task_uids_by_path(board);
    let opportunity_relations = relationships::opportunity_relations(board);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_board_relationship_map"),
            relationship_map,
        );
        data.insert_temp(
            egui::Id::new("koolade_board_hover_next"),
            std::collections::HashSet::<String>::new(),
        );
        data.insert_temp(egui::Id::new("koolade_task_paths_by_uid"), task_paths);
        data.insert_temp(egui::Id::new("koolade_task_titles_by_path"), task_titles);
        data.insert_temp(egui::Id::new("koolade_task_dependents"), dependents);
        data.insert_temp(egui::Id::new("koolade_task_children"), task_children);
        data.insert_temp(
            egui::Id::new("koolade_task_planning_work_relations"),
            planning_work_relations,
        );
        data.insert_temp(
            egui::Id::new("koolade_task_planning_work_keys"),
            planning_work_keys,
        );
        data.insert_temp(
            egui::Id::new("koolade_task_planning_item_relations"),
            planning_item_relations,
        );
        data.insert_temp(
            egui::Id::new("koolade_task_planning_item_keys"),
            planning_item_keys,
        );
        data.insert_temp(
            egui::Id::new("koolade_task_uids_by_path"),
            task_uids_by_path,
        );
        data.insert_temp(
            egui::Id::new("koolade_task_opportunity_relations"),
            opportunity_relations,
        );
        if let Some(error) = &board.opportunity_error {
            data.insert_temp(
                egui::Id::new("koolade_task_opportunity_error"),
                error.clone(),
            );
        } else {
            data.remove::<String>(egui::Id::new("koolade_task_opportunity_error"));
        }
    });
    if let Some(error) = board.lifecycle_error.as_deref() {
        ui.colored_label(
            theme::WARNING,
            format!("Lifecycle card status unavailable or incomplete: {error}"),
        );
    }
    opportunities::paint(ui, s, board, &mut planning_selection);
    lifecycle_summary::paint(ui, board);
    columns::paint(ui, s, board, &mut selected_path, &mut planning_selection);

    // Opening or switching a task is an interaction, not a rendering side
    // effect. Emit one typed initialization command on the selection change,
    // before the modal reads its conversation in the same frame.
    task_chat::prepare_for_selection(
        ui,
        s,
        board,
        selected_path.as_deref(),
        planning_selection.as_deref(),
    );
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
            let navigation = ui.ctx().data_mut(|data| {
                data.remove_temp::<String>(egui::Id::new("koolade_task_relationship_navigation"))
            });
            if let Some(path) = navigation {
                selected_path = Some(path);
            } else if closed {
                selected_path = None;
                ui.ctx().data_mut(|data| {
                    data.remove::<Vec<String>>(egui::Id::new("koolade_task_relationship_history"));
                });
            }
        } else {
            selected_path = None;
        }
        let planning_navigation = ui.ctx().data_mut(|data| {
            data.remove_temp::<String>(egui::Id::new(
                "koolade_task_relationship_planning_navigation",
            ))
        });
        let navigated_to_planning = planning_navigation.is_some();
        if let Some(key) = planning_navigation {
            selected_path = None;
            planning_selection = Some(key);
            ui.ctx().request_repaint();
        }
        if !navigated_to_planning {
            planning_modal::paint(ui, s, board, &mut planning_selection, panel_bounds);
        }
        let task_navigation = ui.ctx().data_mut(|data| {
            data.remove_temp::<String>(egui::Id::new("koolade_task_relationship_task_navigation"))
        });
        if let Some(path) = task_navigation {
            planning_selection = None;
            selected_path = Some(path);
        }
    }
    if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        selected_path = None;
        ui.ctx().data_mut(|data| {
            data.remove::<Vec<String>>(egui::Id::new("koolade_task_relationship_history"));
            data.remove::<String>(egui::Id::new("koolade_task_relationship_work_return"));
        });
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
        ui.ctx().data_mut(|data| {
            data.remove::<String>(egui::Id::new("koolade_task_relationship_work_return"));
        });
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
