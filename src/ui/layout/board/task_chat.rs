use crate::ui::{ApplicationCommand, Surface, planning_board::ViewModel};

pub(super) fn prepare_for_selection(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &ViewModel,
    selected_path: Option<&str>,
    planning_selection: Option<&str>,
) {
    let selected_chat = selected_path
        .filter(|path| board.task_documents.iter().any(|doc| doc.path == *path))
        .map(str::to_owned)
        .or_else(|| {
            let selected = planning_selection?;
            board
                .planning_items
                .iter()
                .find(|item| item.id == selected)
                .map(|item| item.conversation_key().to_owned())
                .or_else(|| {
                    board
                        .planning_work
                        .iter()
                        .find(|work| work.key == selected)
                        .map(|work| work.key.clone())
                })
                .or_else(|| {
                    board
                        .clarification_questions
                        .iter()
                        .find(|question| {
                            selected == super::planning_modal::clarification_key(&question.uid)
                        })
                        .map(|question| super::planning_modal::clarification_key(&question.uid))
                })
                .or_else(|| {
                    board
                        .opportunities
                        .iter()
                        .find(|(proposal, _)| {
                            selected
                                == crate::core::task_conversation::opportunity_conversation_key(
                                    &proposal.opportunity_uid,
                                )
                        })
                        .map(|(proposal, _)| {
                            crate::core::task_conversation::opportunity_conversation_key(
                                &proposal.opportunity_uid,
                            )
                        })
                })
        });
    let prepared_id = egui::Id::new("koolade_last_prepared_task_chat");
    let previously_prepared = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(prepared_id));
    if selected_chat != previously_prepared {
        ui.ctx().data_mut(|data| {
            if let Some(key) = selected_chat.as_ref() {
                data.insert_temp(prepared_id, key.clone());
            } else {
                data.remove::<String>(prepared_id);
            }
        });
        if let Some(key) = selected_chat {
            surface.dispatch(ApplicationCommand::PrepareTaskChat { key });
        }
    }
}
