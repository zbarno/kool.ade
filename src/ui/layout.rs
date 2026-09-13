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
    Panel::left("packet_chat")
        .default_size(330.0)
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
                    ui.label(RichText::new("Project manager").size(11.0).weak());
                });
            });
            ui.add_space(24.0);
            let msgs = s.chat_messages().to_vec();
            let progress = s.live_progress().cloned();
            let busy = s.conversation_busy();
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
                .data_mut(|d| d.get_temp::<bool>(tab_id).unwrap_or(true));
            {
                ui.horizontal(|ui| {
                    if ui.selectable_label(!tasks_tab, "Specification").clicked() {
                        tasks_tab = false;
                    }
                    if ui
                        .selectable_label(
                            tasks_tab,
                            format!(
                                "Board  {}",
                                s.task_documents()
                                    .iter()
                                    .filter(|d| !d.path.ends_with("/README.md"))
                                    .count()
                                    + s.items().len()
                                    + s.synthetic_items().len()
                            ),
                        )
                        .clicked()
                    {
                        tasks_tab = true;
                    }
                });
                ui.add_space(12.0);
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
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.set_max_width(880.0);
                        crate::ui::spec_viewer::render(ui, Some(s.spec_text()));
                    });
                });
        });
}

fn paint_tasks(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let panel_bounds = ui.max_rect();
    let activity_id = egui::Id::new("packet_task_activity");
    let mut activity_path = ui.ctx().data_mut(|d| d.get_temp::<String>(activity_id));
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
    let mut selected_path = ui.ctx().data_mut(|d| d.get_temp::<String>(id));
    let mut planning_selection = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(egui::Id::new("packet_selected_planning")));
    let mut items = s
        .items()
        .iter()
        .chain(s.synthetic_items())
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by_key(|item| (item.priority.rank(), item.id.clone()));
    items.dedup_by(|a, b| a.id == b.id);
    let eligible =
        crate::core::routing::eligible_items(s.items(), s.current_user(), s.stakeholders())
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>();
    ui.label(
        RichText::new(
            "Planning questions and task workers · select a card for details and activity",
        )
        .small()
        .weak(),
    );
    if let Some(doc) = docs.iter().find(|doc| doc.path.ends_with("/README.md")) {
        if ui.button("Batch overview").clicked() {
            selected_path = Some(doc.path.clone());
        }
    }
    let height = (ui.available_height() - 24.0).max(120.0);
    let column_width = ((ui.available_width() - 100.0) / 5.0).max(224.0);
    egui::ScrollArea::horizontal()
        .id_salt("task_board_horizontal")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (column, label) in crate::core::implementation::BOARD_COLUMNS
                    .iter()
                    .enumerate()
                {
                    let cards = docs
                        .iter()
                        .filter(|doc| {
                            !doc.path.ends_with("/README.md")
                                && crate::core::implementation::board_column(
                                    s.implementation_state(&doc.path),
                                    s.implementation_active(&doc.path),
                                ) == column
                        })
                        .collect::<Vec<_>>();
                    let questions = items
                        .iter()
                        .filter(|item| planning_column(item) == column)
                        .collect::<Vec<_>>();
                    egui::Frame::NONE.fill(theme::BG).corner_radius(8).inner_margin(10).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(column_width);
                            ui.set_min_height(height);
                            ui.label(
                                RichText::new(format!(
                                    "{label} · {}",
                                    cards.len() + questions.len()
                                ))
                                .strong(),
                            );
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .id_salt(("task_board_column", column))
                                .max_height(height - 32.0)
                                .show(ui, |ui| {
                                    for item in questions {
                                        board_card(ui, &item.id, false, |ui| {
                                            ui.label(RichText::new(format!("{} · {}", item.id, item.kind)).small().color(theme::ACCENT));
                                            if ui.add(egui::Button::new(RichText::new(item.summary()).strong()).frame(false).wrap()).clicked() {
                                                planning_selection = Some(item.id.clone());
                                            }
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(RichText::new(item.priority.to_string()).small().color(if item.priority == crate::domain::item::Priority::Blocking { theme::DANGER } else { theme::WARNING }));
                                                ui.label(RichText::new(item.assigned_to.as_deref().unwrap_or("Unassigned")).small().weak());
                                            });
                                            ui.label(RichText::new(&item.category).small().weak());
                                            if eligible.contains(&item.id) { ui.label(RichText::new("For you").small().color(theme::ACCENT)); }
                                            if s.next_question_id() == Some(item.id.as_str()) { ui.label("Asking now"); }
                                        });
                                    }
                                    for doc in cards {
                                        let active = s.implementation_active(&doc.path);
                                        board_card(ui, &doc.path, active, |ui| {
                                            ui.label(RichText::new(format!("{} · Task", task_key(&doc.path))).small().color(theme::ACCENT));
                                            if ui.add(egui::Button::new(RichText::new(&doc.title).strong()).frame(false).wrap()).clicked() {
                                                selected_path = Some(doc.path.clone());
                                            }
                                            let status = s.implementation_state(&doc.path).map(|r| r.status.as_str()).unwrap_or(if active { "Starting" } else { "To do" });
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(RichText::new(status).small().color(if active { theme::ACCENT } else { theme::TEXT_DIM }));
                                                ui.label(RichText::new(if active { "Assigned worker" } else { "Task worker" }).small().weak());
                                            });
                                            if let Some(progress) = s.task_progress(&doc.path) {
                                                if crate::ui::task_activity::compact(ui, progress, active) { activity_path = Some(doc.path.clone()); }
                                            } else if active { ui.spinner(); ui.label("Waiting for worker output…"); }
                                        });
                                    }
                                });
                        });
                    });
                }
            });
        });
    if activity_path.is_none() {
        if let Some(selected) = selected_path
            .as_ref()
            .and_then(|path| docs.iter().position(|doc| &doc.path == path))
        {
            let closed = crate::ui::overlays::show_panel_modal(
                ui,
                &format!("{} / Task details", task_key(&docs[selected].path)),
                panel_bounds,
                |ui| {
                    paint_task_details(
                        ui,
                        s,
                        &docs[selected],
                        (panel_bounds.height() - 160.0).max(160.0),
                        &mut activity_path,
                    );
                },
            );
            if closed {
                selected_path = None;
            }
        } else {
            selected_path = None;
        }
        if let Some(item) = planning_selection
            .as_ref()
            .and_then(|id| items.iter().find(|i| &i.id == id))
        {
            let mut discuss = false;
            let closed = crate::ui::overlays::show_panel_modal(
                ui,
                &format!("Planning · {}", item.id),
                panel_bounds,
                |ui| {
                    ui.heading(&item.question);
                    ui.label(format!(
                        "{} · {} · {:?}",
                        item.kind, item.priority, item.status
                    ));
                    ui.label(format!("Category: {}", item.category));
                    ui.label(format!(
                        "Owner: {}",
                        item.assigned_to.as_deref().unwrap_or("Unassigned")
                    ));
                    ui.label(&item.reason);
                    if eligible.contains(&item.id) {
                        ui.label("This item is in your planning queue.");
                    }
                    if s.next_question_id() == Some(item.id.as_str()) {
                        ui.label("The project manager is asking about this item now.");
                    }
                    let discussion = s
                        .chat_messages()
                        .iter()
                        .filter(|m| m.ref_item.as_deref() == Some(item.id.as_str()))
                        .collect::<Vec<_>>();
                    if !discussion.is_empty() {
                        ui.separator();
                        ui.heading("Planning discussion");
                        for message in discussion {
                            ui.label(&message.text);
                        }
                    }
                    discuss = ui.button("Discuss with project manager").clicked();
                    if item.is_ownership_gap() && ui.button("Assign ownership").clicked() {
                        s.on_header_action(HeaderAction::Stakeholders);
                    }
                },
            );
            if discuss {
                *s.chat_draft() = format!("Regarding {}: {}\n", item.id, item.question);
            }
            if closed || discuss {
                planning_selection = None;
            }
        } else {
            planning_selection = None;
        }
    }
    if let Some(ticket) = activity_path.clone() {
        let bounds = ui.ctx().content_rect();
        let closed = crate::ui::overlays::show_panel_modal(
            ui,
            &format!("{} / All activity", task_key(&ticket)),
            bounds,
            |ui| {
                if let Some(doc) = docs.iter().find(|doc| doc.path == ticket) {
                    ui.heading(&doc.title);
                }
                if let Some(progress) = s.task_progress(&ticket) {
                    crate::ui::task_activity::full(ui, progress, s.implementation_active(&ticket));
                } else {
                    ui.label("No activity has been recorded yet.");
                }
            },
        );
        if closed {
            activity_path = None;
        }
    }
    ui.ctx().data_mut(|d| {
        if let Some(path) = activity_path {
            d.insert_temp(activity_id, path);
        } else {
            d.remove::<String>(activity_id);
        }
    });
    ui.ctx().data_mut(|d| {
        if let Some(path) = selected_path {
            d.insert_temp(id, path);
        } else {
            d.remove::<String>(id);
        }
        let id = egui::Id::new("packet_selected_planning");
        if let Some(item) = planning_selection {
            d.insert_temp(id, item);
        } else {
            d.remove::<String>(id);
        }
    });
}

fn planning_column(item: &crate::domain::item::OpenItem) -> usize {
    if item.status == crate::domain::item::ItemStatus::Resolved {
        4
    } else if item.is_ownership_gap() || item.priority == crate::domain::item::Priority::Blocking {
        3
    } else {
        0
    }
}

fn paint_task_properties(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
) {
    ui.add_space(10.0);
    ui.label(RichText::new(&doc.path).size(11.0).weak());
    let ticket = &doc.path;
    let state = s.implementation_state(ticket).cloned();
    if !ticket.ends_with("/README.md") {
        ui.horizontal_wrapped(|ui| {
            if let Some(record) = &state {
                let status = if matches!(record.status.as_str(), "Preparing" | "Implementing" | "Verifying") && !s.implementation_active(ticket) { "Interrupted — ready to resume" } else { &record.status };
                ui.label(RichText::new(status).size(12.0).weak());
                if let Some(url) = &record.pr_url { ui.hyperlink_to("Open PR", url); }
            }
            let label = if state.is_some() { "Resume implementation" } else if s.auto_mode() { "Implement & continue queue" } else { "Implement" };
            if state.as_ref().is_none_or(|record| record.pr_url.is_none() && record.status != "Done") && !s.implementation_active(ticket) && ui.add_enabled(!s.is_busy(), egui::Button::new(label)).on_hover_text("Implement this ticket with Pi in a dedicated worktree, verify changes, then publish using the selected Auto or pull-request mode. Existing work is preserved on resume. New tasks fetch the latest remote base with fast-forward checks. Resume preserves the existing worktree.").clicked() {
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
    if s.implementation_active(ticket) && ui.button("Stop task and pause queue").clicked() {
        s.cancel_task();
    }
    if let Some(record) = &state {
        ui.collapsing("Implementation properties", |ui| {
            for (label, value) in [
                ("Branch", record.branch.as_str()),
                ("Base", record.base.as_str()),
                ("Base commit", record.base_commit.as_str()),
                (
                    "Verified commit",
                    record.verified_head.as_deref().unwrap_or("Not verified"),
                ),
                (
                    "Merge commit",
                    record.merged_commit.as_deref().unwrap_or("Not merged"),
                ),
                ("PR state", record.pr_state.as_deref().unwrap_or("No PR")),
                (
                    "Last PR attempt",
                    record
                        .pr_check_attempted_at
                        .as_deref()
                        .unwrap_or("Not checked"),
                ),
                (
                    "Publication",
                    if record.auto_merge {
                        "Automatic merge"
                    } else {
                        "Pull request"
                    },
                ),
            ] {
                ui.label(format!("{label}: {value}"));
            }
            ui.label(&record.detail);
        });
    }
}

fn board_card(ui: &mut egui::Ui, key: &str, active: bool, body: impl FnOnce(&mut egui::Ui)) {
    ui.push_id(key, |ui| {
        Frame::NONE
            .fill(theme::PANEL)
            .corner_radius(6)
            .stroke(egui::Stroke::new(
                1.0,
                if active { theme::ACCENT } else { theme::BORDER },
            ))
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                body(ui);
            });
        ui.add_space(10.0);
    });
}

fn task_key(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    let prefix = name.split('-').next().unwrap_or(name);
    format!("TASK-{}", prefix.to_uppercase())
}

fn paint_task_details(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    height: f32,
    activity_path: &mut Option<String>,
) {
    ui.heading(&doc.title);
    ui.add_space(8.0);
    let width = ui.available_width();
    if width >= 720.0 {
        let sidebar = (width * 0.3).clamp(220.0, 310.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width - sidebar - 20.0);
                egui::ScrollArea::vertical()
                    .id_salt(("task_paper", &doc.path))
                    .max_height(height)
                    .show(ui, |ui| {
                        crate::ui::spec_viewer::render(ui, Some(&doc.text));
                    });
            });
            ui.vertical(|ui| {
                ui.set_width(sidebar);
                egui::ScrollArea::vertical()
                    .id_salt(("task_properties", &doc.path))
                    .max_height(height)
                    .show(ui, |ui| {
                        task_sidebar(ui, s, doc, activity_path);
                    });
            });
        });
    } else {
        task_sidebar(ui, s, doc, activity_path);
        ui.add_space(12.0);
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
    }
}

fn task_sidebar(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    activity_path: &mut Option<String>,
) {
    ui.label(RichText::new("DETAILS").small().weak());
    paint_task_properties(ui, s, doc);
    ui.separator();
    ui.label(RichText::new("ACTIVITY").small().weak());
    if let Some(progress) = s.task_progress(&doc.path) {
        if crate::ui::task_activity::compact(ui, progress, s.implementation_active(&doc.path)) {
            *activity_path = Some(doc.path.clone());
        }
    } else {
        ui.label("No worker activity recorded yet.");
    }
}
