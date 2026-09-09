//! Split conversation and document workspace with an optional item inspector.
use crate::ui::{Surface, theme};
use egui::{CentralPanel, Frame, Layout, Panel, RichText};

pub enum HeaderAction {
    Refresh,
    Import,
    Stakeholders,
    CopySpec,
    Disconnect,
}

pub fn paint(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let inspector_id = egui::Id::new("packet_inspector_visible");
    let mut inspector = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(inspector_id).unwrap_or(false));
    Panel::top("packet_header")
        .exact_size(58.0)
        .frame(
            Frame::NONE
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(22, 10)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                ui.label(RichText::new("Packet").size(21.0).strong());
                ui.add_space(18.0);
                ui.label(
                    RichText::new(s.session_title())
                        .size(14.0)
                        .color(theme::TEXT_DIM),
                );
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("Workspace", |ui| {
                        for (label, action) in [
                            ("Import references", HeaderAction::Import),
                            ("Stakeholders & ownership", HeaderAction::Stakeholders),
                            ("Refresh repository", HeaderAction::Refresh),
                            ("Disconnect", HeaderAction::Disconnect),
                        ] {
                            if ui.button(label).clicked() {
                                s.on_header_action(action);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .selectable_label(inspector, format!("Open items  {}", s.items_len()))
                        .clicked()
                    {
                        inspector = !inspector;
                    }
                    ui.add_space(12.0);
                    let label = if !s.is_git_repo() {
                        "No repository".to_owned()
                    } else {
                        format!(
                            "{}  ·  {}",
                            s.git_branch(),
                            if s.git_dirty() {
                                "Uncommitted changes"
                            } else {
                                "Saved to git"
                            }
                        )
                    };
                    ui.label(RichText::new(label).size(11.0).color(theme::TEXT_DIM));
                });
            });
        });
    ui.ctx()
        .data_mut(|d| d.insert_temp(inspector_id, inspector));
    if inspector {
        Panel::right("packet_items")
            .default_size(300.0)
            .min_size(260.0)
            .max_size(380.0)
            .resizable(true)
            .frame(Frame::NONE.fill(theme::BG).inner_margin(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Open items").size(17.0).strong());
                });
                ui.add_space(16.0);
                crate::ui::items_pane::paint(
                    ui,
                    &crate::ui::items_pane::Args {
                        items: s.items(),
                        synthetic: s.synthetic_items(),
                        user: s.current_user(),
                        next_question_id: s.next_question_id(),
                    },
                );
            });
    }
    Panel::left("packet_chat")
        .default_size(390.0)
        .min_size(300.0)
        .max_size(560.0)
        .resizable(true)
        .frame(
            Frame::NONE
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(22, 18)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Conversation").size(17.0).strong());
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Planning partner").size(11.0).weak());
                });
            });
            ui.add_space(24.0);
            let msgs = s.chat_messages().to_vec();
            let progress = s.live_progress().cloned();
            let busy = s.is_busy();
            let offer = s.task_offer().cloned();
            let intent = crate::ui::chat_pane::paint(
                ui,
                &msgs,
                s.chat_draft(),
                busy,
                progress.as_ref(),
                offer.as_ref(),
            );
            if intent.send || intent.cancel || intent.generate_tasks {
                s.on_intent(&intent);
            }
        });
    CentralPanel::default()
        .frame(
            Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(28, 18)),
        )
        .show(ui, |ui| {
            let tab_id = egui::Id::new("packet_document_tab");
            let mut tasks_tab = ui
                .ctx()
                .data_mut(|d| d.get_temp::<bool>(tab_id).unwrap_or(false));
            if !s.task_documents().is_empty() {
                ui.horizontal(|ui| {
                    if ui.selectable_label(!tasks_tab, "Specification").clicked() {
                        tasks_tab = false;
                    }
                    if ui
                        .selectable_label(
                            tasks_tab,
                            format!("Task stories  {}", s.task_documents().iter().filter(|d| !d.path.ends_with("/README.md")).count()),
                        )
                        .clicked()
                    {
                        tasks_tab = true;
                    }
                });
                ui.add_space(12.0);
            } else {
                tasks_tab = false;
            }
            ui.ctx().data_mut(|d| d.insert_temp(tab_id, tasks_tab));
            if tasks_tab {
                paint_tasks(ui, s.task_documents());
                return;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Specification").size(17.0).strong());
                if s.live_progress().is_some_and(|p| p.specification.is_some()) {
                    theme::badge(ui, "Live draft", theme::ACCENT_SOFT, theme::ACCENT)
                        .on_hover_text("Updates as the planner writes. Saved after validation.");
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Copy")
                        .on_hover_text("Copy specification as Markdown")
                        .clicked()
                    {
                        s.on_header_action(HeaderAction::CopySpec);
                    }
                    ui.label(
                        RichText::new(format!("{} words", s.spec_words()))
                            .size(11.0)
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
                    let inset = ((ui.available_width() - 780.0) / 2.0).max(0.0);
                    Frame::NONE
                        .inner_margin(egui::Margin::symmetric(inset as i8, 8))
                        .show(ui, |ui| {
                            crate::ui::spec_viewer::render(ui, Some(s.spec_text()));
                            ui.add_space(48.0);
                        });
                });
        });
}

fn paint_tasks(ui: &mut egui::Ui, docs: &[crate::artifacts::task_docs::TaskDocument]) {
    if let Some(progress) = docs.iter().find(|d| d.path.ends_with("/README.md")) {
        ui.label(RichText::new(&progress.title).weak());
        ui.add_space(8.0);
    }
    let id = egui::Id::new("packet_selected_task");
    let selected_path = ui.ctx().data_mut(|d| d.get_temp::<String>(id));
    let mut selected = selected_path.as_ref()
        .and_then(|path| docs.iter().position(|doc| &doc.path == path))
        .unwrap_or(0);
    egui::ComboBox::from_id_salt(id)
        .selected_text(&docs[selected].title)
        .width(ui.available_width().min(620.0))
        .show_ui(ui, |ui| {
            for (i, doc) in docs.iter().enumerate() {
                ui.selectable_value(&mut selected, i, &doc.title);
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(id, docs[selected].path.clone()));
    ui.label(RichText::new(&docs[selected].path).size(11.0).weak());
    ui.add_space(18.0);
    egui::ScrollArea::vertical()
        .id_salt(("task_document", &docs[selected].path))
        .show(ui, |ui| {
            crate::ui::spec_viewer::render(ui, Some(&docs[selected].text));
        });
}
