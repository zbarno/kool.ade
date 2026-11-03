use crate::ui::{ApplicationCommand, Surface};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: Option<&mut dyn Surface>,
    board: &crate::ui::planning_board::ViewModel,
    root: &crate::core::planning_work::Work,
) {
    if root.task_intent_cycles.len() > 1 {
        egui::CollapsingHeader::new(format!(
            "Work cycle history · {} cycles",
            root.task_intent_cycles.len()
        ))
        .id_salt(("task_intent_cycles", root.uid.as_str()))
        .show(ui, |ui| {
            for cycle in root.task_intent_cycles.iter().rev() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Cycle {} · {}", cycle.sequence, cycle.goal));
                    if cycle.work_uid != root.uid
                        && let Some(work) = board
                            .planning_work
                            .iter()
                            .find(|work| work.uid == cycle.work_uid)
                        && ui.button("Open cycle workspace").clicked()
                    {
                        super::details::navigate_to_planning_child(ui, &root.key, &work.key);
                    }
                });
            }
        });
    }

    let draft_id = egui::Id::new(("task_intent_follow_up_goal", root.uid.as_str()));
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_default();
    ui.add_space(crate::ui::theme::spacing::S);
    ui.label(crate::ui::theme::section_heading("Start a follow-up cycle"));
    ui.add(
        egui::TextEdit::multiline(&mut draft)
            .desired_rows(2)
            .hint_text("Describe the new, explicitly scoped work"),
    );
    let valid = !draft.trim().is_empty()
        && draft.trim().len() <= crate::core::planning_work::MAX_TASK_INTENT_CYCLE_GOAL_BYTES;
    if ui
        .add_enabled(valid, egui::Button::new("Start follow-up cycle"))
        .clicked()
    {
        if let Some(surface) = surface {
            surface.dispatch(ApplicationCommand::StartTaskIntentFollowUp {
                root_uid: root.uid.clone(),
                goal: draft.trim().to_owned(),
            });
        }
        draft.clear();
    }
    if draft.trim().len() > crate::core::planning_work::MAX_TASK_INTENT_CYCLE_GOAL_BYTES {
        ui.label(crate::ui::theme::helper_text(format!(
            "Keep the follow-up under {} UTF-8 bytes.",
            crate::core::planning_work::MAX_TASK_INTENT_CYCLE_GOAL_BYTES
        )));
    }
    ui.ctx().data_mut(|data| data.insert_temp(draft_id, draft));
}
