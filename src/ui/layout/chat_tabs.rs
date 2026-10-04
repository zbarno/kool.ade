use super::*;

#[derive(Clone, Default)]
pub(crate) struct ChatTabs {
    pub(crate) keys: Vec<String>,
    pub(crate) active: Option<String>,
    reveal_active: bool,
}

impl ChatTabs {
    pub(crate) fn open(&mut self, key: &str) {
        if !self.keys.iter().any(|existing| existing == key) {
            self.keys.push(key.to_owned());
        }
        self.active = Some(key.to_owned());
        self.reveal_active = true;
    }

    pub(crate) fn close(&mut self, key: &str) {
        self.keys.retain(|existing| existing != key);
        if self.active.as_deref() == Some(key) {
            self.active = None;
        }
    }
}

fn conversation_title(board: &crate::ui::planning_board::ViewModel, key: &str) -> String {
    board
        .planning_items
        .iter()
        .find(|item| item.conversation_key() == key)
        .map(|item| item.question.clone())
        .or_else(|| {
            board
                .planning_work
                .iter()
                .find(|w| w.key == key)
                .map(|w| w.title.clone())
        })
        .or_else(|| {
            board
                .task_documents
                .iter()
                .find(|doc| doc.path == key)
                .map(|doc| doc.title.clone())
        })
        .unwrap_or_else(|| key.to_owned())
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
) {
    let id = egui::Id::new("koolade_chat_tabs");
    let mut tabs = ui
        .ctx()
        .data_mut(|d| d.get_temp::<ChatTabs>(id).unwrap_or_default());
    egui::ScrollArea::horizontal()
        .id_salt("chat_tabs")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for key in tabs.keys.clone() {
                    ui.push_id(&key, |ui| {
                        let title = conversation_title(board, &key);
                        let label = board
                            .planning_items
                            .iter()
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
            s.dispatch(ApplicationCommand::PrepareTaskChat {
                key: key.to_owned(),
            });
            let context = s.task_chat_context(key);
            let title = conversation_title(board, key);
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
                ui.collapsing("Reply activity", |ui| {
                    crate::ui::chat_pane::paint_progress(ui, &progress)
                });
            }
            if let Some(error) = s.task_chat_error() {
                ui.colored_label(theme::WARNING, error);
                if ui.button("Retry saving conversation").clicked() {
                    s.dispatch(ApplicationCommand::RetryTaskChatSave);
                }
            }
            if let Some(draft) = s.task_draft(key) {
                let intent = crate::ui::chat_pane::paint_task_with_context(
                    ui,
                    &messages,
                    draft,
                    busy,
                    context.as_deref(),
                );
                if intent.send {
                    s.dispatch(ApplicationCommand::SendTaskReply {
                        key: key.to_owned(),
                    });
                }
                if intent.cancel && active {
                    s.dispatch(ApplicationCommand::CancelTaskReply {
                        key: key.to_owned(),
                    });
                }
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, tabs));
}
