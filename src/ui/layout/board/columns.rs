use super::super::*;

mod archive;
mod attention;
mod cards;
mod header;
mod task_intents;
mod visibility;

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
        .filter(|action| {
            action
                .plan_snapshot
                .as_ref()
                .is_none_or(|snapshot| board.feature_visible(&snapshot.feature_uid))
        })
        .collect::<Vec<_>>();
    let plan_reviews = board
        .lifecycle_features
        .iter()
        .filter(|feature| feature.phase == crate::core::lifecycle::FeaturePhase::PlanReviewPending)
        .filter(|feature| board.feature_visible(&feature.feature_uid))
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
                        .filter(|item| {
                            visibility::work_column(board, item) == column
                                && visibility::visible(
                                    board,
                                    &item.key,
                                    item.task_intent_root_uid(),
                                )
                        })
                        .collect::<Vec<_>>();
                    let pr_work = board
                        .lifecycle_pr_work
                        .iter()
                        .filter(|task| cards::pr_task_column(task.preparation.status) == column)
                        .filter(|task| {
                            task_intents::pr_owner(board, task)
                                .is_none_or(|root| !board.is_task_intent_archived(&root.uid))
                        })
                        .collect::<Vec<_>>();
                    let cards = docs
                        .iter()
                        .filter(|doc| {
                            !doc.path.ends_with("/README.md")
                                && visibility::visible(
                                    board,
                                    &doc.path,
                                    crate::artifacts::task_docs::task_intent_uid_for_document(
                                        doc, docs, work,
                                    )
                                    .as_deref(),
                                )
                                && cards::task_card_column(s, board, doc) == column
                        })
                        .collect::<Vec<_>>();
                    let questions = items
                        .iter()
                        .filter(|item| {
                            visibility::visible(
                                board,
                                item.conversation_key(),
                                item.task_intent_uid.as_deref(),
                            )
                        })
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
                    let clarifications = if matches!(column, 3 | 4) {
                        &board.clarification_questions[..]
                    } else {
                        &[]
                    };
                    let visible_count = cards.len()
                        + pr_work.len()
                        + questions.len()
                        + planning.len()
                        + clarifications
                            .iter()
                            .filter(|question| board.question_visible(question) && crate::ui::planning_board::question_column(question.status) == column)
                            .count()
                        + usize::from(column == 2) * plan_reviews.len()
                        + usize::from(column == 3) * feature_approvals.len()
                        + if column == 3 {
                            board
                                .setup_tasks
                                .iter()
                                .filter(|task| task.is_active())
                                .count()
                                + board.setup_attention.len()
                        } else {
                            0
                        };
                    egui::Frame::NONE
                        .fill(theme::COLUMN)
                        .stroke(egui::Stroke::new(1.0, theme::BORDER))
                        .corner_radius(8)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(column_width);
                                ui.set_min_height(height);
                                header::paint(ui, column, label, visible_count);
                                if visible_count == 0 {
                                    let (headline, detail) = header::empty_message(column);
                                    ui.add_space(((height - 160.0) * 0.4).max(12.0));
                                    super::empty_lane::paint(ui, column, headline, detail);
                                }
                                egui::ScrollArea::vertical()
                                    .id_salt(("task_board_column", column))
                                    .max_height(height - 76.0)
                                    .show(ui, |ui| {
                                        for task in &pr_work {
                                            if task_intents::pr_owner(board, task).is_none() {
                                                cards::pr_task(ui, task, column, selected_path);
                                            }
                                        }
                                        if column == 3 {
                                            let (grouped_work, grouped_tasks, grouped_items) =
                                                task_intents::paint(
                                                    ui,
                                                    task_intents::Lane {
                                                        surface: s,
                                                        board,
                                                        planning: &planning,
                                                        tasks: &cards,
                                                        items: &questions,
                                                        column,
                                                        selected_path,
                                                        planning_selection,
                                                    },
                                                );
                                            let remaining_planning = planning
                                                .iter()
                                                .copied()
                                                .filter(|work| !grouped_work.contains(&work.uid))
                                                .collect::<Vec<_>>();
                                            let remaining_tasks = cards
                                                .iter()
                                                .copied()
                                                .filter(|task| !grouped_tasks.contains(&task.path))
                                                .collect::<Vec<_>>();
                                            let remaining_questions = questions
                                                .iter()
                                                .copied()
                                                .filter(|item| !grouped_items.contains(&item.id))
                                                .collect::<Vec<_>>();
                                            attention::paint(
                                                ui,
                                                s,
                                                board,
                                                attention::Lane {
                                                    planning: &remaining_planning,
                                                    issues: &board.setup_attention,
                                                    setup_tasks: &board.setup_tasks,
                                                    items: &remaining_questions,
                                                    tasks: &remaining_tasks,
                                                    clarifications,
                                                    clarification_error: board
                                                        .clarification_error
                                                        .as_deref(),
                                                },
                                                selected_path,
                                                planning_selection,
                                            );
                                            for action in &feature_approvals {
                                                if !task_intents::approval_owned(board, action) {
                                                    cards::approval(ui, action, planning_selection);
                                                }
                                            }
                                        } else {
                                            let (grouped_work, grouped_tasks, grouped_items) =
                                                task_intents::paint(
                                                    ui,
                                                    task_intents::Lane {
                                                        surface: s,
                                                        board,
                                                        planning: &planning,
                                                        tasks: &cards,
                                                        items: &questions,
                                                        column,
                                                        selected_path,
                                                        planning_selection,
                                                    },
                                                );
                                            if column == 2 {
                                                for feature in
                                                    plan_reviews.iter().filter(|feature| {
                                                        !task_intents::review_owned(board, feature)
                                                    })
                                                {
                                                    cards::plan_review(
                                                        ui,
                                                        feature,
                                                        planning_selection,
                                                    );
                                                }
                                            }
                                            if column == 4 {
                                                for question in clarifications.iter().filter(|question| board.question_visible(question)
                                                    && crate::ui::planning_board::question_column(question.status) == 4
                                                    && !task_intents::clarification_owned(board, question)) {
                                                    cards::clarification(ui, question, column, planning_selection);
                                                }
                                            }
                                            for item in &planning {
                                                if !grouped_work.contains(&item.uid) {
                                                    cards::work(
                                                        ui,
                                                        s,
                                                        item,
                                                        board,
                                                        column,
                                                        planning_selection,
                                                    );
                                                }
                                            }
                                            for item in &questions {
                                                if !grouped_items.contains(&item.id) {
                                                    cards::item(
                                                        ui,
                                                        s,
                                                        board,
                                                        item,
                                                        column,
                                                        planning_selection,
                                                    );
                                                }
                                            }
                                            for doc in &cards {
                                                if !grouped_tasks.contains(&doc.path) {
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
                                        }
                                    });
                            });
                        });
                }
            });
        });
    archive::paint(ui, board, work, planning_selection);
}
