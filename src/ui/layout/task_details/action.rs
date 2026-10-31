//! Essential task information; supporting controls stay in one disclosure.
use super::*;
mod answer;
mod choice;
pub(super) use choice::card;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    state: state::Presentation<'_>,
    brief: Option<&crate::core::attention::Brief>,
    checks_unavailable: bool,
    activity_path: &mut Option<String>,
) {
    let needs_answer = !state.active
        && state.column != 4
        && !state.decision_sent
        && !review::approval_required(state.record)
        && state.view.lifecycle_pr_status.is_none()
        && brief.is_some_and(|brief| !brief.options.is_empty());
    let action_needed = needs_answer || review::changes_requested(state.record);
    let status = if state.active {
        "Working"
    } else if state.column == 4 {
        "Completed"
    } else if action_needed {
        "Action needed"
    } else if state.failure.is_some() {
        if state.view.failure_disposition
            == Some(crate::core::implementation::RecoveryDisposition::UserAction)
        {
            "Blocked"
        } else {
            "Failed"
        }
    } else if state.column == 2 {
        "In review"
    } else {
        "Ready"
    };
    egui::Frame::NONE
        .fill(theme::PANEL_ALT)
        .stroke(egui::Stroke::new(
            1.0,
            if action_needed {
                theme::WARNING
            } else {
                theme::BORDER
            },
        ))
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.label(
                RichText::new(if action_needed {
                    format!("!  {status}")
                } else {
                    status.to_owned()
                })
                .size(17.0)
                .strong()
                .color(if action_needed {
                    theme::WARNING
                } else {
                    theme::TEXT_DIM
                }),
            );
        });
    ui.add_space(16.0);
    let open_question = state
        .open_ask
        .and_then(|index| state.view.messages.get(index))
        .and_then(|message| {
            crate::ui::reply_tail::parse_reply_tail(&crate::ui::message_text::readable(message)).ask
        });
    let title = brief
        .filter(|_| !state.active && state.column != 4)
        .map(|brief| brief.problem.as_str())
        .or(open_question
            .as_deref()
            .filter(|_| !state.active && state.column != 4))
        .unwrap_or_else(|| super::super::board::presentation::human_title(&doc.title, &doc.path));
    ui.add(
        egui::Label::new(RichText::new(title).font(theme::task_heading_font(ui.ctx(), 28.0)))
            .wrap(),
    );
    ui.add_space(10.0);
    let outcome = if state.column == 4 {
        state
            .record
            .map(|record| concise_detail(&record.detail))
            .filter(|detail| !detail.is_empty())
    } else {
        state
            .failure
            .map(concise_detail)
            .filter(|detail| !detail.is_empty())
    };
    let explanation = if state.active {
        "Kool.ad/e is working on this task. You can ask a question while it continues."
    } else if state.column == 4 {
        state.cleanup_error.or(outcome.as_deref()).unwrap_or(
            "This task is complete. You can review the outcome or ask a follow-up question.",
        )
    } else if state.decision_sent {
        "Your decision is saved with this task."
    } else if review::changes_requested(state.record) {
        "Describe the changes you need before this work is ready."
    } else if state.column == 2 {
        "This task is ready for review."
    } else {
        brief
            .map(|brief| brief.after.as_str())
            .or(outcome.as_deref())
            .unwrap_or("Ask a question if you need more information before continuing.")
    };
    ui.add(egui::Label::new(RichText::new(explanation).size(18.0).color(theme::TEXT_DIM)).wrap());
    ui.add_space(24.0);
    if needs_answer {
        ui.label(RichText::new("Choose one").font(theme::task_heading_font(ui.ctx(), 21.0)));
        ui.add_space(8.0);
        for choice in &brief.unwrap().options {
            if card(
                ui,
                &choice.label,
                &choice.meaning,
                !state.view.conversation_active,
            ) {
                if choice.clarification {
                    ask(
                        surface,
                        state.ticket,
                        format!(
                            "I need clarification: {}. Please explain before I decide.",
                            choice.label
                        ),
                    );
                } else {
                    submit(
                        surface,
                        state.ticket,
                        format!("I choose option ({}): {}.", choice.id, choice.label),
                        true,
                    );
                }
            }
            ui.add_space(10.0);
        }
        if !brief
            .unwrap()
            .options
            .iter()
            .any(|option| option.clarification)
            && card(
                ui,
                "I'm not sure",
                "I need more information first.",
                !state.view.conversation_active,
            )
        {
            ask(
                surface,
                state.ticket,
                "I'm not sure. Please explain the choices and what each would change.".into(),
            );
        }
    }
    let requested_changes = review::changes_requested(state.record);
    let needs_text = state.open_ask.is_some()
        || requested_changes
        || brief.is_some_and(|brief| !brief.steps.is_empty());
    if !needs_answer && !state.active && state.column != 4 && !state.decision_sent && needs_text {
        let choices = if requested_changes {
            Vec::new()
        } else {
            crate::ui::reply_tail::open_digest_choices(&state.view.messages)
        };
        if !choices.is_empty() {
            ui.label(RichText::new("Choose one").font(theme::task_heading_font(ui.ctx(), 21.0)));
            for choice in choices {
                if card(ui, &choice.text, "", !state.view.conversation_active) {
                    submit(
                        surface,
                        state.ticket,
                        choice.text,
                        state.view.answer_context.is_some(),
                    );
                }
                ui.add_space(10.0);
            }
        } else {
            answer::paint(ui, surface, state, requested_changes);
        }
    }
    if !needs_answer {
        details::paint_primary(ui, surface, state, checks_unavailable);
    }
    ui.add_space(18.0);
    ui.separator();
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(12)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new(RichText::new("Why am I seeing this?").size(17.0).strong())
                .id_salt(("task_context", state.ticket))
                .default_open(false)
                .show(ui, |ui| {
                    if let Some(brief) = brief {
                        if state.decision_sent && !state.active {
                            choice::paint_change(ui, surface, state, brief);
                        }
                        reply::paint_recommendation(ui, brief);
                        for choice in &brief.options {
                            ui.label(format!("{}: {}", choice.label, choice.consequence));
                        }
                        for step in &brief.steps {
                            ui.label(format!("{}: {}", step.owner, step.action));
                        }
                    }
                    details::paint(
                        ui,
                        surface,
                        doc,
                        state,
                        checks_unavailable,
                        state.failure,
                        activity_path,
                    );
                });
        });
}

fn ask(surface: &mut dyn Surface, ticket: &str, draft: String) {
    details::dispatch(
        surface,
        crate::ui::task_detail::Command::AskQuestion {
            ticket: ticket.to_owned(),
            draft,
        },
    );
}

fn submit(surface: &mut dyn Surface, ticket: &str, draft: String, decision: bool) {
    details::dispatch(
        surface,
        crate::ui::task_detail::Command::SubmitReply {
            ticket: ticket.to_owned(),
            draft,
            decision,
        },
    );
}

pub(super) fn concise_detail(detail: &str) -> String {
    let text = detail
        .lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("Full report:")
        })
        .unwrap_or_default();
    let mut result = text.chars().take(280).collect::<String>();
    if text.chars().count() > 280 {
        result.push('…');
    }
    result
}
