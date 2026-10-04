use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    compact: bool,
) {
    CentralPanel::default()
        .frame(
            Frame::NONE
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(
                    if compact { 12 } else { 28 },
                    if compact { 4 } else { 18 },
                )),
        )
        .show(ui, |ui| {
            let tab_id = egui::Id::new("koolade_document_tab");
            let mut tasks_tab = ui
                .ctx()
                .data_mut(|d| d.get_temp::<bool>(tab_id).unwrap_or(true));
            {
                ui.horizontal(|ui| {
                    if ui.add(tab_button("Specification", !tasks_tab)).clicked() {
                        tasks_tab = false;
                    }
                    if ui
                        .add(tab_button(
                            format!(
                                "Board  {}",
                                board
                                    .task_documents
                                    .iter()
                                    .filter(|d| !d.path.ends_with("/README.md")
                                        && !board.is_archived(&d.path))
                                    .count()
                                    + board
                                        .planning_items
                                        .iter()
                                        .filter(|i| !board.is_archived(i.conversation_key()))
                                        .count()
                                    + board
                                        .planning_work
                                        .iter()
                                        .filter(|w| !board.is_archived(&w.key))
                                        .count()
                                    + usize::from(board.setup_attention.is_some())
                            ),
                            tasks_tab,
                        ))
                        .on_hover_ui(|ui| {
                            ui.set_max_width(320.0);
                            super::board::presentation::legend(ui);
                        })
                        .clicked()
                    {
                        tasks_tab = true;
                    }
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        new_task::trigger(ui);
                        if tasks_tab && ui.available_width() >= 440.0 {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 24.0),
                                Layout::left_to_right(egui::Align::Center),
                                super::board::presentation::legend,
                            );
                        } else if tasks_tab && ui.available_width() >= 65.0 {
                            ui.menu_button("Types", |ui| {
                                ui.set_min_width(300.0);
                                super::board::presentation::legend(ui);
                            });
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            ui.ctx().data_mut(|d| d.insert_temp(tab_id, tasks_tab));
            new_task::paint(ui, s);
            if tasks_tab {
                super::board::paint(ui, s, board);
                return;
            }
            let view_id = egui::Id::new("koolade_spec_document_view");
            let mut view = ui
                .ctx()
                .data_mut(|d| d.get_temp::<u8>(view_id))
                .unwrap_or(0);
            let features = s
                .active_features()
                .into_iter()
                .map(|(id, body)| (id.to_owned(), body.to_owned()))
                .collect::<Vec<_>>();
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(view == 0, "Product Specification")
                    .clicked()
                {
                    view = 0;
                }
                if !features.is_empty()
                    && ui
                        .selectable_label(view == 1, format!("Features  {}", features.len()))
                        .clicked()
                {
                    view = 1;
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(view_id, view));
            let selected_id = egui::Id::new("koolade_selected_feature");
            let mut selected = ui
                .ctx()
                .data_mut(|d| {
                    d.get_temp::<Option<String>>(selected_id)
                        .flatten()
                        .or_else(|| d.get_persisted::<Option<String>>(selected_id).flatten())
                })
                .filter(|id| features.iter().any(|(feature_id, _)| feature_id == id))
                .or_else(|| features.first().map(|(id, _)| id.clone()));
            if view == 1 && !features.is_empty() {
                egui::ComboBox::from_id_salt("feature_specification_selector")
                    .selected_text(selected.as_deref().unwrap_or("Select feature"))
                    .show_ui(ui, |ui| {
                        for (id, body) in &features {
                            let title = body
                                .lines()
                                .find_map(|line| line.strip_prefix("# "))
                                .unwrap_or(id);
                            ui.selectable_value(&mut selected, Some(id.clone()), title);
                        }
                    });
                if selected.as_deref().is_some_and(|id| s.feature_approved(id)) {
                    ui.label(RichText::new("Approved").color(theme::SUCCESS));
                }
            }
            ui.ctx().data_mut(|d| {
                d.insert_persisted(selected_id, selected.clone());
                d.insert_temp(selected_id, selected.clone());
            });
            let document = if view == 1 {
                selected
                    .as_deref()
                    .and_then(|selected| {
                        features
                            .iter()
                            .find(|(id, _)| id == selected)
                            .map(|(_, body)| body.clone())
                    })
                    .unwrap_or_else(|| s.spec_text().to_owned())
            } else {
                s.spec_text().to_owned()
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if view == 1 {
                        "Feature specification"
                    } else {
                        "Product specification"
                    })
                    .size(17.0)
                    .strong(),
                );
                if s.live_progress().is_some_and(|p| p.specification.is_some()) {
                    theme::badge(ui, "Live draft", theme::ACCENT_SOFT, theme::ACCENT)
                        .on_hover_text("Updates as the planner writes. Saved after validation.");
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Copy")
                        .on_hover_text("Copy the displayed specification as Markdown")
                        .clicked()
                    {
                        ui.ctx().copy_text(document.clone());
                    }
                    ui.label(
                        RichText::new(format!("{} words", document.split_whitespace().count()))
                            .size(12.5)
                            .weak(),
                    );
                });
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("LIVING DOCUMENT  /  Refined through conversation")
                    .size(10.0)
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(22.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    crate::ui::spec_viewer::render(ui, Some(&document));
                });
        });
}

fn tab_button(label: impl Into<String>, selected: bool) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(label.into())
            .size(14.0)
            .strong()
            .color(if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            }),
    )
    .min_size(egui::vec2(90.0, 38.0))
    .fill(if selected {
        egui::Color32::from_rgb(163, 12, 35)
    } else {
        theme::PANEL
    })
    .stroke(egui::Stroke::new(
        if selected { 2.0 } else { 1.0 },
        if selected {
            theme::PUNCH
        } else {
            theme::BORDER
        },
    ))
}
