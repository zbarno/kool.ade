use crate::ui::{ApplicationCommand, Surface, theme};

#[derive(Clone, Default)]
struct DeferralDraft {
    assumption: String,
    owner: String,
    risk: String,
    verification_test: String,
    follow_up_task: String,
}

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
    if ui.button("Decline to choose").clicked() {
        surface.dispatch(ApplicationCommand::AnswerClarification {
            uid: question.uid.clone(),
            expected_revision: question.revision,
            answer: String::new(),
            selected_option: None,
        });
        ui.ctx().data_mut(|data| {
            data.remove::<String>(draft_id);
            data.remove::<String>(selected_id);
        });
    }
    if question.priority == crate::core::planning::QuestionPriority::Optional {
        defer_optional(ui, surface, question);
    }
}

fn defer_optional(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
) {
    let open_id = egui::Id::new(("clarification_defer_open", question.uid.as_str()));
    let draft_id = egui::Id::new(("clarification_defer_draft", question.uid.as_str()));
    let mut is_open = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(open_id))
        .unwrap_or(false);
    if ui.button("Defer optional question…").clicked() {
        is_open = true;
    }
    ui.ctx().data_mut(|data| data.insert_temp(open_id, is_open));
    if !is_open {
        return;
    }
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<DeferralDraft>(draft_id))
        .unwrap_or_else(|| DeferralDraft {
            assumption: question.assumption_if_deferred.clone().unwrap_or_default(),
            ..DeferralDraft::default()
        });
    ui.label(theme::section_heading("Record the deferral"));
    ui.add(
        egui::TextEdit::multiline(&mut draft.assumption)
            .desired_rows(2)
            .hint_text("Assumption"),
    );
    ui.add(egui::TextEdit::singleline(&mut draft.owner).hint_text("Owner"));
    ui.add(
        egui::TextEdit::multiline(&mut draft.risk)
            .desired_rows(2)
            .hint_text("Risk"),
    );
    ui.add(egui::TextEdit::singleline(&mut draft.verification_test).hint_text("Verification test"));
    ui.add(egui::TextEdit::singleline(&mut draft.follow_up_task).hint_text("Follow-up task"));
    let complete = [
        &draft.assumption,
        &draft.owner,
        &draft.risk,
        &draft.verification_test,
        &draft.follow_up_task,
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    ui.ctx()
        .data_mut(|data| data.insert_temp(draft_id, draft.clone()));
    if ui
        .add_enabled(
            complete,
            egui::Button::new("Save deferral").fill(theme::ACTION),
        )
        .clicked()
    {
        surface.dispatch(ApplicationCommand::DeferClarification {
            uid: question.uid.clone(),
            expected_revision: question.revision,
            details: crate::core::planning::DeferredAssumption {
                assumption: draft.assumption.trim().to_owned(),
                owner: draft.owner.trim().to_owned(),
                risk: draft.risk.trim().to_owned(),
                verification_test: draft.verification_test.trim().to_owned(),
                follow_up_task: draft.follow_up_task.trim().to_owned(),
            },
        });
        ui.ctx().data_mut(|data| {
            data.remove::<DeferralDraft>(draft_id);
            data.remove::<bool>(open_id);
        });
    }
}
