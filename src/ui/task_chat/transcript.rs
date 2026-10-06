use crate::domain::{ChatMessage, ChatRole};
use crate::ui::{Surface, theme};
use egui::RichText;
/// `follow_tail` mirrors the card's streaming state: while a reply streams,
/// the view chases the newest message (historical behaviour); once idle,
/// the history reads top-down so disclored earlier messages are never
/// stranded above the sticky bottom by taller content such as a lifted
/// digest block.
pub(crate) fn transcript(ui: &mut egui::Ui, messages: &[ChatMessage], follow_tail: bool) {
    transcript_with_max_height(ui, messages, follow_tail, 260.0);
}

pub(crate) fn transcript_with_max_height(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    follow_tail: bool,
    max_height: f32,
) {
    transcript_inner(ui, messages, follow_tail, max_height, true);
}

pub(crate) fn history_with_max_height(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    follow_tail: bool,
    max_height: f32,
) {
    transcript_inner(ui, messages, follow_tail, max_height, false);
}

fn transcript_inner(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    follow_tail: bool,
    max_height: f32,
    lift_open_ask: bool,
) {
    egui::ScrollArea::vertical()
        .max_height(max_height)
        .stick_to_bottom(follow_tail)
        .show(ui, |ui| {
            // Same one-lift-per-transcript rule as the main pane: the
            // freshest agent reply still owed an answer.
            let lift_at = lift_open_ask
                .then(|| crate::ui::reply_tail::open_ask_index(messages))
                .flatten();
            for (index, message) in messages.iter().enumerate() {
                let who = match message.role {
                    ChatRole::User => "You",
                    ChatRole::Agent => "Kool.ad/e",
                    ChatRole::System => "Update",
                };
                egui::Frame::NONE
                    .fill(if message.role == ChatRole::User {
                        theme::ACCENT_SOFT
                    } else {
                        theme::BG
                    })
                    .corner_radius(6)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(who).small().strong())
                            .on_hover_text(message.time_label());
                        let readable = crate::ui::message_text::readable(message);
                        if message.role == ChatRole::Agent {
                            // Agent prose renders as structured dark-theme
                            // Markdown; user and System lines keep today's
                            // plain label regime.
                            if Some(index) == lift_at {
                                // Lifted reply: the tail leaves the body and
                                // floats as its own distinct block below.
                                let tail =
                                    crate::ui::reply_tail::parse_reply_tail(readable.as_ref());
                                crate::ui::markdown::paint(
                                    ui,
                                    &tail.body,
                                    crate::ui::markdown::CHAT,
                                );
                                crate::ui::reply_tail::paint_open_ask(ui, &tail);
                            } else {
                                crate::ui::markdown::paint(
                                    ui,
                                    readable.as_ref(),
                                    crate::ui::markdown::CHAT,
                                );
                            }
                        } else {
                            ui.label(readable.as_ref());
                        }
                        if readable.as_ref() != message.text {
                            ui.push_id(&message.id, |ui| {
                                ui.collapsing("Response details", |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&message.text).monospace().small(),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        }
                    });
                ui.add_space(6.0);
            }
        });
}

/// Show the durable discussion without a second composer in task details.
pub fn paint_history(ui: &mut egui::Ui, s: &dyn Surface, key: &str) {
    let messages = s.task_messages(key);
    paint_history_messages(ui, messages, s.task_chat_active(key));
}

pub(crate) fn paint_history_messages(ui: &mut egui::Ui, messages: &[ChatMessage], active: bool) {
    if messages.is_empty() {
        ui.label("No discussion yet.");
    } else {
        transcript(ui, messages, active);
    }
}
