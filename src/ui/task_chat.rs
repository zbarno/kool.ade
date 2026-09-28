//! A task's next action comes first; the transcript remains available on demand.
//! CHG-003 story 5 offers tappable option chips in the answer-needed frame
//! for digest bullets carrying recognisable options: a tap joins the full
//! choice text into the task draft (newline-joined expanded, space-joined
//! compact) without sending, and the card composer's caret pins at its end.
use crate::domain::{Authority, ChatMessage, ChatRole, ItemStatus};
use crate::ui::{ApplicationCommand, Surface, theme};
use egui::RichText;
mod decision;

#[derive(Default, Debug, PartialEq)]
struct Reply {
    summary: String,
    next: Option<String>,
    no_reply: bool,
}

fn split_reply(text: &str) -> Reply {
    // Delegated to the shared tail classifier; the mapping onto the legacy
    // struct is the byte-for-byte compatibility contract (asserted in tests).
    let tail = crate::ui::reply_tail::parse_reply_tail(text);
    Reply {
        summary: tail.body,
        next: tail.ask,
        no_reply: tail.no_reply,
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

/// Conversation participation is durable progress, not implementation completion.
/// Explicit review, blocker and terminal states take precedence over discussion.
pub(crate) fn board_column(base: usize, messages: &[ChatMessage], active: bool) -> usize {
    if base >= 2 {
        return base;
    }
    if active {
        return 1;
    }
    if failed(messages) || messages.last().is_some_and(|m| m.role == ChatRole::User) {
        return 3;
    }
    if messages.iter().any(|m| m.role == ChatRole::User) {
        return 1;
    }
    base
}

/// `follow_tail` mirrors the card's streaming state: while a reply streams,
/// the view chases the newest message (historical behaviour); once idle,
/// the history reads top-down so disclored earlier messages are never
/// stranded above the sticky bottom by taller content such as a lifted
/// digest block.
fn transcript(ui: &mut egui::Ui, messages: &[ChatMessage], follow_tail: bool) {
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .stick_to_bottom(follow_tail)
        .show(ui, |ui| {
            // Same one-lift-per-transcript rule as the main pane: the
            // freshest agent reply still owed an answer.
            let lift_at = crate::ui::reply_tail::open_ask_index(messages);
            for (index, message) in messages.iter().enumerate() {
                let who = match message.role {
                    ChatRole::User => "You",
                    ChatRole::Agent => "Packet",
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

fn composer(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    key: &str,
    expanded: bool,
    answer: bool,
    chip_fired: bool,
) {
    let busy = s.task_chat_active(key);
    let mut send = false;
    if let Some(draft) = s.task_draft(key) {
        let salt = if answer {
            "task_answer_composer"
        } else {
            "task_context_composer"
        };
        let response = if expanded {
            egui::ScrollArea::vertical()
                .id_salt((key, salt, "draft_scroll"))
                .max_height(88.0)
                .show(ui, |ui| {
                    egui::TextEdit::multiline(draft)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text(if answer {
                            "Your answer…"
                        } else {
                            "Add a follow-up…"
                        })
                        .id_salt(salt)
                        .show(ui)
                })
                .inner
        } else {
            egui::TextEdit::singleline(draft)
                .desired_width(f32::INFINITY)
                .hint_text(if answer {
                    "Your answer…"
                } else {
                    "Add a follow-up…"
                })
                .id_salt(salt)
                .show(ui)
        };
        if chip_fired {
            // A chip tap landed this frame: park the caret behind the
            // freshly joined option and hand the box back.
            crate::ui::reply_tail::pin_caret_to_end(&response, ui.ctx(), draft);
        }
        let enter = if expanded {
            response.response.has_focus()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
        } else {
            response.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        };
        let enabled = !busy && !draft.trim().is_empty();
        let button = egui::Button::new(
            RichText::new(if answer { "Send answer" } else { "Send reply" }).strong(),
        )
        .fill(theme::ACCENT_SOFT);
        send = ui
            .add_enabled(enabled, button)
            .on_hover_text(if busy {
                "Another update is running. Your draft stays here until you can send it."
            } else if expanded {
                "Send to this task only · Ctrl / ⌘ + Enter"
            } else {
                "Send to this task only · Enter"
            })
            .clicked()
            || (enabled && enter);
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
        s.dispatch(ApplicationCommand::SendTaskReply {
            key: key.to_owned(),
        });
    }
}

/// Returns true when the compact card requests the expanded discussion.
pub fn paint(ui: &mut egui::Ui, s: &mut dyn Surface, key: &str, expanded: bool) -> bool {
    let board = s.planning_board();
    paint_with_board(ui, s, key, expanded, &board)
}

pub(crate) fn paint_with_board(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    key: &str,
    expanded: bool,
    board: &crate::ui::planning_board::ViewModel,
) -> bool {
    let mut open = false;
    // Flipped once per frame by a claimed chip tap; the frame's composer
    // pins the caret behind the joined option exactly in that frame.
    let mut chip_fired = false;
    ui.push_id(("task_conversation", key, expanded), |ui| {
        let messages = s.task_messages(key).to_vec();
        let item = board
            .planning_items
            .iter()
            .find(|item| item.conversation_key() == key)
            .cloned();
        let eligible = item
            .as_ref()
            .is_none_or(|item| board.eligible_item_ids.contains(&item.id));
        let blocked_by = item
            .as_ref()
            .map(|item| {
                item.blocked_by
                    .iter()
                    .filter(|dependency| {
                        board.planning_items.iter().any(|candidate| {
                            candidate.id == dependency.as_str()
                                && candidate.status == ItemStatus::Open
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let blocked = !blocked_by.is_empty();
        let latest_reply = messages
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::Agent);
        let introduction = latest_reply.is_some_and(|m| m.id.starts_with("task-introduction:"));
        let reply = latest_reply
            .map(|m| split_reply(&crate::ui::message_text::readable(m)))
            .unwrap_or_default();
        let structured_options = item.as_ref().is_some_and(|item| {
            item.authority == Authority::Human
                && item
                    .decision_brief
                    .as_ref()
                    .is_some_and(|brief| !brief.options.is_empty())
        });
        let active = s.task_chat_active(key);
        let retry = !active
            && (failed(&messages) || messages.last().is_some_and(|m| m.role == ChatRole::User));
        let resolved = item
            .as_ref()
            .is_some_and(|i| i.status == ItemStatus::Resolved);
        let ownership = !resolved && item.as_ref().is_some_and(|i| i.is_ownership_gap());
        let review = !resolved
            && eligible
            && item.as_ref().is_some_and(|i| {
                i.authority == Authority::Review
                    && i.feature_id.is_some()
                    && !i.recommendation.is_empty()
            });
        let implementation = s.implementation_state(key).cloned();
        let implementing = s.implementation_active(key);
        let story = item.is_none()
            && crate::artifacts::layout::ArtifactLayout::is_task_ticket_path(key);
        let implementation_column =
            crate::core::implementation::board_column(implementation.as_ref(), implementing);
        let needs_answer = !active
            && !retry
            && !resolved
            && !ownership
            && !review
            && !(story && (implementing || implementation_column >= 2))
            && eligible
            && !reply.no_reply
            && ((reply.next.is_some()
                && (!introduction || latest_reply.is_some_and(|m| m.text.contains("\n---\n"))))
                || item
                    .as_ref()
                    .is_some_and(|i| i.authority == Authority::Human));
        let (heading, action) = if active {
            (
                if expanded { "Answer sent — Packet is updating this task" } else { "Packet is replying…" },
                "Your answer is saved. You can keep drafting while you wait.".to_string(),
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
        } else if blocked {
            (
                "Waiting for a prerequisite",
                format!("Resolve {} first.", blocked_by.join(", ")),
            )
        } else if needs_answer {
            (
                "Your answer needed",
                reply
                    .next
                    .clone()
                    .unwrap_or_else(|| item.as_ref().unwrap().question.clone()),
            )
        } else if story {
            match implementation_column {
                4 => ("Completed", "No action needed.".to_string()),
                2 => (
                    "Ready for review",
                    "Review the changes in the pull request.".to_string(),
                ),
                3 => (
                    "Needs attention",
                    implementation
                        .as_ref()
                        .map(|r| crate::core::context_build::clip(&r.detail, 240))
                        .unwrap_or_default(),
                ),
                1 => (
                    "Packet is working",
                    "Implementation is running. Activity is available below.".to_string(),
                ),
                _ => (
                    "Ready to implement",
                    "Start implementation, or add context before Packet begins.".to_string(),
                ),
            }
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
        if expanded && !reply.summary.is_empty() && !active && !introduction {
            ui.label(RichText::new("Latest update").small().weak());
            ui.label(crate::core::context_build::clip(&reply.summary, 420));
        }
        if expanded && !active
            && let Some(previous) = messages.iter().rev().find(|m| m.role == ChatRole::User) {
                ui.label(RichText::new("Your last answer").small().weak());
                ui.label(crate::core::context_build::clip(&previous.text, 240));
            }
        egui::Frame::NONE
            .fill(if needs_answer || ownership || review || retry {
                theme::ACCENT_SOFT
            } else if !expanded {
                egui::Color32::TRANSPARENT
            } else {
                theme::BG
            })
            .corner_radius(6)
            .inner_margin(if expanded || needs_answer || ownership || review || retry { 8 } else { 0 })
            .show(ui, |ui| {
                ui.label(RichText::new(heading).strong().color(if retry {
                    theme::WARNING
                } else {
                    theme::TEXT
                }));
                if !action.is_empty() && (expanded || needs_answer || retry || blocked)
                    && !item.as_ref().is_some_and(|item| item.question == action)
                {
                    ui.label(&action);
                }
                if !active
                    && (needs_answer || expanded && eligible)
                    && let Some(item) = &item
                {
                    decision::summary(ui, item, expanded);
                }
                if needs_answer
                    && !active
                    && let Some(item) = &item
                    && decision::choices(ui, s, key, item, expanded, active)
                {
                    chip_fired = true;
                }
                // CHG-003 story 5: chips remain visible while a reply is
                // in flight, but are rendered as inert controls until it
                // settles. A settled/answered card has no open choices.
                let choices = if structured_options {
                    Vec::new()
                } else {
                    crate::ui::reply_tail::open_digest_choices(&messages)
                };
                if let Some(hit) =
                    crate::ui::reply_tail::paint_chip_row(ui, &choices, !s.task_chat_active(key))
                    && let Some(draft) = s.task_draft(key)
                {
                    crate::ui::reply_tail::join_choice(
                        draft,
                        &choices[hit],
                        if expanded { '\n' } else { ' ' },
                    );
                    chip_fired = true;
                }
                if active && ui.small_button("Stop reply").clicked() {
                    s.dispatch(ApplicationCommand::CancelTaskReply {
                        key: key.to_owned(),
                    });
                }
                if expanded && active
                    && let Some(previous) = messages.iter().rev().find(|m| m.role == ChatRole::User)
                    {
                        ui.label(RichText::new("Your answer").small().weak());
                        ui.label(crate::core::context_build::clip(&previous.text, 400));
                    }
                let actions = s.feature_actions(Some(key));
                if let Some((id, _)) = super::feature_approval::paint(ui, &actions, s.conversation_busy()) {
                    s.dispatch(ApplicationCommand::ApproveFeature { id });
                }
                if ownership && ui.button("Assign ownership").clicked() {
                    s.dispatch(ApplicationCommand::HeaderAction(
                        crate::ui::HeaderAction::Stakeholders,
                    ));
                }
                if review {
                    if expanded {
                        let item = item.as_ref().unwrap();
                        if !item.recommendation.trim().is_empty() {
                            ui.label(&item.recommendation);
                        }
                        if ui
                            .add_enabled(
                                !s.task_chat_active(key),
                                egui::Button::new("Approve provisional decision"),
                            )
                            .clicked()
                        {
                            s.dispatch(ApplicationCommand::ApproveReviewItem {
                                id: item.id.clone(),
                            });
                        }
                    } else if ui.button("Review decision").clicked() {
                        open = true;
                    }
                }
                if retry
                    && let Some(previous) = messages.iter().rev().find(|m| m.role == ChatRole::User)
                    {
                        let empty = s
                            .task_draft(key)
                            .is_some_and(|draft| draft.trim().is_empty());
                        if ui
                            .add_enabled(
                                empty && !s.task_chat_active(key),
                                egui::Button::new("Retry last reply"),
                            )
                            .clicked()
                        {
                            if let Some(draft) = s.task_draft(key) {
                                *draft = previous.text.clone();
                            }
                            s.dispatch(ApplicationCommand::SendTaskReply {
                                key: key.to_owned(),
                            });
                        }
                    }
                if needs_answer || retry {
                    composer(ui, s, key, expanded, true, chip_fired);
                }
            });
        if !expanded && !reply.summary.is_empty() && !active {
            ui.label(RichText::new(crate::core::context_build::clip(&reply.summary, 120)).small())
                .on_hover_text("Open this task for the complete conversation.");
        }
        if let Some(error) = s.task_chat_error() {
            ui.colored_label(theme::WARNING, "Conversation has unsaved messages.");
            ui.label(RichText::new(error).small());
            if ui.button("Retry saving conversation").clicked() {
                s.dispatch(ApplicationCommand::RetryTaskChatSave);
            }
        }
        // A compact Kanban card only owns an input while Packet is explicitly
        // asking for one. Free-form context remains available after opening
        // the task's full conversation.
        if expanded && review {
            ui.label(RichText::new("Suggest a different choice or add context").small().strong());
            composer(ui, s, key, expanded, false, false);
        }
        if expanded && !needs_answer && !retry && !review && !blocked {
            let has_draft = s.task_draft(key).is_some_and(|draft| !draft.is_empty());
            egui::CollapsingHeader::new("Add context")
                .default_open(has_draft)
                .open(has_draft.then_some(true))
                .show(ui, |ui| composer(ui, s, key, expanded, false, false));
        }
        if expanded && !messages.is_empty() {
            ui.collapsing(format!("Conversation history ({})", messages.len()), |ui| {
                transcript(ui, &messages, active)
            });
        } else if !expanded && !review {
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Open conversation").on_hover_text("Continue this same conversation with the task context and full history. Your draft comes with you.").clicked() {
                    open = true;
                }
                if !messages.is_empty() {
                    ui.label(RichText::new(format!("{} messages", messages.len())).small().weak());
                }
            });
        }
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_roles_gate_markdown_like_the_main_pane() {
        const MD: &str = "Card says **boldcard** now.";
        let messages = vec![
            ChatMessage::new(ChatRole::Agent, MD, None),
            ChatMessage::new(ChatRole::User, MD, None),
            ChatMessage::new(ChatRole::System, MD, None),
        ];
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| transcript(ui, &messages, false));
        });
        let texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some((*shape.galley).text().to_owned()),
                _ => None,
            })
            .collect();
        let bearing = texts.iter().filter(|t| t.contains("boldcard")).count();
        assert_eq!(bearing, 3, "each role paints its body once: {texts:?}");
        let scrubbed = texts
            .iter()
            .filter(|t| t.contains("boldcard") && !t.contains('*'))
            .count();
        assert_eq!(
            scrubbed, 1,
            "only the agent entry renders Markdown: {texts:?}"
        );
        output.textures_delta.clear();
    }

    #[test]
    fn conversation_progress_preserves_explicit_lifecycle_states() {
        let mut messages = vec![];
        assert_eq!(board_column(0, &messages, false), 0);
        messages.push(ChatMessage::new(ChatRole::User, "Use SSO", None));
        assert_eq!(board_column(0, &messages, true), 1);
        assert_eq!(board_column(0, &messages, false), 3); // Interrupted reply.
        messages.push(ChatMessage::new(
            ChatRole::Agent,
            "Recorded. Anything else?",
            None,
        ));
        assert_eq!(board_column(0, &messages, false), 1);
        // Saved history retains progress after a restart, without marking Done.
        let restored: Vec<ChatMessage> =
            serde_json::from_str(&serde_json::to_string(&messages).unwrap()).unwrap();
        assert_eq!(board_column(0, &restored, false), 1);
        for base in [2, 3, 4] {
            assert_eq!(board_column(base, &restored, false), base);
            assert_eq!(board_column(base, &restored, true), base);
        }
        messages.push(ChatMessage::new(
            ChatRole::System,
            "Planning stopped: provider unavailable",
            None,
        ));
        assert_eq!(board_column(0, &messages, false), 3);
        assert_eq!(board_column(0, &messages, true), 1); // Retrying.
        messages.push(ChatMessage::new(ChatRole::User, "Try again", None));
        messages.push(ChatMessage::new(ChatRole::Agent, "Recorded.", None));
        assert_eq!(board_column(0, &messages, false), 1);
    }

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

    #[test]
    fn adapter_maps_parsed_tails_bit_for_bit_onto_the_legacy_reply() {
        // Compatibility contract: every legacy shape must come back through
        // the shared classifier as exactly today's tuple.
        for input in [
            "SSO is recorded.\nYour next step: Should guests use SSO too?",
            "SSO is recorded.\n**Your next step:** Should guests use SSO too?",
            "Done here.\nYour next step:",
            "Your next step:",
            "SSO and MFA are confirmed.\nNo reply needed.",
            "Anything else?",
            "Patch landed.\nDeploy tonight?",
            "Quiet workday.",
        ] {
            let tail = crate::ui::reply_tail::parse_reply_tail(input);
            assert_eq!(
                split_reply(input),
                Reply {
                    summary: tail.body,
                    next: tail.ask,
                    no_reply: tail.no_reply,
                },
                "legacy parity drifted for {input:?}"
            );
        }
        // The unreleased digest shape maps body → summary and the first
        // bullet → next, landing 1:1 on the card 'Your answer needed' line.
        let input = "Draft ready.\n\n---\n- Enable SSO for all guests?\n- Recommended: yes, effective Monday.";
        let tail = crate::ui::reply_tail::parse_reply_tail(input);
        assert_eq!(tail.kind, crate::ui::reply_tail::TailKind::Digest);
        let reply = split_reply(input);
        assert_eq!(
            reply,
            Reply {
                summary: tail.body.clone(),
                next: tail.ask.clone(),
                no_reply: tail.no_reply,
            }
        );
        assert_eq!(reply.summary, "Draft ready.");
        assert_eq!(reply.next.as_deref(), Some(tail.bullets[0].as_str()));
        assert_eq!(reply.next.as_deref(), Some("Enable SSO for all guests?"));
        assert!(!reply.no_reply);
    }

    /// Transcript lift (test plan 4b): a legacy 'Your next step:' final reply
    /// floats its question verbatim and strongly inside the card transcript,
    /// leaves no marker in any galley, and retires the moment a user answer
    /// lands.
    #[test]
    fn transcript_lifts_legacy_asks_verbatim_and_retires_them_on_answer() {
        const LEGACY: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
        const QUESTION: &str = "Should guests use SSO too?";
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());

        fn backdrops(output: &egui::FullOutput) -> usize {
            output
                .shapes
                .iter()
                .filter(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) => rect.fill == theme::DIGEST_BG,
                    _ => false,
                })
                .count()
        }
        fn texts_of(output: &egui::FullOutput) -> Vec<String> {
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) => Some((*shape.galley).text().to_owned()),
                    _ => None,
                })
                .collect()
        }
        fn strong_spot(output: &egui::FullOutput, needle: &str) -> Option<bool> {
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) => Some(&*shape.galley),
                    _ => None,
                })
                .find(|g| g.text() == needle)
                .map(|g| {
                    let section = g
                        .job
                        .sections
                        .iter()
                        .find(|s| s.byte_range.end.0 > s.byte_range.start.0)
                        .expect("non-empty section");
                    section.format.extra_letter_spacing.abs() > 1e-9
                })
        }

        let user = ChatMessage::new(ChatRole::User, "Set up SSO.", None);
        let agent = ChatMessage::new(ChatRole::Agent, LEGACY, None);

        // Open: the question floats verbatim, unlabeled and strong.
        let open_log = vec![user.clone(), agent.clone()];
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| transcript(ui, &open_log, false));
        });
        assert_eq!(
            backdrops(&output),
            1,
            "legacy ask must float in the transcript"
        );
        let texts = texts_of(&output);
        assert_eq!(
            texts.iter().filter(|t| *t == QUESTION).count(),
            1,
            "question lifts verbatim exactly once: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("Your next step:")),
            "marker leaked: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("SSO is recorded.")),
            "transcript body must still render: {texts:?}"
        );
        assert!(
            strong_spot(&output, QUESTION).unwrap_or(false),
            "lifted row prints strongly"
        );
        output.textures_delta.clear();

        // Answered: the lift disappears entirely.
        let answered_log = vec![
            user,
            agent,
            ChatMessage::new(ChatRole::User, "Yes, guests included.", None),
        ];
        let mut after = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| transcript(ui, &answered_log, false));
        });
        assert_eq!(backdrops(&after), 0, "a user answer must retire the lift");
        let after_texts = texts_of(&after);
        assert_eq!(
            after_texts.iter().filter(|t| *t == QUESTION).count(),
            0,
            "no lifted row may survive the answer: {after_texts:?}"
        );
        after.textures_delta.clear();
    }

    // ------------------------------------------------------------------
    // CHG-003 story 5: tappable option chips on the answer-needed card.
    // ------------------------------------------------------------------

    use crate::domain::item::{ItemKind, OpenItem, Priority};
    use crate::domain::user::CurrentUser;
    use crate::ui::ToastQueue;

    const CARD_DIGEST: &str = "Ready.\n\n---\n\
- Which vendor shall we bind?\n\
- Yes, bind Aurora effective Monday.\n\
- No, keep Postman.";

    /// Minimal Surface probe: an eligible Human-question item `CLR-001`
    /// (category General → broadcast-eligible to the seated operator) with
    /// controllable task transcript, draft, busyness and send accounting.
    #[derive(Default)]
    struct CardProbe {
        items: Vec<OpenItem>,
        messages: Vec<ChatMessage>,
        main: Vec<ChatMessage>,
        draft: String,
        draft_present: bool,
        busy: bool,
        sent: u32,
        user: CurrentUser,
        stakes: crate::domain::Stakeholders,
        toasts: ToastQueue,
    }

    impl CardProbe {
        /// The standard open answer card: an eligible question whose latest
        /// agent reply ends in a two-choice digest.
        fn answer_card(question: &str, digest: &str) -> Self {
            Self {
                items: vec![OpenItem::new(
                    "CLR-001".to_string(),
                    Priority::High,
                    ItemKind::Question,
                    "General".to_string(),
                    None,
                    question.to_string(),
                    "Vendor selection".to_string(),
                )],
                messages: vec![
                    ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
                    ChatMessage::new(ChatRole::Agent, digest, None),
                ],
                user: CurrentUser::new("Operator", vec![]),
                draft_present: true,
                ..Self::default()
            }
        }
    }

    impl Surface for CardProbe {
        fn session_title(&self) -> &str {
            "card probe"
        }
        fn is_git_repo(&self) -> bool {
            false
        }
        fn git_branch(&self) -> &str {
            ""
        }
        fn git_head(&self) -> &str {
            ""
        }
        fn git_dirty(&self) -> bool {
            false
        }
        fn chat_messages(&self) -> &[ChatMessage] {
            &self.main
        }
        fn chat_draft(&mut self) -> &mut String {
            &mut self.draft
        }
        fn is_busy(&self) -> bool {
            self.busy
        }
        fn conversation_busy(&self) -> bool {
            self.busy
        }
        fn task_chat_active(&self, _key: &str) -> bool {
            self.busy
        }
        fn task_progress(&self, _ticket: &str) -> Option<&crate::harness::LiveProgress> {
            None
        }
        fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief> {
            None
        }
        fn implementation_state(
            &self,
            _ticket: &str,
        ) -> Option<&crate::core::implementation::Implementation> {
            None
        }
        fn task_detail_view(&mut self, _ticket: &str) -> Option<crate::ui::task_detail::ViewModel> {
            None
        }
        fn implementation_active(&self, _ticket: &str) -> bool {
            false
        }
        fn auto_plan(&self) -> bool {
            false
        }
        fn auto_build(&self) -> bool {
            false
        }
        fn auto_publish(&self) -> bool {
            false
        }
        fn require_independent_checks(&self) -> bool {
            false
        }
        fn queue_status(&self) -> &str {
            ""
        }
        fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
            None
        }
        fn planning_board(&self) -> crate::ui::planning_board::ViewModel {
            crate::ui::planning_board::ViewModel {
                planning_items: self.items.clone(),
                eligible_item_ids: crate::core::routing::eligible_items(
                    &self.items,
                    &self.user,
                    &self.stakes,
                )
                .iter()
                .map(|item| item.id.clone())
                .collect(),
                ..Default::default()
            }
        }
        fn next_question_id(&self) -> Option<&str> {
            None
        }
        fn spec_text(&self) -> &str {
            ""
        }
        fn toasts(&mut self) -> &mut ToastQueue {
            &mut self.toasts
        }
        fn dispatch(&mut self, command: ApplicationCommand) {
            if matches!(command, ApplicationCommand::SendTaskReply { .. }) {
                self.sent += 1;
            }
        }
        // The card-under-test hooks.
        fn task_messages(&self, _key: &str) -> &[ChatMessage] {
            &self.messages
        }
        fn task_draft(&mut self, key: &str) -> Option<&mut String> {
            (key == "CLR-001" && self.draft_present).then_some(&mut self.draft)
        }
    }

    /// (rect, stroke colour) pairs for every chip-styled cell on the frame.
    fn card_chip_cells(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Color32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect)
                    if rect.fill == theme::CHIP_FILL
                        && rect.corner_radius == egui::CornerRadius::same(10_u8) =>
                {
                    Some((rect.rect, rect.stroke.color))
                }
                _ => None,
            })
            .collect()
    }

    /// Centre of the text shape whose full text equals `needle`.
    fn card_point(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(shape),
                _ => None,
            })
            .find(|shape| shape.galley.text() == needle)
            .map(|shape| shape.pos + shape.galley.mesh_bounds.center().to_vec2())
    }

    fn card_has_text(output: &egui::FullOutput, needle: &str) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) => shape.galley.text().contains(needle),
            _ => false,
        })
    }

    /// Walk-order position among TEXT galleys, for nesting assertions.
    fn card_walk_index(output: &egui::FullOutput, needle: &str) -> Option<usize> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(&**shape.galley),
                _ => None,
            })
            .position(|text| text == needle)
    }

    /// Runs one card frame with the probe; returns the frame output.
    fn card_frame(
        ctx: &egui::Context,
        probe: &mut CardProbe,
        expanded: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = paint(ui, probe, "CLR-001", expanded);
                });
            },
        )
    }

    fn press_events(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]
    }

    fn release_events(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]
    }

    #[test]
    fn human_decision_card_shows_consequences_recommendation_and_freeform_reply() {
        let mut probe = CardProbe::answer_card("Choose a retention period", "");
        probe.items[0].authority = Authority::Human;
        probe.items[0].decision_brief = Some(crate::domain::DecisionBrief {
            id: "CLR-001".into(),
            question: "Choose a retention period".into(),
            why_now: "The launch must settle how long saved information stays available.".into(),
            recommendation: Some(crate::domain::DecisionRecommendation {
                option_id: "short".into(),
                rationale: "A 30-day limit reduces how much old information remains available if an account is misused.".into(),
            }),
            confidence: Some(crate::domain::DecisionConfidence {
                level: crate::domain::ConfidenceLevel::Medium,
                explanation: "We know records are stored, but the right period depends on your rules."
                    .into(),
            }),
            options: vec![
                crate::domain::DecisionOption {
                    id: "short".into(),
                    label: "Keep records for 30 days".into(),
                    summary: "Remove older records automatically.".into(),
                    benefits: vec![],
                    costs: vec![],
                    risks: vec![],
                    consequences: vec!["Information older than 30 days is removed automatically.".into()],
                    reversibility: "You can change how long information is kept.".into(),
                },
                crate::domain::DecisionOption {
                    id: "all".into(),
                    label: "Keep records until deleted".into(),
                    summary: "Users remove records themselves.".into(),
                    benefits: vec![],
                    costs: vec![],
                    risks: vec![],
                    consequences: vec!["Information remains available until you delete it.".into()],
                    reversibility: "You can add an automatic time limit later.".into(),
                },
            ],
            benefits: vec![],
            costs: vec![],
            risks: vec![],
            ramifications: vec![],
            reversibility: "You can change how long information is kept.".into(),
            defer_consequence: "The launch cannot proceed until a retention rule is chosen.".into(),
            evidence: vec![],
            adr_assessment: None,
        });
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut output = card_frame(&ctx, &mut probe, false, Vec::new());
        for content in [
            "The launch must settle how long saved information stays available.",
            "Packet recommends",
            "Keep records for 30 days",
            "A 30-day limit reduces how much old information remains available if an account is misused.",
            "Information older than 30 days is removed automatically.",
            "Information remains available until you delete it.",
            "Confidence: Medium",
            "We know records are stored, but the right period depends on your rules.",
            "Can this change later? You can change how long information is kept.",
            "If you wait: The launch cannot proceed until a retention rule is chosen.",
            "Send answer",
        ] {
            assert!(card_has_text(&output, content), "missing {content}");
        }
        let choice = card_point(&output, "Keep records until deleted").unwrap();
        card_frame(&ctx, &mut probe, false, press_events(choice))
            .textures_delta
            .clear();
        card_frame(&ctx, &mut probe, false, release_events(choice))
            .textures_delta
            .clear();
        assert_eq!(probe.sent, 0, "a selection remains an explicit draft");
        assert!(probe.draft.contains("I choose option (all)"));
        output.textures_delta.clear();
    }

    #[test]
    fn dependent_human_item_shows_its_prerequisite_without_an_answer_control() {
        let mut probe = CardProbe::answer_card("Choose recovery behavior", "");
        let parent = OpenItem::new(
            "CLR-010".into(),
            Priority::High,
            ItemKind::Question,
            "General".into(),
            None,
            "Choose an account model".into(),
            String::new(),
        );
        let mut dependent = OpenItem::new(
            "CLR-001".into(),
            Priority::High,
            ItemKind::Question,
            "General".into(),
            None,
            "Choose recovery behavior".into(),
            String::new(),
        );
        dependent.blocked_by = vec![parent.id.clone()];
        probe.items = vec![parent, dependent];
        probe.messages.clear();
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut output = card_frame(&ctx, &mut probe, false, Vec::new());
        assert!(card_has_text(&output, "Waiting for a prerequisite"));
        assert!(card_has_text(&output, "Resolve CLR-010 first."));
        assert!(!card_has_text(&output, "Your answer needed"));
        assert!(!card_has_text(&output, "Send answer"));
        output.textures_delta.clear();
    }

    /// AC1/AC3: the compact answer-needed card keeps its heading, action and
    /// Send-answer affordance UNTOUCHED while the two chips nestle inside the
    /// ACCENT_SOFT frame BETWEEN the action and the composer; a tap on the
    /// second chip space-joins its FULL text into the task draft and never
    /// invokes send_task_reply.
    #[test]
    fn collapsed_card_nested_chips_spacejoin_the_tapped_option_into_the_task_draft() {
        let choices = crate::ui::reply_tail::digest_choices(
            &crate::ui::reply_tail::parse_reply_tail(CARD_DIGEST),
        );
        assert_eq!(choices.len(), 2, "the two bullets offer options");
        let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);
        let no_label = crate::ui::reply_tail::choice_label(&choices[1]);

        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut probe = CardProbe::answer_card(
            "Binding vendor for the checkout rollout", // ≠ action → action label paints
            CARD_DIGEST,
        );
        probe.draft = "Short answer:".to_string();

        let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
        let heading_spot = card_walk_index(&out, "Your answer needed")
            .expect("the answer-needed heading survives");
        let action_spot =
            card_walk_index(&out, "Which vendor shall we bind?").expect("the action row survives");
        let send_spot = card_walk_index(&out, "Send answer").expect("the send affordance survives");
        let yes_spot = card_walk_index(&out, &yes_label).expect("chip one projects");
        let no_spot = card_walk_index(&out, &no_label).expect("chip two projects");
        // Nesting: action, THEN chips, THEN the composer/send.
        assert!(
            action_spot < yes_spot && yes_spot < no_spot && no_spot < send_spot,
            "chips interleave action → chips → composer ({} {} {})",
            action_spot,
            yes_spot,
            send_spot
        );
        assert!(heading_spot < action_spot, "heading leads the frame");
        let cells = card_chip_cells(&out);
        assert_eq!(cells.len(), 2, "radius-10 CHIP_FILL cells, one per choice");
        assert!(
            cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
            "resting strokes"
        );
        let no_point = card_point(&out, &no_label).expect("chip two centre");
        out.textures_delta.clear();

        // Gesture: move + press, then release — mirroring the pane e2e.
        card_frame(&ctx, &mut probe, false, press_events(no_point))
            .textures_delta
            .clear();
        card_frame(&ctx, &mut probe, false, release_events(no_point))
            .textures_delta
            .clear();

        assert_eq!(
            probe.sent, 0,
            "a chip tap must never invoke send_task_reply"
        );
        assert_eq!(probe.messages.len(), 2, "no reply began on the task lane");
        assert_eq!(
            probe.draft, "Short answer: No, keep Postman.",
            "compact card space-joins the FULL chosen text"
        );

        // DoD caret proof on the card composer: the focus request settles on
        // the next frame, and a keystroke afterwards must LAND BEHIND the
        // inserted option (caret pinned at the drafted end, not a stale
        // midpoint).
        card_frame(&ctx, &mut probe, false, Vec::new())
            .textures_delta
            .clear();
        card_frame(
            &ctx,
            &mut probe,
            false,
            vec![egui::Event::Text("x".to_string())],
        )
        .textures_delta
        .clear();
        assert_eq!(
            probe.draft, "Short answer: No, keep Postman.x",
            "the post-tap keystroke landed at the pinned END of the draft"
        );
    }

    /// AC1 expanded mode: the multiline task composer receives a NEWLINE join
    /// for the tapped choice (matching the existing task-chat join convention
    /// the ticket prescribes for expanded drafts).
    #[test]
    fn expanded_card_nested_chips_newlinejoin_the_tapped_option() {
        let choices = crate::ui::reply_tail::digest_choices(
            &crate::ui::reply_tail::parse_reply_tail(CARD_DIGEST),
        );
        assert_eq!(choices.len(), 2);
        let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);

        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut probe =
            CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
        probe.draft = "Long rationale:".to_string();

        let mut out = card_frame(&ctx, &mut probe, true, Vec::new());
        assert!(
            card_walk_index(&out, "Your answer needed").is_some(),
            "expanded card keeps the heading"
        );
        let cells = card_chip_cells(&out);
        assert_eq!(cells.len(), 2, "expanded frame nests the chips too");
        let yes_point = card_point(&out, &yes_label).expect("chip one centre");
        out.textures_delta.clear();

        card_frame(&ctx, &mut probe, true, press_events(yes_point))
            .textures_delta
            .clear();
        card_frame(&ctx, &mut probe, true, release_events(yes_point))
            .textures_delta
            .clear();

        assert_eq!(probe.sent, 0, "expanded tap must not send either");
        assert_eq!(
            probe.draft, "Long rationale:\nYes, bind Aurora effective Monday.",
            "expanded card newline-joins the FULL chosen text"
        );
    }

    /// AC1 negative: a busy task lane DIMS the row (still two radius-10
    /// cells) and swallows the tap — no draft mutation, no send attempt.
    #[test]
    fn busy_cards_dim_the_chips_and_swallow_taps() {
        let choices = crate::ui::reply_tail::digest_choices(
            &crate::ui::reply_tail::parse_reply_tail(CARD_DIGEST),
        );
        let no_label = crate::ui::reply_tail::choice_label(&choices[1]);

        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut probe =
            CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
        probe.busy = true;
        probe.draft = "Kept.".to_string();

        let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
        let cells = card_chip_cells(&out);
        assert_eq!(
            cells.len(),
            2,
            "busyness dims, it does not remove, the chips"
        );
        assert!(
            cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
            "no hover promotion while busy"
        );
        let no_point = card_point(&out, &no_label).expect("dimmed chip centre");
        out.textures_delta.clear();

        card_frame(&ctx, &mut probe, false, press_events(no_point))
            .textures_delta
            .clear();
        card_frame(&ctx, &mut probe, false, release_events(no_point))
            .textures_delta
            .clear();

        assert_eq!(
            probe.draft, "Kept.",
            "busy tap must leave the draft untouched"
        );
        assert_eq!(probe.sent, 0, "busy tap must not send");
    }

    /// AC1/AC3 discipline on the card surface: a legacy next-step final, a
    /// single-choice digest, an answered digest and a plain final each paint
    /// ZERO chips even though the frame itself still behaves (heading
    /// intact); the control open two-choice digest renders both.
    #[test]
    fn cards_without_an_open_two_to_six_choice_digest_offer_no_chips() {
        let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
        let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
        let cases: Vec<(&str, Vec<ChatMessage>)> = vec![
            (
                "legacy next-step final",
                vec![
                    u("Set up SSO."),
                    a("SSO is recorded.\nYour next step: Should guests use SSO too?"),
                ],
            ),
            (
                "single-choice digest",
                vec![u("Deal?"), a("Deal?\n---\n- Yes, take it.")],
            ),
            (
                "answered digest (retry frame)",
                vec![u("Pick a vendor."), a(CARD_DIGEST), u("Aurora, Monday.")],
            ),
            (
                "marker-free plain final",
                vec![u("Status?"), a("All green, nothing blocked.")],
            ),
        ];
        for (name, messages) in cases {
            let ctx = egui::Context::default();
            ctx.set_visuals(theme::packet_visuals());
            let mut probe =
                CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
            probe.messages = messages;
            let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
            assert_eq!(card_chip_cells(&out).len(), 0, "{name}: zero chips");
            out.textures_delta.clear();
        }
        // Control: the open two-choice digest offers its row.
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        let mut probe =
            CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
        let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
        assert_eq!(
            card_chip_cells(&out).len(),
            2,
            "open digest renders both cards' chips"
        );
        out.textures_delta.clear();
    }
}
