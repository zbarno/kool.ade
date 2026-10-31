use crate::ui::{ApplicationCommand, Surface, theme};
mod deferral;
use deferral::paint as defer_optional;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
    height: f32,
) {
    let key = format!("clarification:{}", question.uid);
    if ui.available_width() >= 760.0 {
        let width = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2((width - 28.0) * 0.6, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("clarification_details", &key))
                        .max_height(height)
                        .show(ui, |ui| details(ui, surface, question));
                },
            );
            ui.separator();
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(
                egui::vec2((width - 28.0) * 0.4, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| thread(ui, surface, &key, height),
            );
        });
    } else {
        details(ui, surface, question);
        ui.separator();
        thread(ui, surface, &key, height);
    }
}

fn thread(ui: &mut egui::Ui, surface: &mut dyn Surface, key: &str, height: f32) {
    let _ = height;
    super::conversation::paint(ui, surface, key);
}

fn details(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
) {
    theme::badge(
        ui,
        match question.status {
            crate::core::planning::QuestionStatus::Open
            | crate::core::planning::QuestionStatus::Declined => "Action needed",
            crate::core::planning::QuestionStatus::Answered
            | crate::core::planning::QuestionStatus::EvidenceResolved => "Completed",
            crate::core::planning::QuestionStatus::Draft => "Draft",
            crate::core::planning::QuestionStatus::Deferred => "Deferred",
            crate::core::planning::QuestionStatus::Superseded => "Superseded",
            crate::core::planning::QuestionStatus::Cancelled => "Cancelled",
        },
        theme::PANEL_ALT,
        theme::WARNING,
    );
    ui.add_space(16.0);
    ui.heading(&question.question);
    ui.add_space(10.0);
    ui.label(&question.reason);
    if question.status != crate::core::planning::QuestionStatus::Open {
        ui.label(theme::helper_text(match question.status {
            crate::core::planning::QuestionStatus::Answered => {
                "Answer recorded; the planner can revisit readiness."
            }
            crate::core::planning::QuestionStatus::EvidenceResolved => {
                "Man.ager resolved this technical choice from cited repository evidence; this is not a user decision."
            }
            crate::core::planning::QuestionStatus::Declined => {
                "The user declined to answer; the question remains unresolved for readiness."
            }
            crate::core::planning::QuestionStatus::Deferred => {
                "Optional assumption recorded; this is not a product decision."
            }
            crate::core::planning::QuestionStatus::Draft => "This question is still a draft.",
            crate::core::planning::QuestionStatus::Superseded => {
                "This question was superseded by a newer plan revision."
            }
            crate::core::planning::QuestionStatus::Cancelled => "This question was cancelled.",
            crate::core::planning::QuestionStatus::Open => unreachable!(),
        }));
        if let Some(deferred) = &question.deferred {
            ui.label(theme::section_heading("Deferred assumption"));
            ui.label(&deferred.assumption);
            ui.label(format!("Owner: {}", deferred.owner));
            ui.label(format!("Risk: {}", deferred.risk));
            ui.label(format!("Verification test: {}", deferred.verification_test));
            ui.label(format!("Follow-up task: {}", deferred.follow_up_task));
            ui.label(theme::metadata_text(format!(
                "Deferred by {} · record revision {}",
                question.deferred_by.as_deref().unwrap_or("unknown"),
                question.revision
            )));
        }
        if let Some(answer) = &question.answer_text {
            ui.label(theme::section_heading("Recorded answer"));
            ui.label(answer);
        }
        if let Some(selected_option) = &question.selected_option
            && let Some(option) = question
                .options
                .iter()
                .find(|option| option.id == *selected_option)
        {
            ui.label(theme::section_heading("Selected option"));
            ui.label(&option.label);
            ui.label(theme::helper_text(&option.tradeoffs));
        }
        context(ui, question);
        return;
    }

    let draft_id = egui::Id::new(("clarification_answer", question.uid.as_str()));
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_default();
    if !question.options.is_empty() {
        ui.label(theme::section_heading("Choose one"));
        for option in &question.options {
            let recommended = question.recommended_option_id.as_deref() == Some(&option.id);
            let label = if recommended {
                format!("{} · Recommended", option.label)
            } else {
                option.label.clone()
            };
            if super::super::action::card(ui, &label, &option.tradeoffs, true) {
                surface.dispatch(ApplicationCommand::AnswerClarification {
                    uid: question.uid.clone(),
                    expected_revision: question.revision,
                    answer: String::new(),
                    selected_option: Some(option.id.clone()),
                });
            }
        }
    }
    if super::super::action::card(
        ui,
        "I'm not sure",
        "I need more information first.",
        !surface.task_chat_active(&format!("clarification:{}", question.uid)),
    ) {
        surface.dispatch(ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::AskQuestion {
                ticket: format!("clarification:{}", question.uid),
                draft: "I'm not sure. Please explain the question before I answer.".into(),
            },
        ));
    }
    ui.label(theme::section_heading("Your answer"));
    ui.add(
        egui::TextEdit::multiline(&mut draft)
            .desired_rows(3)
            .hint_text("Choose an option or enter a free-form answer"),
    );
    ui.ctx().data_mut(|data| {
        data.insert_temp(draft_id, draft.clone());
    });
    let can_submit = !draft.trim().is_empty();
    if ui
        .add_enabled(
            can_submit,
            egui::Button::new("Save answer").fill(theme::ACTION),
        )
        .clicked()
    {
        surface.dispatch(ApplicationCommand::AnswerClarification {
            uid: question.uid.clone(),
            expected_revision: question.revision,
            answer: draft.trim().to_owned(),
            selected_option: None,
        });
    }
    if ui.button("Decline to choose").clicked() {
        surface.dispatch(ApplicationCommand::AnswerClarification {
            uid: question.uid.clone(),
            expected_revision: question.revision,
            answer: String::new(),
            selected_option: None,
        });
    }
    if question.priority == crate::core::planning::QuestionPriority::Optional {
        defer_optional(ui, surface, question);
    }
    context(ui, question);
}

fn context(ui: &mut egui::Ui, question: &crate::core::planning::QuestionV1) {
    ui.collapsing("Why am I seeing this?", |ui| {
        ui.label(theme::section_heading("If unanswered"));
        ui.label(&question.no_answer_impact);
        if !question.origin_requirement_ids.is_empty() {
            ui.label(theme::section_heading("Linked requirements"));
            ui.label(question.origin_requirement_ids.join(", "));
        }
        if !question.evidence_refs.is_empty() {
            ui.label(theme::section_heading("Evidence"));
            for evidence in &question.evidence_refs {
                ui.label(evidence);
            }
        }
    });
}
