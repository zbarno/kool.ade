use super::super::*;

mod attention;
mod cards;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    selected_path: &mut Option<String>,
    planning_selection: &mut Option<String>,
) {
    let docs = &board.task_documents;
    let work = &board.planning_work;
    let items = &board.planning_items;
    let feature_approvals = s
        .feature_actions(None)
        .into_iter()
        .filter(|action| !action.approved)
        .collect::<Vec<_>>();
    let height = (ui.available_height() - 46.0).max(100.0);
    let gaps = 16.0 * 4.0;
    let column_width = ((ui.available_width() - 150.0 - gaps) / 5.0).max(180.0);
    egui::ScrollArea::horizontal()
        .id_salt("task_board_horizontal")
        .scroll_bar_visibility(if ui.available_width() < 1114.0 {
            egui::scroll_area::ScrollBarVisibility::AlwaysVisible
        } else {
            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
        })
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for (column, label) in crate::core::implementation::BOARD_COLUMNS
                    .iter()
                    .enumerate()
                {
                    let planning = work
                        .iter()
                        .filter(|item| item.board_column() == column
                            && !board.is_archived(&item.key)
                            && !board.cancelled.contains(&crate::persistence::cancelled_work::planning_id(&item.uid)))
                        .collect::<Vec<_>>();
                    let cards = docs
                        .iter()
                        .filter(|doc| {
                            !doc.path.ends_with("/README.md")
                                && !board.is_archived(&doc.path)
                                && !board.cancelled.contains(&crate::persistence::cancelled_work::task_id(doc))
                                && task_board_column(s, &doc.path) == column
                        })
                        .collect::<Vec<_>>();
                    let questions = items
                        .iter()
                        .filter(|item| !board.is_archived(item.conversation_key()))
                        .filter(|item| {
                            let base = planning_column(item, items);
                            let displayed =
                                if crate::core::routing::has_open_prerequisite(items, item) {
                                    base
                                } else {
                                    crate::ui::task_chat::board_column(
                                        if s.activity_active(item.conversation_key()) {
                                            1
                                        } else {
                                            base
                                        },
                                        s.task_messages(item.conversation_key()),
                                        s.task_chat_active(item.conversation_key()),
                                    )
                                };
                            displayed == column
                        })
                        .collect::<Vec<_>>();
                    egui::Frame::NONE
                        .fill(theme::COLUMN)
                        .stroke(egui::Stroke::new(1.0, theme::BORDER))
                        .corner_radius(8)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(column_width);
                                ui.set_min_height(height);
                                ui.horizontal(|ui| {
                                    let count = cards.len()
                                        + questions.len()
                                        + planning.len()
                                        + usize::from(column == 3) * feature_approvals.len()
                                        + usize::from(
                                            column == 3 && board.setup_attention.is_some(),
                                        );
                                    let lane_color = theme::TEXT_DIM;
                                    let (dot, _) = ui.allocate_exact_size(
                                        egui::vec2(7.0, 7.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(
                                        dot.center(),
                                        6.0,
                                        lane_color.gamma_multiply(0.2),
                                    );
                                    ui.painter().circle_filled(dot.center(), 3.5, lane_color);
                                    ui.label(
                                        RichText::new(*label)
                                            .size(15.0)
                                            .strong()
                                            .color(theme::TEXT),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            theme::badge(
                                                ui,
                                                &format!("{count}"),
                                                lane_color.gamma_multiply(0.12),
                                                lane_color,
                                            );
                                        },
                                    );
                                });
                                let subtitle = ui.label(
                                    RichText::new(match column {
                                        0 => "Ready for the next step",
                                        1 => "Ideas becoming reality",
                                        2 => "A fresh pair of eyes",
                                        3 => "Your input moves things forward",
                                        _ => "Look what you made happen",
                                    })
                                    .size(11.0)
                                    .color(theme::TEXT_MUTED),
                                );
                                if column == 3 {
                                    subtitle.on_hover_text(
                                        "Most recent task or conversation activity appears first. Cards without a recorded time keep a stable order.",
                                    );
                                }
                                let (line, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 2.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    line,
                                    1,
                                    theme::BORDER.gamma_multiply(0.65),
                                );
                                let visible_count = cards.len()
                                    + questions.len()
                                    + planning.len()
                                    + usize::from(column == 3) * feature_approvals.len()
                                    + usize::from(column == 3 && board.setup_attention.is_some());
                                if visible_count == 0 {
                                    let (headline, detail) = match column {
                                        0 => (
                                            "Nothing queued",
                                            "New planning work will appear here.",
                                        ),
                                        1 => (
                                            "No active work",
                                            "Workers and active conversations appear here.",
                                        ),
                                        2 => (
                                            "Nothing in review",
                                            "Completed checks await their next review step here.",
                                        ),
                                        3 => (
                                            "All clear",
                                            "Items needing a person will appear here.",
                                        ),
                                        _ => {
                                            ("Nothing completed yet", "Finished work appears here.")
                                        }
                                    };
                                    ui.add_space(((height - 160.0) * 0.4).max(12.0));
                                    super::empty_lane::paint(ui, column, headline, detail);
                                }
                                egui::ScrollArea::vertical()
                                    .id_salt(("task_board_column", column))
                                    .max_height(height - 76.0)
                                    .show(ui, |ui| {
                                        if column == 3 {
                                            attention::paint(
                                                ui,
                                                s,
                                                board,
                                                attention::Lane {
                                                    planning: &planning,
                                                    issue: board.setup_attention.as_ref(),
                                                    items: &questions,
                                                    tasks: &cards,
                                                },
                                                selected_path,
                                                planning_selection,
                                            );
                                            for action in &feature_approvals {
                                                cards::approval(ui, action, planning_selection);
                                            }
                                        } else {
                                            for item in &planning {
                                                cards::work(ui, s, item, &planning, column, planning_selection);
                                            }
                                            for item in &questions {
                                                cards::item(
                                                    ui,
                                                    s,
                                                    board,
                                                    item,
                                                    column,
                                                    planning_selection,
                                                );
                                            }
                                            for doc in &cards {
                                                cards::task(
                                                    ui,
                                                    s,
                                                    board,
                                                    doc,
                                                    column,
                                                    selected_path,
                                                );
                                            }
                                        }
                                    });
                            });
                        });
                }
            });
        });
}
