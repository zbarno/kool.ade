mod cycles;
mod details;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: Option<&mut dyn crate::ui::Surface>,
    board: &crate::ui::planning_board::ViewModel,
    work: &crate::core::planning_work::Work,
) {
    let cancelled = board
        .cancelled
        .contains(&crate::persistence::cancelled_work::planning_id(&work.uid));
    let Some(intent) = crate::core::task_intent::TaskIntent::project_with_cancelled(
        work,
        &[],
        None,
        &[],
        None,
        cancelled,
    ) else {
        return;
    };
    cycles::paint(ui, surface, board, work);
    let planning_children = board
        .planning_work
        .iter()
        .filter(|child| {
            child.uid != intent.uid && child.task_intent_uid.as_deref() == Some(intent.uid)
        })
        .collect::<Vec<_>>();
    let task_documents = board
        .task_documents
        .iter()
        .filter(|document| {
            crate::artifacts::task_docs::task_intent_uid_for_document(
                document,
                &board.task_documents,
                &board.planning_work,
            )
            .as_deref()
                == Some(intent.uid)
        })
        .collect::<Vec<_>>();
    let current_cycle = work.task_intent_cycles.last();
    let current_cycle_uid = current_cycle.map(|cycle| cycle.uid.as_str());
    let decision_items = board
        .planning_items
        .iter()
        .filter(|item| {
            item.task_intent_uid.as_deref() == Some(intent.uid)
                && current_cycle_uid.is_none_or(|cycle_uid| {
                    item.task_intent_cycle_uid.as_deref() == Some(cycle_uid)
                        || (work.task_intent_cycles.len() == 1
                            && item.task_intent_cycle_uid.is_none())
                })
        })
        .collect::<Vec<_>>();
    let lifecycle = board.task_intent_lifecycle(work);

    ui.horizontal_wrapped(|ui| {
        crate::ui::theme::badge(
            ui,
            "TaskIntent",
            crate::ui::theme::PANEL,
            crate::ui::theme::TEXT_DIM,
        );
        crate::ui::theme::badge(
            ui,
            lifecycle.label(),
            crate::ui::theme::PANEL_ALT,
            crate::ui::theme::TEXT,
        );
    });
    ui.add_space(crate::ui::theme::spacing::S);
    ui.label(crate::ui::theme::section_heading("Original goal"));
    ui.add(egui::Label::new(intent.original_goal).wrap());
    ui.add_space(crate::ui::theme::spacing::S);
    ui.label(crate::ui::theme::section_heading(
        "What needs attention next",
    ));
    ui.label(lifecycle.next_action());
    if let Some(cycle) = current_cycle {
        ui.label(crate::ui::theme::helper_text(format!(
            "Current cycle {}: {}",
            cycle.sequence, cycle.goal
        )));
    }

    details::paint_task_map(
        ui,
        intent.uid,
        intent.conversation_key,
        &planning_children,
        &task_documents,
        &decision_items,
        board,
    );
    details::paint_history(ui, intent.uid, &work.task_intent_history);
}

#[cfg(test)]
#[path = "task_intent/tests.rs"]
mod tests;
