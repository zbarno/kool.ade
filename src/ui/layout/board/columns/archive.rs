pub(super) fn paint(
    ui: &mut egui::Ui,
    board: &crate::ui::planning_board::ViewModel,
    work: &[crate::core::planning_work::Work],
    planning_selection: &mut Option<String>,
) {
    let roots = work
        .iter()
        .filter(|item| {
            item.task_intent_root_uid() == Some(item.uid.as_str())
                && board.is_task_intent_archived(&item.uid)
        })
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return;
    }
    ui.add_space(12.0);
    egui::CollapsingHeader::new(format!("Archived TaskIntents · {}", roots.len()))
        .id_salt("archived_task_intents")
        .show(ui, |ui| {
            ui.label(crate::ui::theme::helper_text(
                "Archived requests keep their work map and history. Open one to restore it or start a new cycle.",
            ));
            for root in roots {
                if ui
                    .button(format!(
                        "{} · Archived",
                        root.title
                    ))
                    .clicked()
                {
                    *planning_selection = Some(root.key.clone());
                }
            }
        });
}
