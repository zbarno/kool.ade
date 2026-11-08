use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    compact: bool,
) {
    let _ = board;
    let specification_open = super::workspace_tabs::selected_tab(ui.ctx()) == 0;
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
                    ui.label(if s.has_published_product_spec() {
                        "Ask about the published specification, or propose a change to save a private draft."
                    } else {
                        "Start with an idea or review this codebase. Proposed product changes are saved privately; nothing is published from this conversation."
                    });
                    let mode_id = egui::Id::new("koolade_spec_chat_intent");
                    let default_mode = if s.has_published_product_spec() { 0 } else { 1 };
                    let mut mode = ui.ctx().data_mut(|data| data.get_temp::<u8>(mode_id).unwrap_or(default_mode));
                    ui.horizontal(|ui| {
                        if ui.selectable_label(mode == 0, "Ask").clicked() { mode = 0; }
                        if ui.selectable_label(mode == 1, "Propose change").clicked() { mode = 1; }
                    });
                    ui.ctx().data_mut(|data| data.insert_temp(mode_id, mode));
                    let messages = s.specification_chat_messages();
                    let busy = s.specification_chat_busy();
                    let progress = s.specification_chat_progress().cloned();
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
                        let scope = match feature {
                            Some(display_id) => s.active_features().into_iter()
                                .find(|(id, _)| *id == display_id.as_str())
                                .and_then(|(_, markdown)| crate::domain::ArtifactIdentity::from_markdown(markdown).ok().flatten())
                                .map(|identity| crate::core::specification_chat::Scope::Feature { display_id, uid: identity.uid }),
                            None => Some(crate::core::specification_chat::Scope::Product),
                        };
                        let Some(scope) = scope else {
                            s.toasts().warning("The selected feature identity is unavailable. Refresh the specification before sending.");
                            return;
                        };
                        let reviewed_repository_evidence = if mode == 1
                            && matches!(scope, crate::core::specification_chat::Scope::Product)
                        {
                            let review_id = ui.ctx().data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_baseline_evidence_review_id")).filter(|id| !id.is_empty()));
                            review_id.map(|id| s.reviewed_baseline_evidence(&id)).unwrap_or_default()
                        } else { Vec::new() };
                        match s.specification_chat_target(scope) {
                            Ok(target) => {
                                let text = s.chat_draft().clone();
                                s.dispatch(ApplicationCommand::SpecificationChat(
                                    crate::core::specification_chat::Request {
                                        text,
                                        intent: if mode == 1 {
                                            crate::core::specification_chat::Intent::Propose
                                        } else {
                                            crate::core::specification_chat::Intent::Ask
                                        },
                                        target,
                                        omitted_history_messages: 0,
                                        reviewed_repository_evidence,
                                    },
                                ));
                            }
                            Err(error) => s.toasts().warning(error),
                        }
                        return;
                    }
                    s.dispatch(ApplicationCommand::UserIntent(std::mem::take(&mut intent)));
                }
            });
    }
}
