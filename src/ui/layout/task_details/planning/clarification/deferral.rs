use super::*;

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
