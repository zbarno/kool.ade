use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    compact: bool,
) {
    let has_task_conversation = chat_tabs::retain_planning_conversations(ui.ctx(), board);
    let specification_open = !ui.ctx().data_mut(|data| {
        data.get_temp::<bool>(egui::Id::new("koolade_document_tab"))
            .unwrap_or(true)
    });
    if has_task_conversation || specification_open {
        let chat_panel = if compact {
            Panel::top("koolade_chat_panel").exact_size(if specification_open {
                300.0
            } else {
                230.0
            })
        } else {
            Panel::left("koolade_chat_panel")
                .exact_size((ui.available_width() * 0.32).clamp(340.0, 560.0))
        };
        chat_panel
            .frame(Frame::NONE.fill(theme::BG).inner_margin(12))
            .show(ui, |ui| {
                if specification_open {
                    ui.heading("Revise specification");
                    ui.label("Describe a change below. Kool.ad/e updates the specification through conversation.");
                    let messages = s.chat_messages().to_vec();
                    let busy = s.conversation_busy();
                    let progress = s.live_progress().cloned();
                    let mut intent = crate::ui::chat_pane::paint_specification(
                        ui, &messages, s.chat_draft(), busy, progress.as_ref());
                    if intent.send {
                        let feature = ui.ctx().data_mut(|data| {
                            (data.get_temp::<u8>(egui::Id::new("koolade_spec_document_view")) == Some(1))
                                .then(|| {
                                    data.get_temp::<Option<String>>(egui::Id::new("koolade_selected_feature"))
                                        .flatten()
                                        .or_else(|| data.get_persisted::<Option<String>>(egui::Id::new("koolade_selected_feature")).flatten())
                                })
                                .flatten()
                        });
                        let target = feature.map(|id| format!("feature {id}")).unwrap_or_else(|| "product".into());
                        let draft = s.chat_draft();
                        *draft = format!("Revise the {target} specification: {}", draft.trim());
                    }
                    s.dispatch(ApplicationCommand::UserIntent(std::mem::take(&mut intent)));
                } else {
                    chat_tabs::paint(ui, s, board);
                }
            });
    }
}
