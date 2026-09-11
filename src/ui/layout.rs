//! Split conversation and document workspace with an optional item inspector.
use crate::ui::{Surface, theme};
use egui::{CentralPanel, Frame, Layout, Panel, RichText};

pub enum HeaderAction {
    Refresh,
    Import,
    Stakeholders,
    McpServers,
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
                            ("MCP servers", HeaderAction::McpServers),
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
                        stakes: s.stakeholders(),
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
                            format!(
                                "Task stories  {}",
                                s.task_documents()
                                    .iter()
                                    .filter(|d| !d.path.ends_with("/README.md"))
                                    .count()
                            ),
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
                paint_tasks(ui, s);
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

fn paint_tasks(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let mut auto_mode = s.auto_mode();
    if ui.checkbox(&mut auto_mode, "Auto mode — merge verified tasks and continue the queue").on_hover_text("Enabled by default. Implement starts the queue. Disable to stop after the current task and use pull requests for future tasks.").changed() { s.set_auto_mode(auto_mode); }
    if !s.queue_status().is_empty() {
        ui.label(s.queue_status().lines().next().unwrap_or_default());
        if s.queue_status().contains('\n') {
            ui.collapsing("Queue recovery details", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.label(s.queue_status());
                    });
            });
        }
    }
    let docs = s.task_documents().to_vec();
    if let Some(progress) = docs.iter().find(|d| d.path.ends_with("/README.md")) {
        ui.label(RichText::new(&progress.title).weak());
        ui.add_space(8.0);
    }
    let id = egui::Id::new("packet_selected_task");
    let selected_path = ui.ctx().data_mut(|d| d.get_temp::<String>(id));
    if docs.is_empty() {
        ui.label("No task stories yet.");
        return;
    }
    let mut selected = selected_path
        .as_ref()
        .and_then(|path| docs.iter().position(|doc| &doc.path == path))
        .unwrap_or_else(|| {
            docs.iter()
                .position(|doc| !doc.path.ends_with("/README.md"))
                .unwrap_or(0)
        });
    ui.label(
        RichText::new("PR status refreshes every minute. Select a card to review its story.")
            .size(12.0)
            .weak(),
    );
    if let Some(index) = docs.iter().position(|doc| doc.path.ends_with("/README.md")) {
        if ui
            .selectable_label(selected == index, "Batch overview")
            .clicked()
        {
            selected = index;
        }
    }
    egui::ScrollArea::horizontal()
        .id_salt("task_board_horizontal")
        .max_height(285.0)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (column, label) in crate::core::implementation::BOARD_COLUMNS
                    .iter()
                    .enumerate()
                {
                    let cards = docs
                        .iter()
                        .enumerate()
                        .filter(|(_, doc)| {
                            !doc.path.ends_with("/README.md")
                                && crate::core::implementation::board_column(
                                    s.implementation_state(&doc.path),
                                    s.implementation_active(&doc.path),
                                ) == column
                        })
                        .collect::<Vec<_>>();
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(190.0);
                            ui.label(RichText::new(format!("{label} · {}", cards.len())).strong());
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .id_salt(("task_board_column", column))
                                .max_height(230.0)
                                .show(ui, |ui| {
                                    if cards.is_empty() {
                                        ui.label(RichText::new("No tasks").weak());
                                    }
                                    for (index, doc) in cards {
                                        if ui
                                            .add(
                                                egui::Button::new(&doc.title)
                                                    .selected(selected == index)
                                                    .wrap(),
                                            )
                                            .clicked()
                                        {
                                            selected = index;
                                        }
                                        if let Some(record) = s.implementation_state(&doc.path) {
                                            if let Some(url) = &record.pr_url {
                                                ui.hyperlink_to("Open PR", url);
                                            }
                                            if record.pr_check_error.is_some() {
                                                ui.label(
                                                    RichText::new(
                                                        "PR check failed; showing last known state",
                                                    )
                                                    .small(),
                                                );
                                            }
                                        }
                                        ui.add_space(6.0);
                                    }
                                });
                        });
                    });
                }
            });
        });
    ui.ctx()
        .data_mut(|d| d.insert_temp(id, docs[selected].path.clone()));
    ui.add_space(10.0);
    ui.label(RichText::new(&docs[selected].title).strong());
    ui.label(RichText::new(&docs[selected].path).size(11.0).weak());
    let ticket = &docs[selected].path;
    let state = s.implementation_state(ticket).cloned();
    if !ticket.ends_with("/README.md") {
        ui.horizontal(|ui| {
            if let Some(record) = &state {
                let status = if matches!(record.status.as_str(), "Preparing" | "Implementing" | "Verifying") && !s.implementation_active(ticket) { "Interrupted — ready to resume" } else { &record.status };
                ui.label(RichText::new(status).size(12.0).weak());
                if let Some(url) = &record.pr_url { ui.hyperlink_to("Open PR", url); }
            }
            let label = if state.is_some() { "Resume implementation" } else if s.auto_mode() { "Implement & continue queue" } else { "Implement" };
            if state.as_ref().is_none_or(|record| record.pr_url.is_none() && record.status != "Done") && ui.add_enabled(!s.is_busy(), egui::Button::new(label)).on_hover_text("Implement this ticket with Pi in a dedicated worktree, verify changes, then publish using the selected Auto or pull-request mode. Existing work is preserved on resume. New tasks fetch the latest remote base with fast-forward checks. Resume preserves the existing worktree.").clicked() {
                s.implement_task(ticket.clone());
            }
        });
        if let Some(record) = &state {
            ui.label(
                RichText::new(record.worktree.display().to_string())
                    .size(11.0)
                    .weak(),
            );
            if let Some(checked) = &record.pr_checked_at {
                ui.label(
                    RichText::new(format!("PR last checked: {checked}"))
                        .small()
                        .weak(),
                );
            }
            if let Some(error) = &record.pr_check_error {
                ui.label(format!("PR check failed: {error}"));
            }
            if record.pr_state.as_deref() == Some("CLOSED") {
                ui.label("PR closed without merging. Reopen the PR on GitHub to return this task to review.");
            }
            if record.status == "Needs attention" {
                ui.collapsing("Recovery details", |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            ui.label(&record.detail);
                        });
                });
            }
            if record.status == "Done" {
                if let Some(commit) = &record.merged_commit {
                    ui.label(format!(
                        "Merged into {} · {}",
                        record.base,
                        &commit[..commit.len().min(12)]
                    ));
                }
            }
        }
    }
    ui.add_space(12.0);
    egui::ScrollArea::vertical()
        .id_salt(("task_document", &docs[selected].path))
        .show(ui, |ui| {
            crate::ui::spec_viewer::render(ui, Some(&docs[selected].text));
        });
}
