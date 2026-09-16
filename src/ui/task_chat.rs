//! A task's next action comes first; the transcript remains available on demand.
use crate::domain::{Authority, ChatMessage, ChatRole, ItemStatus};
use crate::ui::{Surface, theme};
use egui::RichText;

#[derive(Default, Debug, PartialEq)]
struct Reply {
    summary: String,
    next: Option<String>,
    no_reply: bool,
}

fn split_reply(text: &str) -> Reply {
    let text = text.replace("**Your next step:**", "Your next step:");
    if let Some((summary, next)) = text.split_once("Your next step:") {
        return Reply {
            summary: summary.trim().into(),
            next: Some(next.trim().into()).filter(|s: &String| !s.is_empty()),
            no_reply: false,
        };
    }
    let no_reply = text.trim_end().ends_with("No reply needed.");
    let summary = text
        .trim_end()
        .trim_end_matches("No reply needed.")
        .trim()
        .to_owned();
    // Older replies have no marker. Surface an explicit final question verbatim.
    let next = (!no_reply)
        .then(|| summary.lines().rev().find(|line| !line.trim().is_empty()))
        .flatten()
        .filter(|line| line.trim().ends_with('?') && line.chars().count() <= 280)
        .map(|line| line.trim().to_string());
    let summary = if next.as_deref() == Some(summary.as_str()) {
        String::new()
    } else {
        summary
    };
    Reply {
        summary,
        next,
        no_reply,
    }
}

fn failed(messages: &[ChatMessage]) -> bool {
    messages
        .iter()
        .rev()
        .take_while(|m| m.role != ChatRole::User)
        .any(|m| {
            m.role == ChatRole::System
                && (m.text.starts_with("⚠ Turn rejected")
                    || m.text.starts_with("Planning stopped:")
                    || m.text.starts_with("Task generation needs attention:"))
        })
}

fn transcript(ui: &mut egui::Ui, messages: &[ChatMessage]) {
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for message in messages {
                let who = match message.role {
                    ChatRole::User => "You",
                    ChatRole::Agent => "Packet",
                    ChatRole::System => "Update",
                };
                ui.label(
                    RichText::new(format!("{who} · {}", message.time_label()))
                        .small()
                        .weak(),
                );
                let readable = crate::ui::message_text::readable(message);
                ui.label(readable.as_ref());
                if readable.as_ref() != message.text {
                    ui.push_id(&message.id, |ui| {
                        ui.collapsing("Response details", |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&message.text).monospace().small())
                                    .wrap(),
                            );
                        });
                    });
                }
                ui.add_space(6.0);
            }
        });
}

fn composer(ui: &mut egui::Ui, s: &mut dyn Surface, key: &str, expanded: bool, answer: bool) {
    let busy = s.task_reply_busy();
    let mut send = false;
    if let Some(draft) = s.task_draft(key) {
        let response = if expanded {
            ui.add(
                egui::TextEdit::multiline(draft)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text(if answer {
                        "Your answer…"
                    } else {
                        "Add a follow-up…"
                    }),
            )
        } else {
            ui.add(
                egui::TextEdit::singleline(draft)
                    .desired_width(f32::INFINITY)
                    .hint_text(if answer {
                        "Your answer…"
                    } else {
                        "Add a follow-up…"
                    }),
            )
        };
        let enter = if expanded {
            response.has_focus()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
        } else {
            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        };
        let enabled = !busy && !draft.trim().is_empty();
        let button = egui::Button::new(
            RichText::new(if answer { "Send answer" } else { "Send reply" }).strong(),
        )
        .fill(theme::ACCENT_SOFT);
        send = ui.add_enabled(enabled, button).clicked() || (enabled && enter);
        if expanded {
            ui.label(
                RichText::new(if busy {
                    "Waiting for the current update. Your draft is kept."
                } else {
                    "Ctrl / ⌘ + Enter to send"
                })
                .small()
                .weak(),
            );
        }
    }
    if send {
        s.send_task_reply(key);
    }
}

/// Returns true when the compact card requests the expanded discussion.
pub fn paint(ui: &mut egui::Ui, s: &mut dyn Surface, key: &str, expanded: bool) -> bool {
    let mut open = false;
    ui.push_id(("task_conversation", key, expanded), |ui| {
        let messages = s.task_messages(key).to_vec();
        let item = s
            .items()
            .iter()
            .chain(s.synthetic_items())
            .chain(s.resolved_items())
            .find(|item| item.conversation_key() == key)
            .cloned();
        let eligible = item.as_ref().is_none_or(|item| {
            crate::core::routing::eligible_items(
                std::slice::from_ref(item),
                s.current_user(),
                s.stakeholders(),
            )
            .len()
                == 1
        });
        let reply = messages
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::Agent)
            .map(|m| split_reply(&crate::ui::message_text::readable(m)))
            .unwrap_or_default();
        let active = s.task_chat_active(key);
        let retry = !active
            && (failed(&messages) || messages.last().is_some_and(|m| m.role == ChatRole::User));
        let resolved = item
            .as_ref()
            .is_some_and(|i| i.status == ItemStatus::Resolved);
        let ownership = !resolved && item.as_ref().is_some_and(|i| i.is_ownership_gap());
        let review = !resolved
            && item.as_ref().is_some_and(|i| {
                i.authority == Authority::Review
                    && i.feature_id.is_some()
                    && !i.recommendation.is_empty()
            });
        let needs_answer = !active
            && !retry
            && !resolved
            && !ownership
            && !review
            && eligible
            && !reply.no_reply
            && (reply.next.is_some()
                || item
                    .as_ref()
                    .is_some_and(|i| i.authority == Authority::Human));
        if expanded {
            ui.heading("Task conversation");
        }
        let (heading, action) = if active {
            (
                "Packet is replying",
                "You can keep drafting while you wait.".to_string(),
            )
        } else if retry {
            (
                "Update not saved",
                "Nothing changed. Retry your last reply or send a revised answer.".to_string(),
            )
        } else if resolved {
            ("Resolved", "No reply needed.".to_string())
        } else if ownership {
            (
                "Your next step",
                "Choose who owns this category.".to_string(),
            )
        } else if review {
            (
                "Your next step",
                "Review Packet’s recommendation and approve it or request a change.".to_string(),
            )
        } else if needs_answer {
            (
                "Your answer needed",
                reply
                    .next
                    .clone()
                    .unwrap_or_else(|| item.as_ref().unwrap().question.clone()),
            )
        } else if !eligible {
            (
                "Waiting on the owner",
                format!(
                    "Assigned to {}. You can still add context.",
                    item.as_ref()
                        .and_then(|i| i.assigned_to.as_deref())
                        .unwrap_or("another stakeholder")
                ),
            )
        } else if item
            .as_ref()
            .is_some_and(|i| i.authority == Authority::Agent)
        {
            (
                "Assigned to Packet",
                "No answer needed from you.".to_string(),
            )
        } else {
            ("No reply needed", "".to_string())
        };
        egui::Frame::NONE
            .fill(if needs_answer || ownership || review || retry {
                theme::ACCENT_SOFT
            } else {
                theme::BG
            })
            .corner_radius(6)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.label(RichText::new(heading).strong().color(if retry {
                    theme::WARNING
                } else {
                    theme::TEXT
                }));
                if !action.is_empty() && !item.as_ref().is_some_and(|item| item.question == action)
                {
                    ui.label(&action);
                }
                if active && ui.small_button("Stop reply").clicked() {
                    s.cancel_task_reply(key);
                }
                if ownership && ui.button("Assign ownership").clicked() {
                    s.on_header_action(crate::ui::HeaderAction::Stakeholders);
                }
                if review {
                    if expanded {
                        let item = item.as_ref().unwrap();
                        ui.label(&item.recommendation);
                        if ui
                            .add_enabled(
                                !s.task_reply_busy(),
                                egui::Button::new("Approve provisional decision"),
                            )
                            .clicked()
                        {
                            s.approve_review_item(&item.id);
                        }
                    } else if ui.button("Review decision").clicked() {
                        open = true;
                    }
                }
                if retry {
                    if let Some(previous) = messages.iter().rev().find(|m| m.role == ChatRole::User)
                    {
                        let empty = s
                            .task_draft(key)
                            .is_some_and(|draft| draft.trim().is_empty());
                        if ui
                            .add_enabled(
                                empty && !s.task_reply_busy(),
                                egui::Button::new("Retry last reply"),
                            )
                            .clicked()
                        {
                            if let Some(draft) = s.task_draft(key) {
                                *draft = previous.text.clone();
                            }
                            s.send_task_reply(key);
                        }
                    }
                }
            });
        if !reply.summary.is_empty() && !active {
            ui.label(RichText::new("Latest reply").small().weak());
            ui.label(crate::core::context_build::clip(
                &reply.summary,
                if expanded { 420 } else { 180 },
            ));
        }
        if let Some(error) = s.task_chat_error() {
            ui.colored_label(theme::WARNING, "Conversation has unsaved messages.");
            ui.label(RichText::new(error).small());
            if ui.button("Retry saving conversation").clicked() {
                s.retry_task_chat_save();
            }
        }
        if needs_answer || retry || expanded {
            composer(ui, s, key, expanded, needs_answer || retry);
        } else {
            let has_draft = s.task_draft(key).is_some_and(|draft| !draft.is_empty());
            egui::CollapsingHeader::new("Add context")
                .default_open(has_draft)
                .open(has_draft.then_some(true))
                .show(ui, |ui| composer(ui, s, key, false, false));
        }
        if expanded && !messages.is_empty() {
            ui.collapsing(format!("Conversation history ({})", messages.len()), |ui| {
                transcript(ui, &messages)
            });
        } else if !expanded && !review && ui.small_button("Open conversation").clicked() {
            open = true;
        }
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn next_step_is_separate_and_no_reply_is_not_a_request() {
        assert_eq!(
            split_reply("SSO is recorded.\nYour next step: Should guests use SSO too?"),
            Reply {
                summary: "SSO is recorded.".into(),
                next: Some("Should guests use SSO too?".into()),
                no_reply: false
            }
        );
        let reply = split_reply("SSO and MFA are confirmed.\nNo reply needed.");
        assert!(reply.no_reply);
        assert_eq!(reply.summary, "SSO and MFA are confirmed.");
        assert_eq!(reply.next, None);
    }
}
