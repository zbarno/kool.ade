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
    let compact = ui.ctx().content_rect().width() < 960.0;
    Panel::top("packet_header")
        .exact_size(if compact { 52.0 } else { 58.0 })
        .frame(
            Frame::NONE
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(if compact { 14 } else { 22 }, 10)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(if compact { 30.0 } else { 36.0 });
                ui.label(RichText::new("Packet").size(21.0).strong());
                if !compact {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new(s.session_title())
                            .size(14.0)
                            .color(theme::TEXT_DIM),
                    );
                }
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
                    if !compact {
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
                        ui.label(RichText::new(label).size(12.5).color(theme::TEXT_DIM));
                    }
                });
            });
        });
    if compact {
        paint_compact_conversation(ui, s);
    } else {
        Panel::left("packet_chat")
            .default_size((ui.ctx().content_rect().width() * 0.23).clamp(380.0, 480.0))
            .min_size(380.0)
            .max_size(560.0)
            .resizable(true)
            .frame(
                Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(22, 18)),
            )
            .show(ui, |ui| paint_conversation(ui, s, true));
    }
    CentralPanel::default()
        .frame(
            Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(if compact { 12 } else { 28 }, 18)),
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
                                    + s.resolved_items().len()
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
            let view_id = egui::Id::new("packet_spec_document_view");
            let mut view = ui
                .ctx()
                .data_mut(|d| d.get_temp::<u8>(view_id))
                .unwrap_or(if s.active_feature().is_some() { 0 } else { 1 });
            ui.horizontal(|ui| {
                if s.active_feature().is_some()
                    && ui.selectable_label(view == 0, "Active Feature").clicked()
                {
                    view = 0;
                }
                if ui
                    .selectable_label(view == 1, "Product Specification")
                    .clicked()
                {
                    view = 1;
                }
                if s.task_story_preview().is_some()
                    && ui.selectable_label(view == 2, "Task Stories").clicked()
                {
                    view = 2;
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(view_id, view));
            if view == 0 && s.active_feature().is_some() {
                if s.active_feature_approved() {
                    ui.label(RichText::new("Approved for implementation").color(theme::SUCCESS));
                } else if ui
                    .add_enabled(
                        !s.is_busy(),
                        egui::Button::new("Approve feature for implementation"),
                    )
                    .clicked()
                {
                    s.approve_active_feature();
                }
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
                    let text = match view {
                        0 => s
                            .active_feature()
                            .map(|(_, text)| text)
                            .unwrap_or(s.spec_text()),
                        2 => s.task_story_preview().unwrap_or(s.spec_text()),
                        _ => s.spec_text(),
                    };
                    crate::ui::spec_viewer::render(ui, Some(text));
                });
        });
}

fn paint_compact_conversation(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let open_id = egui::Id::new("packet_compact_conversation_open");
    let mut open = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(open_id).unwrap_or(true));
    Panel::top("packet_compact_conversation_header")
        .exact_size(44.0)
        .frame(
            Frame::NONE
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(14, 7)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let marker = if open { "▾" } else { "▸" };
                if ui
                    .button(format!("{marker}  Conversation"))
                    .on_hover_text(if open {
                        "Collapse project manager conversation"
                    } else {
                        "Expand project manager conversation"
                    })
                    .clicked()
                {
                    open = !open;
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Project manager").size(12.5).weak());
                });
            });
        });
    ui.ctx().data_mut(|data| data.insert_temp(open_id, open));
    if open {
        let max_height = (ui.ctx().content_rect().height() * 0.42).clamp(210.0, 360.0);
        Panel::top("packet_compact_conversation_body")
            .default_size(max_height.min(280.0))
            .min_size(180.0)
            .max_size(max_height)
            .resizable(true)
            .frame(
                Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| paint_conversation(ui, s, false));
    }
}

fn paint_conversation(ui: &mut egui::Ui, s: &mut dyn Surface, heading: bool) {
    if heading {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Conversation").size(17.0).strong());
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("Project manager").size(12.5).weak());
            });
        });
        ui.add_space(24.0);
    }
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
}

fn paint_tasks(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let panel_bounds = ui.max_rect();
    let activity_id = egui::Id::new("packet_task_activity");
    let mut activity_path = ui.ctx().data_mut(|d| d.get_temp::<String>(activity_id));
    let id = egui::Id::new("packet_selected_task");
    let mut selected_path = ui.ctx().data_mut(|d| d.get_temp::<String>(id));
    let docs = s.task_documents().to_vec();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("All activity").strong());
        ui.label(
            RichText::new("Updates / 10s · last 10 minutes")
                .small()
                .weak(),
        );
    });
    crate::ui::task_activity::graph(ui, &s.activity_samples(None), true, 48.0);
    ui.horizontal_wrapped(|ui| {
        for (kind, label) in [
            (None, "Task"),
            (Some(crate::domain::ItemKind::Question), "Question"),
            (Some(crate::domain::ItemKind::Ambiguity), "Ambiguity"),
            (Some(crate::domain::ItemKind::Assumption), "Assumption"),
            (Some(crate::domain::ItemKind::Ownership), "Ownership"),
        ] {
            ui.colored_label(theme::board_hue(kind), format!("● {label}"));
        }
    });
    egui::CollapsingHeader::new(RichText::new("Board overview").strong())
        .id_salt("packet_board_overview")
        .default_open(true)
        .show(ui, |ui| {
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
            if let Some(progress) = docs.iter().find(|d| d.path.ends_with("/README.md")) {
                ui.label(RichText::new(&progress.title).weak());
                if ui.button("Batch overview").clicked() {
                    selected_path = Some(progress.path.clone());
                }
            }
        });
    let mut planning_selection = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(egui::Id::new("packet_selected_planning")));
    let mut items = s
        .items()
        .iter()
        .chain(s.synthetic_items())
        .chain(s.resolved_items())
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
        .size(12.5)
        .weak(),
    );
    let height = (ui.available_height() - 24.0).max(120.0);
    let gaps = ui.spacing().item_spacing.x * 4.0;
    let column_width = ((ui.available_width() - 100.0 - gaps - 2.0) / 5.0).max(190.0);
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
                                        let active = s.activity_active(item.conversation_key());
                                        board_card(ui, &item.id, Some(item.kind), active, |ui| {
                                            ui.label(RichText::new(format!("{} · {}", if item.id.starts_with("ownership:") { "Pending" } else { &item.id }, item.kind)).size(12.5).color(theme::ACCENT));
                                            if ui.add(egui::Button::new(RichText::new(card_summary(&item.question)).strong()).frame(false).wrap()).on_hover_text(&item.question).clicked() {
                                                planning_selection = Some(item.id.clone());
                                            }
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(RichText::new(item.priority.to_string()).size(12.5).color(if item.priority == crate::domain::item::Priority::Blocking { theme::DANGER } else { theme::WARNING }));
                                                ui.label(RichText::new(item.authority.to_string()).size(12.5).color(match item.authority {
                                                    crate::domain::Authority::Agent => theme::SUCCESS,
                                                    crate::domain::Authority::Review => theme::WARNING,
                                                    crate::domain::Authority::Human => theme::ACCENT,
                                                }));
                                                ui.label(RichText::new(item.assigned_to.as_deref().unwrap_or("Unassigned")).size(12.5).weak());
                                            });
                                            ui.label(RichText::new(&item.category).size(12.5).weak());
                                            if active { ui.label(RichText::new("● Active").color(theme::SUCCESS)); }
                                            crate::ui::task_activity::graph(ui, &s.activity_samples(Some(item.conversation_key())), active, 34.0);
                                            if let Some(progress) = s.task_progress(&item.id) {
                                                if let Some(activity) = &progress.activity {
                                                    ui.label(RichText::new(card_summary(activity)).size(12.5).color(theme::SUCCESS));
                                                }
                                                if !progress.response.trim().is_empty() {
                                                    ui.label(RichText::new(card_summary(progress.response.trim())).size(12.0).weak());
                                                }
                                            }
                                            if eligible.contains(&item.id) { ui.label(RichText::new("For you").size(12.5).color(theme::ACCENT)); }
                                            if s.next_question_id() == Some(item.id.as_str()) { ui.label("Asking now"); }
                                            task_conversation(ui, s, item.conversation_key(), false);
                                        });
                                    }
                                    for doc in cards {
                                        let active = s.implementation_active(&doc.path);
                                        let activity_active = s.activity_active(&doc.path);
                                        board_card(ui, &doc.path, None, activity_active, |ui| {
                                            ui.label(RichText::new(format!("{} · Task", task_key(&doc.path))).size(12.5).color(theme::ACCENT));
                                            if ui.add(egui::Button::new(RichText::new(&doc.title).strong()).frame(false).wrap()).clicked() {
                                                selected_path = Some(doc.path.clone());
                                            }
                                            let status = s.implementation_state(&doc.path).map(|r| r.status.as_str()).unwrap_or(if active { "Starting" } else { "To do" });
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(RichText::new(status).size(12.5).color(if active { theme::ACCENT } else { theme::TEXT_DIM }));
                                                ui.label(RichText::new(if active { "Assigned worker" } else { "Task worker" }).size(12.5).weak());
                                            });
                                            crate::ui::task_activity::graph(ui, &s.activity_samples(Some(&doc.path)), activity_active, 34.0);
                                            if let Some(progress) = s.task_progress(&doc.path) {
                                                if crate::ui::task_activity::compact(ui, progress, active) { activity_path = Some(doc.path.clone()); }
                                            } else if active { ui.spinner(); ui.label("Waiting for worker output…"); }
                                            task_conversation(ui, s, &doc.path, false);
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
            let mut approve = false;
            let closed = crate::ui::overlays::show_panel_modal(
                ui,
                &format!(
                    "Planning · {}",
                    if item.id.starts_with("ownership:") {
                        "Ownership assignment"
                    } else {
                        &item.id
                    }
                ),
                panel_bounds,
                |ui| {
                    ui.heading(&item.question);
                    ui.label(format!(
                        "{} · {} · {:?}",
                        item.kind, item.priority, item.status
                    ));
                    ui.label(format!("Authority: {}", item.authority));
                    ui.label(format!("Category: {}", item.category));
                    ui.label(format!(
                        "Owner: {}",
                        item.assigned_to.as_deref().unwrap_or("Unassigned")
                    ));
                    ui.label(&item.reason);
                    if let Some(feature) = &item.feature_id {
                        ui.label(format!("Feature: {feature}"));
                    }
                    if !item.evidence.is_empty() {
                        ui.separator();
                        ui.label(RichText::new("Evidence").strong());
                        ui.label(&item.evidence);
                    }
                    if !item.recommendation.is_empty() {
                        ui.separator();
                        ui.label(RichText::new("Packet's recommendation").strong());
                        ui.label(&item.recommendation);
                    }
                    if let Some(progress) = s.task_progress(&item.id) {
                        ui.separator();
                        ui.heading("Agent investigation");
                        if let Some(activity) = &progress.activity {
                            ui.label(activity);
                        }
                        ui.label(format!("Activity updates: {}", progress.telemetry.updates));
                        if !progress.response.trim().is_empty() {
                            ui.collapsing("Latest output", |ui| {
                                ui.label(crate::core::context_build::clip(
                                    &progress.response,
                                    4000,
                                ));
                            });
                        }
                        if !progress.thoughts.trim().is_empty() {
                            ui.collapsing("Worker thoughts", |ui| {
                                ui.label(crate::core::context_build::clip(
                                    &progress.thoughts,
                                    4000,
                                ));
                            });
                        }
                    }
                    if eligible.contains(&item.id) {
                        ui.label("This item is in your planning queue.");
                    }
                    if s.next_question_id() == Some(item.id.as_str()) {
                        ui.label("The project manager is asking about this item now.");
                    }
                    task_conversation(ui, s, item.conversation_key(), true);
                    if item.authority == crate::domain::Authority::Review
                        && item.feature_id.is_some()
                        && !item.recommendation.is_empty()
                    {
                        approve = ui.button("Approve provisional decision").clicked();
                    }
                    if item.is_ownership_gap() && ui.button("Assign ownership").clicked() {
                        s.on_header_action(HeaderAction::Stakeholders);
                    }
                },
            );
            if approve {
                s.approve_review_item(&item.id);
            }
            if closed || approve {
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
    } else if item.is_ownership_gap()
        || (item.priority == crate::domain::item::Priority::Blocking
            && item.authority == crate::domain::Authority::Human)
    {
        3
    } else {
        match item.authority {
            crate::domain::Authority::Agent => 1,
            crate::domain::Authority::Review => 2,
            crate::domain::Authority::Human => 0,
        }
    }
}

fn paint_task_properties(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
) {
    ui.add_space(10.0);
    ui.label(RichText::new(&doc.path).size(12.5).weak());
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
                    .size(12.5)
                    .weak(),
            );
            if let Some(checked) = &record.pr_checked_at {
                ui.label(
                    RichText::new(format!("PR last checked: {checked}"))
                        .size(12.5)
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

fn board_card(
    ui: &mut egui::Ui,
    key: &str,
    kind: Option<crate::domain::ItemKind>,
    active: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(key, |ui| {
        theme::board_frame(kind, active).show(ui, |ui| {
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

fn task_conversation(ui: &mut egui::Ui, s: &mut dyn Surface, key: &str, expanded: bool) {
    ui.push_id(("task_conversation", key, expanded), |ui| {
        if expanded {
            ui.separator();
            ui.heading("Task conversation");
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for message in s.task_messages(key) {
                        ui.label(
                            RichText::new(format!("{:?} · {}", message.role, message.time_label()))
                                .small()
                                .weak(),
                        );
                        ui.label(&message.text);
                    }
                });
        } else if let Some(message) = s.task_messages(key).last() {
            ui.label(RichText::new(card_summary(&message.text)).small());
        }
        if s.task_chat_active(key) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Replying…");
                if ui.button("Stop reply").clicked() {
                    s.cancel_task_reply(key);
                }
            });
        }
        if let Some(error) = s.task_chat_error() {
            ui.colored_label(theme::DANGER, error);
            if ui.button("Retry saving conversation").clicked() {
                s.retry_task_chat_save();
            }
        }
        let busy = s.task_reply_busy();
        let mut send = false;
        if let Some(draft) = s.task_draft(key) {
            let response = if expanded {
                ui.add(
                    egui::TextEdit::multiline(draft)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Continue this task conversation…"),
                )
            } else {
                ui.add(
                    egui::TextEdit::singleline(draft)
                        .desired_width(f32::INFINITY)
                        .hint_text("Reply to this item…"),
                )
            };
            let enter = if expanded {
                response.has_focus()
                    && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
            } else {
                response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            };
            let enabled = !busy && !draft.trim().is_empty();
            let clicked = ui
                .add_enabled(enabled, egui::Button::new("Send reply"))
                .clicked();
            send = enabled && (enter || clicked);
        }
        if send {
            s.send_task_reply(key);
        }
    });
}

fn paint_task_details(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    height: f32,
    activity_path: &mut Option<String>,
) {
    ui.heading(&doc.title);
    task_conversation(ui, s, &doc.path, true);
    ui.add_space(8.0);
    let width = ui.available_width().min(1460.0);
    let inset = ((ui.available_width() - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(inset);
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.x = 16.0;
                let width = ui.available_width().min(1460.0);
                if width >= 720.0 {
                    let sidebar = (width * 0.3).clamp(280.0, 420.0);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(width - sidebar - ui.spacing().item_spacing.x - 16.0);
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
            },
        );
    });
}

fn task_sidebar(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    activity_path: &mut Option<String>,
) {
    ui.label(RichText::new("DETAILS").size(12.5).weak());
    paint_task_properties(ui, s, doc);
    ui.separator();
    ui.label(RichText::new("ACTIVITY").size(12.5).weak());
    if let Some(progress) = s.task_progress(&doc.path) {
        if crate::ui::task_activity::compact(ui, progress, s.implementation_active(&doc.path)) {
            *activity_path = Some(doc.path.clone());
        }
    } else {
        ui.label("No worker activity recorded yet.");
    }
}

fn card_summary(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 160 {
        flat
    } else {
        format!("{}…", flat.chars().take(160).collect::<String>())
    }
}
