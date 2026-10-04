use super::composer::composer;
use super::{decision, status};
mod presentation;
mod state;
use crate::domain::{Authority, ChatRole, ItemStatus};
use crate::ui::{ApplicationCommand, Surface, theme};
use egui::RichText;
use state::CardStatus;
use status::{failed, split_reply};
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
        let latest_reply = messages.iter().rev().find(|m| m.role == ChatRole::Agent);
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
        let story =
            item.is_none() && crate::artifacts::layout::ArtifactLayout::is_task_ticket_path(key);
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
        let (heading, action) = state::labels(CardStatus {
            active,
            expanded,
            retry,
            resolved,
            ownership,
            review,
            blocked,
            blocked_by: &blocked_by,
            needs_answer,
            reply_next: reply.next.as_deref(),
            item: item.as_ref(),
            story,
            implementation_column,
            implementation_detail: implementation.as_ref().map(|record| record.detail.as_str()),
            eligible,
        });
        if expanded && !reply.summary.is_empty() && !active && !introduction {
            ui.label(RichText::new("Latest update").small().weak());
            ui.label(crate::core::context_build::clip(&reply.summary, 420));
        }
        if expanded
            && !active
            && let Some(previous) = messages.iter().rev().find(|m| m.role == ChatRole::User)
        {
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
            .inner_margin(
                if expanded || needs_answer || ownership || review || retry {
                    8
                } else {
                    0
                },
            )
            .show(ui, |ui| {
                ui.label(
                    RichText::new(heading)
                        .size(if expanded { 15.0 } else { 12.5 })
                        .strong()
                        .color(if retry { theme::WARNING } else { theme::TEXT }),
                );
                if !action.is_empty()
                    && (expanded || needs_answer || retry || blocked)
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
                if expanded
                    && active
                    && let Some(previous) = messages.iter().rev().find(|m| m.role == ChatRole::User)
                {
                    ui.label(RichText::new("Your answer").small().weak());
                    ui.label(crate::core::context_build::clip(&previous.text, 400));
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
        presentation::paint_footer(
            ui,
            s,
            key,
            expanded,
            active,
            review,
            needs_answer,
            retry,
            blocked,
            &messages,
            &reply.summary,
            &mut open,
        );
    });
    open
}
