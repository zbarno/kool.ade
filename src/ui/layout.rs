//! Workspace with persistent tabbed conversations beside the Kanban board.
use crate::app::dialogs;
use crate::ui::{Surface, theme};
use egui::{CentralPanel, Frame, Layout, Panel, RichText};

pub enum HeaderAction {
    Refresh,
    Import,
    Stakeholders,
    McpServers,
    CopySpec,
    OpenWorkspace,
    Disconnect,
}

pub fn paint(ui: &mut egui::Ui, s: &mut dyn Surface) {
    s.drain_task_chat_saves();
    let compact = ui.ctx().content_rect().width() < 960.0;
    let settings_id = egui::Id::new("packet_workspace_settings_open");
    let mut settings_open = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(settings_id).unwrap_or(false));
    // Prior-frame openness: drives the persona draft-drop on the close edge.
    let settings_was_open = settings_open;
    Panel::top("packet_header")
        .exact_size(if compact { 78.0 } else { 108.0 })
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
                        if ui.button("Settings…").clicked() {
                            settings_open = true;
                            ui.close();
                        }
                        ui.separator();
                        for (label, action) in [
                            ("Import references", HeaderAction::Import),
                            ("Stakeholders & ownership", HeaderAction::Stakeholders),
                            ("MCP servers", HeaderAction::McpServers),
                            ("Refresh repository", HeaderAction::Refresh),
                            ("Open workspace", HeaderAction::OpenWorkspace),
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
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if s.active_task_count() > 0 {
                        format!("● {} tasks", s.active_task_count())
                    } else if s.is_busy() {
                        "● Working".into()
                    } else {
                        "● Ready".into()
                    })
                    .small(),
                );
                if !compact && !s.queue_status().is_empty() {
                    ui.add_sized(
                        [260.0, 20.0],
                        egui::Label::new(
                            RichText::new(s.queue_status().lines().next().unwrap_or_default())
                                .small(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(s.queue_status());
                }
                ui.label(RichText::new("All activity").small())
                    .on_hover_text("Updates per 10 seconds · last 10 minutes");
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width().max(40.0), 24.0),
                    Layout::top_down(egui::Align::Min),
                    |ui| {
                        crate::ui::task_activity::graph(ui, &s.activity_samples(None), true, 24.0);
                    },
                );
            });
        });
    let chat_panel = if compact {
        Panel::top("packet_chat_panel").exact_size(230.0)
    } else {
        Panel::left("packet_chat_panel")
            .exact_size((ui.available_width() * 0.32).clamp(340.0, 560.0))
    };
    chat_panel
        .frame(Frame::NONE.fill(theme::BG).inner_margin(12))
        .show(ui, |ui| paint_chat_tabs(ui, s));
    CentralPanel::default()
        .frame(
            Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(
                    if compact { 12 } else { 28 },
                    if compact { 4 } else { 18 },
                )),
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
                                    .filter(|d| !d.path.ends_with("/README.md") && !s.task_archived(&d.path))
                                    .count()
                                    + s.items().iter().chain(s.synthetic_items()).chain(s.resolved_items())
                                        .filter(|i| !s.task_archived(i.conversation_key())).count()
                                    + s.planning_work().iter().filter(|w| !s.task_archived(&w.key)).count()
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
                .unwrap_or(0);
            let features = s.active_features().into_iter()
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
                    && ui.selectable_label(view == 1, format!("Features  {}", features.len())).clicked()
                {
                    view = 1;
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(view_id, view));
            let selected_id = egui::Id::new("packet_selected_feature");
            let mut selected = ui.ctx().data_mut(|d| d.get_temp::<String>(selected_id))
                .filter(|id| features.iter().any(|(feature_id, _)| feature_id == id))
                .or_else(|| features.first().map(|(id, _)| id.clone()));
            if view == 1 && !features.is_empty() {
                egui::ComboBox::from_id_salt("feature_specification_selector")
                    .selected_text(selected.as_deref().unwrap_or("Select feature"))
                    .show_ui(ui, |ui| {
                        for (id, body) in &features {
                            let title = body.lines().find_map(|line| line.strip_prefix("# "))
                                .unwrap_or(id);
                            ui.selectable_value(&mut selected, Some(id.clone()), title);
                        }
                    });
                if let Some(action) = s.feature_actions(None).into_iter()
                    .find(|action| selected.as_deref() == Some(action.id.as_str()))
                {
                    if ui.add_enabled(!s.conversation_busy(), egui::Button::new(action.label())).clicked() {
                        s.approve_feature(&action.id);
                    }
                } else if selected.as_deref().is_some_and(|id| s.feature_approved(id)) {
                    ui.label(RichText::new("Approved for implementation").color(theme::SUCCESS));
                }

            }
            ui.ctx().data_mut(|d| d.insert_temp(selected_id, selected.clone()));
            let document = if view == 1 {
                selected.as_deref().and_then(|selected| features.iter()
                    .find(|(id, _)| id == selected).map(|(_, body)| body.clone()))
                    .unwrap_or_else(|| s.spec_text().to_owned())
            } else {
                s.spec_text().to_owned()
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(if view == 1 { "Feature specification" } else { "Product specification" }).size(17.0).strong());
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
    if settings_open {
        let mut open_batch = None;
        let closed = crate::ui::overlays::show_modal(ui, true, "Workspace settings", 640.0, |ui| {
            ui.heading("Implementation & queue");
            let mut parallel = s.max_parallel_tasks();
            if ui
                .add(egui::Slider::new(&mut parallel, 1..=8).text("Concurrent tasks"))
                .changed()
            {
                s.set_max_parallel_tasks(parallel);
            }
            ui.label(format!("{} workers active. Dependencies must merge before dependent tasks start. Merges are serialized and reverified.", s.active_task_count()));
            ui.label("Lowering the limit affects new starts; running tasks keep their work.");
            let mut auto_mode = s.auto_mode();
            if ui
                .checkbox(
                    &mut auto_mode,
                    "Auto mode — merge verified tasks and continue the queue",
                )
                .changed()
            {
                s.set_auto_mode(auto_mode);
            }
            ui.label("When enabled, verified tasks merge automatically and the queue continues. Disable to use pull requests for future tasks.");
            ui.label("To begin, send ‘start implementing’ in Main Chat. This approves the feature for the next eligible task and starts it. You can also select and approve a feature in Specifications, then use a task’s Implement action.");
            ui.separator();
            paint_persona_section(ui, s);
            if !s.queue_status().is_empty() {
                ui.separator();
                ui.label(s.queue_status());
            }
            for doc in s
                .task_documents()
                .iter()
                .filter(|d| d.path.ends_with("/README.md"))
            {
                ui.separator();
                ui.label(&doc.title);
                if ui.button("Batch overview").clicked() {
                    open_batch = Some(doc.path.clone());
                }
            }
        });
        if closed {
            settings_open = false;
        }
        if let Some(path) = open_batch {
            settings_open = false;
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new("packet_document_tab"), true);
                d.insert_temp(egui::Id::new("packet_selected_task"), path);
            });
        }
    }
    // Close edge: the modal went open → closed this frame. Drop the VOLATILE
    // persona draft (its two session-lived temp slots) so the NEXT open
    // re-binds to the live persona.md bytes — bind-on-open doctrine, no ghost
    // draft (unsaved edits and diagnostics die with the modal on purpose).
    //
    // Deliberately SURGICAL removals, not an egui `IdTypeMap::clear()`:
    // on egui 0.36 a clear() would wipe EVERY temporary and persisted value
    // (the operator's open chat tabs, document views, widget state) — far
    // beyond this ticket's purely-additive bounds. Only the two persona
    // slots owe expiry here.
    if settings_was_open && !settings_open {
        ui.ctx().data_mut(|d| {
            d.remove_temp::<bool>(egui::Id::new("packet_persona_was_closed"));
            d.remove_temp::<dialogs::DlgPersona>(egui::Id::new("packet_persona_card"));
        });
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(settings_id, settings_open));
}

/// Planner persona section inside the Workspace settings modal (host glue
/// only — the card model and painter live in [`crate::app::dialogs`]).
///
/// Lifecycle (bind-on-open):
/// * the FIRST entry into a modal open-cycle constructs
///   `DlgPersona::from_load(&persona::load_persona())` — that load IS the
///   first-run seed trigger (story 001 owns the seed write itself);
/// * later frames of the same cycle reuse the temp-slot draft, so unsaved
///   edits survive redraws for the duration of the open;
/// * when the modal closes, [`paint`] expunges the slots and the next open
///   re-binds to the live file bytes — no stale draft resurrection.
///
/// Signals: save → Written (success toast) / Unchanged (info toast) /
/// Err (card keeps the modal open with the red feedback line already set);
/// restore → Ok (success toast) / Err (same keep-open red line).
fn paint_persona_section(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let card_slot = egui::Id::new("packet_persona_card");
    let live_flag = egui::Id::new("packet_persona_was_closed");
    // Flag absent or true == "was closed" == pristine: first entry of this
    // open cycle. false == a live draft occupies the slot.
    let live = ui.ctx().data_mut(|d| d.get_temp::<bool>(live_flag)) == Some(false);
    let mut card = if live {
        ui.ctx()
            .data_mut(|d| d.remove_temp::<dialogs::DlgPersona>(card_slot))
            .unwrap_or_else(|| {
                dialogs::DlgPersona::from_load(&crate::persistence::persona::load_persona())
            })
    } else {
        dialogs::DlgPersona::from_load(&crate::persistence::persona::load_persona())
    };
    let (save_pressed, restore_pressed) = dialogs::paint_persona_card(ui, &mut card);
    if save_pressed {
        match card.save() {
            Ok(dialogs::PersonaSaveOutcome::Written) => s
                .toasts()
                .success("Persona saved \u{2014} effective from the next reply."),
            Ok(dialogs::PersonaSaveOutcome::Unchanged) => {
                s.toasts().info("Persona already in sync.")
            }
            // Err: the card already carries the red feedback line; the modal
            // stays open (only its X dismisses).
            Err(_) => {}
        }
    }
    if restore_pressed && card.restore_default().is_ok() {
        s.toasts().success("Persona default restored")
    }
    // Err path: red feedback already set on the card; modal stays open.
    ui.ctx().data_mut(|d| {
        d.insert_temp(live_flag, false);
        d.insert_temp(card_slot, card);
    });
}

#[derive(Clone, Default)]
pub(crate) struct ChatTabs {
    pub(crate) keys: Vec<String>,
    pub(crate) active: Option<String>,
    reveal_active: bool,
}

impl ChatTabs {
    fn open(&mut self, key: &str) {
        if !self.keys.iter().any(|existing| existing == key) {
            self.keys.push(key.to_owned());
        }
        self.active = Some(key.to_owned());
        self.reveal_active = true;
    }

    fn close(&mut self, key: &str) {
        self.keys.retain(|existing| existing != key);
        if self.active.as_deref() == Some(key) {
            self.active = None;
        }
    }
}

fn conversation_title(s: &dyn Surface, key: &str) -> String {
    s.items()
        .iter()
        .chain(s.synthetic_items())
        .chain(s.resolved_items())
        .find(|item| item.conversation_key() == key)
        .map(|item| item.question.clone())
        .or_else(|| s.planning_work().into_iter().find(|w| w.key == key).map(|w| w.title))
        .or_else(|| {
            s.task_documents()
                .iter()
                .find(|doc| doc.path == key)
                .map(|doc| doc.title.clone())
        })
        .unwrap_or_else(|| key.to_owned())
}

fn paint_chat_tabs(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let id = egui::Id::new("packet_chat_tabs");
    let mut tabs = ui
        .ctx()
        .data_mut(|d| d.get_temp::<ChatTabs>(id).unwrap_or_default());
    egui::ScrollArea::horizontal()
        .id_salt("chat_tabs")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(tabs.active.is_none(), "Main Chat")
                    .clicked()
                {
                    tabs.active = None;
                }
                for key in tabs.keys.clone() {
                    ui.push_id(&key, |ui| {
                        let title = conversation_title(s, &key);
                        let label = s
                            .items()
                            .iter()
                            .chain(s.synthetic_items())
                            .chain(s.resolved_items())
                            .find(|item| item.conversation_key() == key)
                            .map(|item| item.id.clone())
                            .unwrap_or_else(|| {
                                if key.contains('/') {
                                    task_key(&key)
                                } else {
                                    key.clone()
                                }
                            });
                        let selected = tabs.active.as_deref() == Some(key.as_str());
                        let mut select = false;
                        let mut close = false;
                        let tab = Frame::NONE
                            .fill(if selected {
                                theme::ACCENT_SOFT
                            } else {
                                theme::PANEL
                            })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if selected {
                                    theme::ACCENT
                                } else {
                                    theme::BORDER
                                },
                            ))
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(5, 2))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.horizontal(|ui| {
                                    select = ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&label).color(theme::TEXT),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text(format!("{title}\n{key}"))
                                        .clicked();
                                    close = ui
                                        .add(egui::Button::new("×").frame(false))
                                        .on_hover_text(format!("Close {label}"))
                                        .clicked();
                                });
                            });
                        if selected && tabs.reveal_active {
                            tab.response.scroll_to_me(Some(egui::Align::Center));
                        }
                        if close {
                            tabs.close(&key);
                        } else if select {
                            tabs.active = Some(key.clone());
                        }
                    });
                }
            });
        });
    tabs.reveal_active = false;
    ui.separator();
    ui.push_id(("chat_tab", &tabs.active), |ui| {
        if let Some(key) = tabs.active.as_deref() {
            s.prepare_task_chat(key);
            let context = s.task_chat_context(key);
            let actions = s.feature_actions(Some(key));
            let title = conversation_title(s, key);
            ui.add(egui::Label::new(RichText::new(&title).strong()).truncate())
                .on_hover_text(title);
            let messages = s
                .task_messages(key)
                .iter()
                .map(|message| {
                    let mut readable = message.clone();
                    readable.text = crate::ui::message_text::readable(message).into_owned();
                    readable
                })
                .collect::<Vec<_>>();
            let busy = s.task_chat_active(key);
            let active = s.task_chat_active(key);
            if let Some(progress) = s.task_reply_progress(key).cloned() {
                ui.collapsing("Reply activity", |ui| crate::ui::chat_pane::paint_progress(ui, &progress));
            }
            if let Some(error) = s.task_chat_error() {
                ui.colored_label(theme::WARNING, error);
                if ui.button("Retry saving conversation").clicked() {
                    s.retry_task_chat_save();
                }
            }
            if let Some(draft) = s.task_draft(key) {
                let intent = crate::ui::chat_pane::paint_task_with_actions(ui, &messages, draft, busy, context.as_deref(), &actions);
                if let Some(id) = &intent.approve_feature {
                    s.approve_feature(id);
                }
                if intent.send {
                    s.send_task_reply(key);
                }
                if intent.cancel && active {
                    s.cancel_task_reply(key);
                }
            }
        } else {
            paint_conversation(ui, s, false);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, tabs));
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
    let implementation_offer = s.implementation_offer();
    let actions = s.feature_actions(None);
    let intent = crate::ui::chat_pane::paint_with_actions(
        ui,
        &msgs,
        s.chat_draft(),
        busy,
        progress.as_ref(),
        crate::ui::chat_pane::Actions {
            task_offer: offer.as_ref(), implementation_offer, features: &actions,
        },
    );
    if intent.send || intent.cancel || intent.generate_tasks || intent.implement_tasks || intent.approve_feature.is_some() {
        s.on_intent(&intent);
    }
}

fn paint_tasks(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let viewport = ui.ctx().content_rect();
    let panel_bounds = egui::Rect::from_center_size(
        viewport.center(),
        egui::vec2(viewport.width().min(900.0), viewport.height()),
    );
    let activity_id = egui::Id::new("packet_task_activity");
    let mut activity_path = ui.ctx().data_mut(|d| d.get_temp::<String>(activity_id));
    let show_activity = activity_path.is_some();
    let id = egui::Id::new("packet_selected_task");
    let mut selected_path = ui.ctx().data_mut(|d| d.get_temp::<String>(id));
    let docs = s.task_documents().to_vec();
    let work = s.planning_work();
    if viewport.width() >= 960.0 {
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
    }
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
    if viewport.width() >= 960.0 {
        ui.label(
            RichText::new(
                "Planning questions and task workers · select a card for details and activity",
            )
            .size(12.5)
            .weak(),
        );
    }
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
                    let planning = work.iter().filter(|w| w.column == column && !s.task_archived(&w.key)).collect::<Vec<_>>();
                    let cards = docs
                        .iter()
                        .filter(|doc| {
                            !doc.path.ends_with("/README.md")
                                && !s.task_archived(&doc.path)
                                && task_board_column(s, &doc.path) == column
                        })
                        .collect::<Vec<_>>();
                    let questions = items
                        .iter()
                        .filter(|item| !s.task_archived(item.conversation_key()))
                        .filter(|item| crate::ui::task_chat::board_column(
                            if s.activity_active(item.conversation_key()) { 1 } else {
                            planning_column(item)
                            },
                            s.task_messages(item.conversation_key()),
                            s.task_chat_active(item.conversation_key()),
                        ) == column)
                        .collect::<Vec<_>>();
                    egui::Frame::NONE.fill(theme::BG).corner_radius(8).inner_margin(10).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(column_width);
                            ui.set_min_height(height);
                            ui.label(
                                RichText::new(format!(
                                    "{label} · {}",
                                    cards.len() + questions.len() + planning.len()
                                ))
                                .strong(),
                            );
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .id_salt(("task_board_column", column))
                                .max_height(height - 32.0)
                                .show(ui, |ui| {
                                    for work in &planning {
                                        board_card(ui, &work.key, None, work.column == 1, |ui| {
                                            ui.label(RichText::new("Feature planning").color(theme::ACCENT));
                                            ui.label(RichText::new(&work.title).strong());
                                            ui.label(crate::core::context_build::clip(&work.detail, 180));
                                            if ui.small_button("Open conversation").clicked() {
                                                let mut tabs = ui.ctx().data_mut(|d| d.get_temp::<ChatTabs>(egui::Id::new("packet_chat_tabs"))).unwrap_or_default();
                                                tabs.open(&work.key);
                                                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("packet_chat_tabs"), tabs));
                                            }
                                            if column == 4 && ui.small_button("Archive").clicked() {
                                                s.archive_task(&work.key);
                                            }
                                        });
                                    }
                                    for item in questions {
                                        let active = s.activity_active(item.conversation_key());
                                        board_card(ui, &item.id, Some(item.kind), active, |ui| {
                                            ui.label(RichText::new(format!("{} · {}", if item.id.starts_with("ownership:") { "Pending" } else { &item.id }, item.kind)).size(12.5).color(theme::ACCENT));
                                            if ui.add(egui::Button::new(RichText::new(card_summary(&item.question)).strong()).frame(false).wrap()).on_hover_text(&item.question).clicked() {
                                                planning_selection = Some(item.id.clone());
                                            }
                                            if task_conversation(ui, s, item.conversation_key(), false) { planning_selection = Some(item.id.clone()); }
                                            ui.add_space(4.0);
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
                                            if column == 4 && ui.small_button("Archive").clicked() {
                                                s.archive_task(item.conversation_key());
                                            }
                                            if active {
                                                crate::ui::task_activity::graph(
                                                    ui,
                                                    &s.activity_samples(Some(item.conversation_key())),
                                                    true,
                                                    34.0,
                                                );
                                            }
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
                                            paint_task_failure(ui, s, &doc.path);
                                            if task_conversation(ui, s, &doc.path, false) { selected_path = Some(doc.path.clone()); }
                                            ui.add_space(4.0);
                                            let status = s.implementation_state(&doc.path).map(|r| r.status.clone()).unwrap_or_else(|| if active { "Starting".into() } else { crate::core::implementation::BOARD_COLUMNS[task_board_column(s, &doc.path)].into() });
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(RichText::new(&status).size(12.5).color(if active { theme::ACCENT } else { theme::TEXT_DIM }));
                                                ui.label(RichText::new(if active { "Assigned worker" } else { "Task worker" }).size(12.5).weak());
                                                if column == 4 && ui.small_button("Archive").clicked() {
                                                    s.archive_task(&doc.path);
                                                }
                                            });
                                            if active {
                                                let samples = s.activity_samples(Some(&doc.path));
                                                task_card_activity_band(
                                                    ui,
                                                    &samples,
                                                    true,
                                                    s.task_progress(&doc.path),
                                                    chrono::Utc::now().timestamp_millis(),
                                                );
                                            }
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
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {}",
                            item.kind,
                            item.category,
                            crate::core::implementation::BOARD_COLUMNS
                                [crate::ui::task_chat::board_column(
                                    planning_column(item),
                                    s.task_messages(item.conversation_key()),
                                    s.task_chat_active(item.conversation_key())
                                )]
                        ))
                        .small()
                        .weak(),
                    );
                    if !item.reason.is_empty() {
                        ui.label(crate::core::context_build::clip(&item.reason, 200));
                    }
                    task_conversation(ui, s, item.conversation_key(), true);
                    ui.add_space(12.0);
                    ui.collapsing("Background & evidence", |ui| {
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
                        ui.add(egui::Label::new(&item.reason).wrap());
                        if let Some(feature) = &item.feature_id {
                            ui.label(format!("Feature: {feature}"));
                        }
                        if !item.evidence.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("Evidence").strong());
                            ui.add(egui::Label::new(&item.evidence).wrap());
                        }
                        if !item.recommendation.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("Recommended next step").strong());
                            ui.add(egui::Label::new(&item.recommendation).wrap());
                        }
                    });
                    ui.collapsing("Activity", |ui| {
                        crate::ui::task_activity::graph(
                            ui,
                            &s.activity_samples(Some(item.conversation_key())),
                            s.activity_active(item.conversation_key()),
                            48.0,
                        );
                        if let Some(progress) = s.task_progress(&item.id) {
                            ui.separator();
                            ui.heading("Agent investigation");
                            if let Some(activity) = &progress.activity {
                                ui.add(egui::Label::new(activity).wrap());
                            }
                            ui.label(format!("Activity updates: {}", progress.telemetry.updates));
                            if !progress.response.trim().is_empty() {
                                ui.label(RichText::new("Latest result").strong());
                                egui::ScrollArea::vertical()
                                    .max_height(240.0)
                                    .show(ui, |ui| {
                                        crate::ui::markdown::paint(
                                            ui,
                                            &crate::core::context_build::clip(
                                                &progress.response,
                                                4000,
                                            ),
                                            crate::ui::markdown::CHAT,
                                        )
                                    });
                            }
                            if !progress.thoughts.trim().is_empty() {
                                ui.collapsing("Worker notes", |ui| {
                                    crate::ui::markdown::paint(
                                        ui,
                                        &crate::core::context_build::clip(
                                            &progress.thoughts,
                                            4000,
                                        ),
                                        crate::ui::markdown::CHAT,
                                    );
                                });
                            }
                        }
                        if eligible.contains(&item.id) {
                            ui.label("This item is in your planning queue.");
                        }
                        if s.next_question_id() == Some(item.id.as_str()) {
                            ui.label("The project manager is asking about this item now.");
                        }
                    });
                },
            );
            if closed {
                planning_selection = None;
            }
        } else {
            planning_selection = None;
        }
    }
    if let Some(ticket) = activity_path.clone().filter(|_| show_activity) {
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

fn task_board_column(s: &dyn Surface, key: &str) -> usize {
    if !s.implementation_active(key) && s.implementation_failure(key).is_some() { return 3; }
    let base = crate::core::implementation::board_column(
        s.implementation_state(key),
        s.implementation_active(key),
    );
    // An active implementation owns its status even if an older chat failed.
    if s.implementation_active(key) {
        base
    } else {
        crate::ui::task_chat::board_column(base, s.task_messages(key), s.task_chat_active(key))
    }
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
            crate::domain::Authority::Agent => 0,
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
                let status = if matches!(
                    record.status.as_str(),
                    "Preparing" | "Implementing" | "Verifying"
                ) && !s.implementation_active(ticket)
                {
                    "Interrupted — ready to resume"
                } else {
                    &record.status
                };
                ui.label(RichText::new(status).size(12.0).weak());
                if let Some(url) = &record.pr_url {
                    ui.hyperlink_to("Open PR", url);
                }
            } else {
                ui.label(
                    RichText::new(
                        crate::core::implementation::BOARD_COLUMNS[task_board_column(s, ticket)],
                    )
                    .size(12.0)
                    .weak(),
                );
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
                if let Some(at) = &record.cleanup.completed_at {
                    ui.label(format!("Worktree cleanup completed: {at}. Verification evidence retained."));
                } else {
                    ui.label("Worktree cleanup pending; retried automatically while this project is open.");
                }
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

/// Height (pixels) of the task card's red activity-line band, chosen inside
/// P3's bounded 40–56 px card-width band; the constant vertical reservation
/// keeps card geometry stable regardless of telemetry density.
const CARD_ACTIVITY_BAND_PX: f32 = 48.0;

/// The metric-meaningful hover legend (REQ-F19-2) carried by the task card's
/// activity band; attached unconditionally to the line chart's response so
/// flat-baseline cards explain the metric too.
const CARD_ACTIVITY_HOVER: &str = "Updates per 10-second bucket · Last 10 minutes · Token usage is not reported · Empty buckets do not mean the worker stopped.";

/// Window edge (rightmost ten-second tick) for a task card's 60-slot activity
/// line, derived from card state. Six ticks make a minute.
///
/// Active regime: `(now_ms / 60_000) * 6` — the right edge sits at the current
/// minute's START (story 1's sanctioned live anchor), so the in-progress
/// minute is never peered into and the window rolls forward exactly six ticks
/// per minute across repaints.
///
/// Settled regime: `((last_ms / 60_000) + 1) * 6` — the CLOSE of the record's
/// last-updated minute, with `last_ms` falling back through
/// `telemetry.updated_ms` to the newest recorded sample bucket's end proxy
/// (`bucket * 10_000`) to `now_ms`. Minute-close is deliberate: bucket `b`
/// covers `[b * 10_000, b * 10_000 + 9999]`, so minute-start alignment would
/// strand the final burst of the last updated minute outside the window
/// (e.g. updated_ms = 1_009_999 puts bucket 100 past tick 96's right edge),
/// while minute-close keeps every recorded sample of that minute in view, keeps
/// both regimes on the same six-tick grid, and — because `last_ms` is a pure
/// function of persisted record fields — makes the settled anchor identical
/// across repaints and relaunches (the static final-window guarantee).
fn task_card_activity_anchor(
    active: bool,
    progress: Option<&crate::harness::LiveProgress>,
    now_ms: i64,
) -> i64 {
    if active {
        (now_ms / 60_000) * 6
    } else {
        let last_ms = progress
            .and_then(|p| {
                p.telemetry.updated_ms.or_else(|| {
                    p.telemetry
                        .samples
                        .iter()
                        .map(|(bucket, _)| *bucket * 10_000)
                        .max()
                })
            })
            .unwrap_or(now_ms);
        ((last_ms / 60_000) + 1) * 6
    }
}

/// Mounts the task card's red activity band directly below the card's
/// existing activity preview: computes the card-state-derived anchor
/// ([task_card_activity_anchor]), delegates all drawing to story 1's
/// payload-free primitive `crate::ui::task_activity::line_chart` in
/// `theme::DANGER` over a [CARD_ACTIVITY_BAND_PX]-tall card-width band,
/// attaches [CARD_ACTIVITY_HOVER] unconditionally to the primitive's returned
/// response (so the legend survives degenerate windows, REQ-F19-2), and — per
/// story 1's contract, repaint cadence lives at the mount, not the primitive
/// — schedules the one-second repaint only while this card's worker is
/// running (`active`). The clock arrives as an injected parameter so tests
/// stay wall-clock-free.
fn task_card_activity_band(
    ui: &mut egui::Ui,
    samples: &[(i64, u64)],
    active: bool,
    progress: Option<&crate::harness::LiveProgress>,
    now_ms: i64,
) {
    let anchor = task_card_activity_anchor(active, progress, now_ms);
    let response = crate::ui::task_activity::line_chart(
        ui,
        samples,
        anchor,
        theme::DANGER,
        egui::vec2(ui.available_width(), CARD_ACTIVITY_BAND_PX),
    );
    response.on_hover_text(CARD_ACTIVITY_HOVER);
    if active {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
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
    if name.split_once("-TASK-").is_some_and(|(id, _)| crate::artifacts::product_docs::valid_feature_id(id)) {
        return name.to_owned();
    }
    let prefix = name.split('-').next().unwrap_or(name);
    format!("TASK-{}", prefix.to_uppercase())
}

fn task_conversation(ui: &mut egui::Ui, s: &mut dyn Surface, key: &str, expanded: bool) -> bool {
    if crate::ui::task_chat::paint(ui, s, key, expanded) {
        let id = egui::Id::new("packet_chat_tabs");
        ui.ctx().data_mut(|d| {
            let mut tabs = d.get_temp::<ChatTabs>(id).unwrap_or_default();
            tabs.open(key);
            d.insert_temp(id, tabs);
        });
        ui.ctx().request_repaint();
    }
    false
}

fn paint_task_failure(ui: &mut egui::Ui, s: &dyn Surface, ticket: &str) {
    if s.implementation_active(ticket) { return; }
    let record = s.implementation_state(ticket);
    if let Some(error) = record.and_then(|r| r.cleanup.error.as_deref()) {
        ui.colored_label(egui::Color32::LIGHT_RED, "Cleanup needs attention");
        ui.label(card_summary(error)).on_hover_text(error);
        ui.collapsing("Cleanup details", |ui| {
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| { ui.label(error); });
            if ui.small_button("Copy cleanup failure").clicked() { ui.ctx().copy_text(error.to_owned()); }
        });
        ui.label("Task completed. Worktrees are preserved where cleanup was unsafe or failed. Cleanup retries automatically every minute while this project is open.");
    }
    let failure = s.implementation_failure(ticket).or_else(|| {
        record.filter(|r| r.status == "Needs attention").map(|r| r.detail.as_str())
    });
    let interrupted = record.is_some_and(|r| matches!(r.status.as_str(),
        "Preparing" | "Implementing" | "Verifying" | "Interrupted" | "Waiting to merge" | "Publishing" | "Ready for PR"));
    if let Some(error) = failure {
        ui.colored_label(egui::Color32::LIGHT_RED, "Needs attention");
        ui.label(failure_summary(error)).on_hover_text(error);
        ui.collapsing("Failure details", |ui| {
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    crate::ui::markdown::paint(ui, error, crate::ui::markdown::CHAT);
                });
            if ui.small_button("Copy failure").clicked() { ui.ctx().copy_text(error.to_owned()); }
        });
        ui.label("Open failure details for evidence. Resume implementation after the listed action is complete.");
    } else if interrupted {
        ui.colored_label(egui::Color32::LIGHT_RED, "Interrupted — no worker is running");
        ui.label("Resume implementation to continue preserved work.");
    }
}

fn paint_task_details(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    _height: f32,
    activity_path: &mut Option<String>,
) {
    ui.label(RichText::new(task_key(&doc.path)).small().color(theme::TEXT_DIM));
    ui.heading(&doc.title);
    if doc.path.ends_with("/README.md") {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
        return;
    }
    let ticket = &doc.path;
    let record = s.implementation_state(ticket).cloned();
    let active = s.implementation_active(ticket);
    let cleanup_error = record.as_ref().and_then(|r| r.cleanup.error.clone());
    let failure = s.implementation_failure(ticket).map(str::to_owned).or_else(|| {
        record.as_ref().filter(|r| r.status == "Needs attention").map(|r| r.detail.clone())
    });
    let column = task_board_column(s, ticket);
    let status = if column == 4 && cleanup_error.is_some() {
        "Done · cleanup needs attention"
    } else if record.as_ref().is_some_and(|r| r.pr_state.as_deref() == Some("CLOSED")) {
        "PR closed"
    } else if active {
        record.as_ref().map(|r| r.status.as_str()).unwrap_or("Starting")
    } else if failure.is_some() {
        "Needs attention"
    } else if record.as_ref().is_some_and(|r| matches!(r.status.as_str(), "Preparing" | "Implementing" | "Verifying" | "Publishing" | "Waiting to merge" | "Ready for PR" | "Interrupted")) {
        "Interrupted"
    } else {
        record.as_ref().map(|r| r.status.as_str())
            .unwrap_or(crate::core::implementation::BOARD_COLUMNS[column])
    };
    ui.add_space(10.0);
    egui::Frame::NONE.fill(theme::PANEL_ALT).corner_radius(8).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("CURRENT STATE").size(10.5).strong().color(theme::TEXT_DIM));
        ui.label(RichText::new(status).size(20.0).strong().color(if failure.is_some() || cleanup_error.is_some() || matches!(status, "Interrupted" | "PR closed") { theme::WARNING } else if column == 4 { theme::SUCCESS } else { theme::TEXT }));
        if active {
            if let Some(progress) = s.task_progress(ticket) {
                ui.label(crate::ui::task_activity::preview(progress));
            } else {
                ui.label("Worker is starting.");
            }
        } else if let Some(error) = &failure {
            let headline = if error.starts_with("## Waiting for user action") {
                error.split("### Next action(s)").next()
                    .unwrap_or(error).trim_start_matches("## Waiting for user action").trim()
            } else { error.as_str() };
            ui.label(card_summary(headline)).on_hover_text(error);
        } else if let Some(error) = &cleanup_error {
            ui.label(card_summary(error)).on_hover_text(error);
        } else if status == "PR closed" {
            ui.label("The pull request closed before merging.");
        } else if status == "Interrupted" {
            ui.label("The worker stopped. Preserved work is ready to resume.");
        } else if column == 2 {
            ui.label("Implementation is ready for review.");
        } else if let Some(error) = &cleanup_error {
            ui.label("Review the preserved worktree. Cleanup retries automatically while this project is open.");
            ui.collapsing("Cleanup details", |ui| {
                ui.label(error);
                if ui.small_button("Copy cleanup failure").clicked() { ui.ctx().copy_text(error.clone()); }
            });
        } else if column == 4 {
            ui.label("Implementation is complete.");
        } else {
            ui.label("Ready for Packet to start this task.");
        }
    });

    ui.add_space(10.0);
    egui::Frame::NONE.fill(theme::ACCENT_SOFT).corner_radius(8).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("NEXT ACTION").size(10.5).strong().color(theme::ACCENT));
        if active {
            ui.label("Packet is working. You can stop this task and pause the queue.");
            if ui.button("Stop task and pause queue").clicked() { s.cancel_task_for(ticket); }
        } else if let Some(url) = record.as_ref().and_then(|r| r.pr_url.as_ref()) {
            ui.label(if status == "PR closed" {
                "Reopen the pull request on GitHub to continue review."
            } else { "Review the published changes." });
            ui.hyperlink_to("Open PR", url);
        } else if column == 4 {
            ui.label("No action needed.");
        } else {
            if let Some(error) = &failure {
                let actions = failure_actions(error);
                if actions.is_empty() {
                    ui.label("Review the failure, then resume the preserved work.");
                } else {
                    for action in actions.into_iter().take(2) {
                        ui.label(format!("• {}", card_summary(action))).on_hover_text(action);
                    }
                }
            } else if status == "Interrupted" {
                ui.label("Resume the preserved implementation.");
            } else {
                ui.label("Start implementation when this task is ready.");
            }
            let label = if failure.as_ref().is_some_and(|error| error.contains("## Waiting for user action")) {
                "Resume after action"
            } else if record.is_some() { "Resume implementation" }
                else if s.auto_mode() { "Implement & continue queue" } else { "Implement" };
            if ui.add_enabled(s.implementation_capacity(), egui::Button::new(label).fill(theme::PANEL_ALT)).clicked() {
                s.implement_task(ticket.to_string());
            }
        }
        if let Some(error) = &failure {
            ui.collapsing("Failure details", |ui| {
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    crate::ui::markdown::paint(ui, error, crate::ui::markdown::CHAT);
                });
                if ui.small_button("Copy failure").clicked() { ui.ctx().copy_text(error.clone()); }
            });
        }
    });

    ui.add_space(14.0);
    ui.label(RichText::new("Activity").strong().size(16.0));
    let samples = s.activity_samples(Some(ticket));
    crate::ui::task_activity::graph(ui, &samples, s.activity_active(ticket), 76.0);
    if let Some(progress) = s.task_progress(ticket) {
        ui.label(RichText::new(crate::ui::task_activity::preview(progress)).small());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(crate::ui::task_activity::timing(progress, active)).small().weak());
            if ui.small_button("View all activity").clicked() { *activity_path = Some(ticket.clone()); }
        });
    } else {
        ui.label(RichText::new("No worker activity recorded yet.").small().weak());
    }
    ui.add_space(12.0);
    ui.separator();
    ui.collapsing("Discussion and follow-up", |ui| { task_conversation(ui, s, ticket, true); });
    ui.collapsing("Task description & acceptance criteria", |ui| {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
    });
    ui.collapsing("Technical details", |ui| paint_task_properties(ui, s, doc));
}

fn card_summary(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 160 {
        flat
    } else {
        format!("{}…", flat.chars().take(160).collect::<String>())
    }
}

fn failure_summary(text: &str) -> String {
    text.split_once("### Next action(s)")
        .and_then(|(_, next)| next.lines().find(|line| !line.trim().is_empty()))
        .map(|line| format!("Next: {}", card_summary(line.trim().trim_start_matches("- "))))
        .unwrap_or_else(|| card_summary(text))
}

fn failure_actions(text: &str) -> Vec<&str> {
    text.split_once("### Next action(s)")
        .map(|(_, tail)| tail.lines().filter_map(|line| line.trim().strip_prefix("- ")).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod chat_tab_tests {
    use super::ChatTabs;

    #[test]
    fn multiple_tabs_keep_open_order_and_close_only_the_selected_conversation() {
        let mut tabs = ChatTabs::default();
        tabs.open("first");
        tabs.open("second");
        tabs.open("first");
        assert_eq!(tabs.keys, ["first", "second"]);
        assert_eq!(tabs.active.as_deref(), Some("first"));
        tabs.close("second");
        assert_eq!(tabs.active.as_deref(), Some("first"));
        tabs.open("third");
        tabs.close("third");
        assert!(tabs.active.is_none());
        assert_eq!(tabs.keys, ["first"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a settled-record fixture by mutating `LiveProgress::default()`'
    /// public telemetry fields; no wall clock involved.
    fn settled_fixture(
        updated_ms: Option<i64>,
        samples: &[(i64, u64)],
    ) -> crate::harness::LiveProgress {
        let mut progress = crate::harness::LiveProgress::default();
        progress.telemetry.updated_ms = updated_ms;
        progress.telemetry.samples = samples.to_vec();
        progress
    }

    /// Runs `body` in a fresh default egui context and returns the emitted
    /// shapes (texture deltas cleared, mirroring story 1's captures).
    fn capture(body: impl FnMut(&mut egui::Ui)) -> Vec<egui::epaint::ClippedShape> {
        let mut output = egui::Context::default().run_ui(Default::default(), body);
        output.textures_delta.clear();
        output.shapes
    }

    /// The 60-point polylines in the frame.
    fn polylines(shapes: &[egui::epaint::ClippedShape]) -> Vec<&egui::epaint::PathShape> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Path(path) if path.points.len() == 60 => Some(path),
                _ => None,
            })
            .collect()
    }

    /// The border-colored baseline segments in the frame (coordinate oracle).
    fn baselines(shapes: &[egui::epaint::ClippedShape]) -> Vec<[egui::Pos2; 2]> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::LineSegment { points, stroke } if stroke.color == theme::BORDER => {
                    Some(*points)
                }
                _ => None,
            })
            .collect()
    }

    /// Renders the card band through the mount seam
    /// (`task_card_activity_band`), never the bare primitive.
    fn render_band(
        samples: &[(i64, u64)],
        active: bool,
        progress: Option<&crate::harness::LiveProgress>,
        now_ms: i64,
    ) -> Vec<egui::epaint::ClippedShape> {
        capture(move |ui| task_card_activity_band(ui, samples, active, progress, now_ms))
    }

    #[test]
    fn anchor_matrix_pins_both_regimes_on_the_six_tick_minute_grid() {
        // Active: right edge is the current minute's START; stable inside the
        // minute, exactly +6 at the boundary.
        assert_eq!(task_card_activity_anchor(true, None, 1_015_000), 96);
        assert_eq!(task_card_activity_anchor(true, None, 1_019_999), 96);
        assert_eq!(task_card_activity_anchor(true, None, 1_020_000), 102);
        // Settled: minute-CLOSE of telemetry.updated_ms, wall-clock-blind.
        let stamped = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
        assert_eq!(task_card_activity_anchor(false, Some(&stamped), 0), 102);
        assert_eq!(
            task_card_activity_anchor(false, Some(&stamped), 9_000_000),
            102
        );
        // The minute-boundary instant 960_000 belongs to that minute.
        let boundary = settled_fixture(Some(960_000), &[]);
        assert_eq!(
            task_card_activity_anchor(false, Some(&boundary), 1_015_000),
            102
        );
        // Stamp-less record: the newest sample bucket 100 proxies the last
        // update (end proxy 1_000_000 ms) and lands in-window at slot 57
        // through the shared window math.
        let unstamped = settled_fixture(None, &[(100, 5), (97, 2)]);
        assert_eq!(task_card_activity_anchor(false, Some(&unstamped), 7), 102);
        assert_eq!(
            crate::ui::task_activity::window(&[(100, 5), (97, 2)], 102)[57],
            5
        );
        // Record-less card: falls back to now_ms, still minute-CLOSE aligned.
        assert_eq!(task_card_activity_anchor(false, None, 1_015_000), 102);
    }

    #[test]
    fn settled_anchor_is_static_across_repaints_and_relaunch() {
        // Same record bytes at widely differing wall clocks: the frozen
        // window depends only on persisted record fields.
        let progress = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
        let anchors: Vec<i64> = [500_000i64, 1_000_000, 9_000_000]
            .iter()
            .map(|&now_ms| task_card_activity_anchor(false, Some(&progress), now_ms))
            .collect();
        assert_eq!(anchors, [102, 102, 102], "same record, differing now_ms");
        // Two successive calls agree bit-for-bit (relaunch-survival proxy).
        assert_eq!(
            task_card_activity_anchor(false, Some(&progress), 42),
            task_card_activity_anchor(false, Some(&progress), 43)
        );
        // And the rendered point clouds agree element-wise.
        let samples: &[(i64, u64)] = &[(100, 4), (97, 1)];
        let first = polylines(&render_band(samples, false, Some(&progress), 500_000))[0]
            .points
            .clone();
        let second = polylines(&render_band(samples, false, Some(&progress), 9_000_000))[0]
            .points
            .clone();
        assert_eq!(
            first, second,
            "the final 10-minute window stays permanently static"
        );
    }

    #[test]
    fn active_window_advances_six_ticks_at_the_minute_boundary_and_profile_shifts() {
        // 96 -> 102 across the 1_020_000 boundary: exactly +6 ticks...
        assert_eq!(
            task_card_activity_anchor(true, None, 1_020_000)
                - task_card_activity_anchor(true, None, 1_019_999),
            6
        );
        // ...and the drawn profile slides one minute (six slots) right:
        // bucket 43 sits in slot 6 under anchor 96 and slot 0 under 102.
        let samples: &[(i64, u64)] = &[(43, 7)];
        let before = render_band(samples, true, None, 1_019_999);
        let after = render_band(samples, true, None, 1_020_000);
        let (before_seg, after_seg) = (baselines(&before)[0], baselines(&after)[0]);
        assert_eq!(before_seg, after_seg, "the band geometry itself is fixed");
        let bottom = after_seg[0].y;
        let (pt_before, pt_after) = (
            polylines(&before)[0].points.clone(),
            polylines(&after)[0].points.clone(),
        );
        assert!(
            (bottom - pt_before[6].y - 40.0).abs() <= 0.5,
            "pre-boundary: bucket 43 fills slot 6 to the plot top"
        );
        assert!(
            (bottom - pt_after[0].y - 40.0).abs() <= 0.5,
            "post-boundary: bucket 43 fills slot 0 to the plot top"
        );
        for (index, p) in pt_before.iter().enumerate() {
            if index != 6 {
                assert!(
                    (p.y - bottom).abs() <= 0.01,
                    "slot {index} flat pre-boundary"
                );
            }
        }
        for (index, p) in pt_after.iter().enumerate() {
            if index != 0 {
                assert!(
                    (p.y - bottom).abs() <= 0.01,
                    "slot {index} flat post-boundary"
                );
            }
        }
    }

    #[test]
    fn band_mount_paints_one_danger_polyline_peak_slot_57_quarter_lift_slot_54() {
        // Settled fixture (updated_ms 1_009_999, samples [(100,4),(97,1)]) ->
        // anchor 102: bucket 100 -> slot 57 (peak 4 -> plot top), bucket 97
        // -> slot 54 (a quarter of the 40 px plot -> 10 px of lift).
        let progress = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
        let samples: &[(i64, u64)] = &[(100, 4), (97, 1)];
        let shapes = render_band(samples, false, Some(&progress), 1_500_000);

        let lines = polylines(&shapes);
        assert_eq!(
            lines.len(),
            1,
            "exactly one chart: a single 60-point polyline"
        );
        let path = lines[0];
        assert!(
            matches!(path.stroke.color, egui::epaint::ColorMode::Solid(color) if color == theme::DANGER),
            "solid theme::DANGER stroke, got {:?}",
            path.stroke.color
        );
        assert!(
            (path.stroke.width - 1.8).abs() <= 0.01,
            "story-1 contractual 1.8px stroke"
        );
        assert_eq!(
            shapes
                .iter()
                .filter(|c| matches!(c.shape, egui::Shape::Path(_)))
                .count(),
            1,
            "no second chart on the band"
        );
        assert!(
            shapes
                .iter()
                .all(|c| !matches!(&c.shape, egui::Shape::Rect(rect) if rect.fill.a() > 0)),
            "no bar fills on the card band"
        );

        // The baseline serves as the coordinate oracle.
        let segs = baselines(&shapes);
        assert_eq!(segs.len(), 1, "exactly one baseline");
        let segment = segs[0];
        let (left, right, bottom) = (segment[0].x, segment[1].x, segment[0].y);

        let points = &path.points;
        // Plot height is exactly 40 px (48 px band minus the 2x4 px insets);
        // only the peak vertex climbs near the top.
        let elevated: Vec<usize> = (0..60).filter(|&i| points[i].y <= bottom - 39.0).collect();
        assert_eq!(elevated, [57], "only slot 57 reaches the plot top");
        let expected_x = left + (right - left) * 57.0 / 59.0;
        assert!(
            (points[57].x - expected_x).abs() <= 0.5,
            "peak x pins the slot-57 anchor placement"
        );
        assert!(
            (points[57].y - (bottom - 40.0)).abs() <= 0.5,
            "peak sits at the 40 px plot top"
        );
        assert!(
            (points[54].y - (bottom - 10.0)).abs() <= 0.5,
            "count-1 slot rises exactly 1/4 of the plot"
        );
        for (index, p) in points.iter().enumerate() {
            if index == 54 || index == 57 {
                continue;
            }
            assert!(
                (p.y - bottom).abs() <= 0.01,
                "slot {index} rests on the baseline"
            );
        }
    }

    #[test]
    fn degenerate_telemetry_yields_one_flat_danger_line_through_the_seam() {
        let stamped = settled_fixture(Some(1_009_999), &[(100, 4), (97, 1)]);
        let no_samples: &[(i64, u64)] = &[];
        let zero_buckets: &[(i64, u64)] = &[(5, 0), (6, 0)];
        let pre_window: &[(i64, u64)] = &[(1, 7)];
        let cases: [DegenerateCase<'_>; 4] = [
            (no_samples, None),             // (a) no LiveProgress record at all
            (no_samples, Some(&stamped)),   // (b) record present, samples = []
            (zero_buckets, Some(&stamped)), // (b) all-zero buckets
            (pre_window, Some(&stamped)),   // (c) samples strictly pre-window
        ];
        for (samples, progress) in cases {
            let shapes = render_band(samples, false, progress, 1_015_000);
            let lines = polylines(&shapes);
            assert_eq!(lines.len(), 1, "{samples:?}: exactly one 60-point polyline");
            let path = lines[0];
            assert!(
                matches!(path.stroke.color, egui::epaint::ColorMode::Solid(color) if color == theme::DANGER),
                "{samples:?}: still stroked solid theme::DANGER"
            );
            let segs = baselines(&shapes);
            assert_eq!(segs.len(), 1, "{samples:?}: exactly one border baseline");
            let bottom = segs[0][0].y;
            for (index, p) in path.points.iter().enumerate() {
                assert!(
                    p.x.is_finite() && p.y.is_finite(),
                    "{samples:?}: no NaN/infinite ordinate at slot {index}"
                );
                assert!(
                    (p.y - bottom).abs() <= 0.01,
                    "{samples:?}: slot {index} flat on the baseline — no fabricated spike"
                );
            }
            assert_eq!(
                shapes
                    .iter()
                    .filter(|c| matches!(c.shape, egui::Shape::Path(_)))
                    .count(),
                1,
                "{samples:?}: no additional spike-bearing shape"
            );
            assert!(
                shapes
                    .iter()
                    .all(|c| !matches!(&c.shape, egui::Shape::Rect(rect) if rect.fill.a() > 0)),
                "{samples:?}: the band still reserves its 48 px with no fills"
            );
        }
    }

    /// One degenerate-sweep case: the band's samples and the record it
    /// references (aliased to keep the case-table type out of clippy's
    /// type_complexity complaint range).
    type DegenerateCase<'a> = (&'a [(i64, u64)], Option<&'a crate::harness::LiveProgress>);

    #[test]
    fn band_legend_is_the_verbatim_four_phrase_metric_explanation() {
        assert_eq!(
            CARD_ACTIVITY_HOVER,
            "Updates per 10-second bucket · Last 10 minutes · Token usage is not reported · Empty buckets do not mean the worker stopped."
        );
        for phrase in [
            "Updates per 10-second bucket",
            "Last 10 minutes",
            "Token usage is not reported",
            "Empty buckets do not mean the worker stopped",
        ] {
            assert!(CARD_ACTIVITY_HOVER.contains(phrase), "missing: {phrase}");
        }
    }
}
