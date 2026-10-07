use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    compact: bool,
) {
    let _ = board;
    let specification_open = !ui.ctx().data_mut(|data| {
        data.get_temp::<bool>(egui::Id::new("koolade_document_tab"))
            .unwrap_or(true)
    });
    let collapsed_id = egui::Id::new("koolade_spec_chat_collapsed");
    let collapsed = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(collapsed_id).unwrap_or(compact));
    if specification_open && !collapsed {
        let chat_panel = if compact {
            Panel::top("koolade_chat_panel")
                .exact_size(300.0)
                .resizable(true)
                .min_size(200.0)
                .max_size(440.0)
        } else {
            Panel::left("koolade_chat_panel")
                .default_size((ui.available_width() * 0.30).clamp(320.0, 460.0))
                .resizable(true)
                .min_size(280.0)
                .max_size(560.0)
        };
        chat_panel
            .frame(Frame::NONE.fill(theme::BG).inner_margin(12))
            .show(ui, |ui| {
                if specification_open {
                    ui.horizontal(|ui| {
                        ui.heading("Specification conversation");
                        if ui.small_button("Collapse").clicked() {
                            ui.ctx().data_mut(|data| data.insert_temp(collapsed_id, true));
                        }
                    });
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
                }
            });
    }
}
