use crate::ui::{ApplicationCommand, Surface, theme};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
    height: f32,
) {
    let key = format!("clarification:{}", question.uid);
    ui.heading(&question.question);
    ui.label(theme::metadata_text(format!(
        "Question · {} · plan revision {} · record revision {}",
        question.category.label(),
        question.plan_revision,
        question.revision
    )));
    if ui.available_width() >= 820.0 {
        ui.columns(2, |columns| {
            thread(&mut columns[0], surface, &key, height);
            details(&mut columns[1], surface, question);
        });
    } else {
        details(ui, surface, question);
        ui.separator();
        thread(ui, surface, &key, height);
    }
}

fn thread(ui: &mut egui::Ui, surface: &mut dyn Surface, key: &str, height: f32) {
    ui.label(theme::section_heading("Question thread"));
    ui.label(theme::helper_text(
        "This thread belongs to this clarification.",
    ));
    egui::ScrollArea::vertical()
        .id_salt(("clarification_thread", key))
        .max_height(height)
        .show(ui, |ui| {
            for message in surface.task_messages(key) {
                ui.group(|ui| {
                    ui.label(theme::metadata_text(format!("{:?}", message.role)));
                    crate::ui::markdown::paint(ui, &message.text, crate::ui::markdown::CHAT);
                });
            }
        });
}

fn details(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
) {
    ui.label(theme::section_heading("Why it matters"));
    ui.label(&question.reason);
    ui.label(theme::section_heading("If unanswered"));
    ui.label(&question.no_answer_impact);
    if !question.evidence_refs.is_empty() {
        ui.label(theme::section_heading("Evidence"));
        for evidence in &question.evidence_refs {
            ui.label(evidence);
        }
    }
    if question.status != crate::core::planning::QuestionStatus::Open {
        ui.label(theme::helper_text(match question.status {
            crate::core::planning::QuestionStatus::Answered => {
                "Answer recorded; the planner can revisit readiness."
            }
            crate::core::planning::QuestionStatus::Declined => {
                "The user declined to answer; the question remains unresolved for readiness."
            }
            crate::core::planning::QuestionStatus::Draft => "This question is still a draft.",
            crate::core::planning::QuestionStatus::Superseded => {
                "This question was superseded by a newer plan revision."
            }
            crate::core::planning::QuestionStatus::Cancelled => "This question was cancelled.",
            crate::core::planning::QuestionStatus::Open => unreachable!(),
        }));
        if let Some(answer) = &question.answer_text {
            ui.label(theme::section_heading("Recorded answer"));
            ui.label(answer);
        }
        return;
    }

    let draft_id = egui::Id::new(("clarification_answer", question.uid.as_str()));
    let selected_id = egui::Id::new(("clarification_option", question.uid.as_str()));
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_default();
    let mut selected = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(selected_id));
    if !question.options.is_empty() {
        ui.label(theme::section_heading("Options"));
        for option in &question.options {
            let recommended = question.recommended_option_id.as_deref() == Some(&option.id);
            let label = if recommended {
                format!("{} · Recommended", option.label)
            } else {
                option.label.clone()
            };
            if ui
                .selectable_label(selected.as_deref() == Some(&option.id), label)
                .clicked()
            {
                selected = Some(option.id.clone());
            }
            ui.label(theme::helper_text(&option.tradeoffs));
        }
    }
    ui.label(theme::section_heading("Your answer"));
    ui.add(
        egui::TextEdit::multiline(&mut draft)
            .desired_rows(3)
            .hint_text("Choose an option or enter a free-form answer"),
    );
    ui.ctx().data_mut(|data| {
        data.insert_temp(draft_id, draft.clone());
        if let Some(option) = &selected {
            data.insert_temp(selected_id, option.clone());
        }
    });
    let can_submit = !draft.trim().is_empty() || selected.is_some();
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
            selected_option: selected,
        });
        ui.ctx().data_mut(|data| {
            data.remove::<String>(draft_id);
            data.remove::<String>(selected_id);
        });
    }
}
