use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    item: &crate::domain::item::OpenItem,
    items: &[crate::domain::item::OpenItem],
) {
    if item.status == crate::domain::ItemStatus::Resolved
        || item.authority == crate::domain::Authority::Agent
        || item.is_ownership_gap()
        || crate::core::routing::has_open_prerequisite(items, item)
    {
        return;
    }
    let Some(brief) = item
        .decision_brief
        .as_ref()
        .filter(|brief| !brief.options.is_empty())
    else {
        free_text(ui, surface, item.conversation_key());
        return;
    };

    if !brief.options.is_empty() && item.status != crate::domain::ItemStatus::Resolved {
        ui.label(crate::ui::theme::section_heading("Choose one"));
        for option in &brief.options {
            if super::super::super::action::card(
                ui,
                &option.label,
                &option.summary,
                !surface.task_chat_active(item.conversation_key()),
            ) {
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    crate::ui::task_detail::Command::SubmitReply {
                        ticket: item.conversation_key().to_owned(),
                        draft: format!("I choose option ({}): {}.", option.id, option.label),
                        decision: false,
                    },
                ));
            }
        }
    }
}

pub(super) fn free_text(ui: &mut egui::Ui, surface: &mut dyn Surface, key: &str) {
    let id = egui::Id::new(("planning_answer", key));
    let pending_id = id.with("pending");
    let saved_id = id.with("saved");
    if let Some((after, text)) = ui
        .ctx()
        .data_mut(|data| data.get_temp::<(usize, String)>(pending_id))
        && surface.task_chat_error().is_none()
        && surface
            .task_messages(key)
            .iter()
            .skip(after)
            .any(|message| {
                message.role == crate::domain::ChatRole::User
                    && !message.clarification
                    && message.text == text
            })
    {
        ui.ctx().data_mut(|data| {
            data.remove::<String>(id);
            data.remove::<(usize, String)>(pending_id);
            data.insert_temp(saved_id, true);
        });
    }
    let mut answer = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(id))
        .unwrap_or_default();
    ui.label("Your answer");
    ui.add(
        egui::TextEdit::multiline(&mut answer)
            .desired_width(f32::INFINITY)
            .hint_text("Enter your answer…"),
    );
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, answer.clone()));
    if ui
        .add_enabled(
            !surface.task_chat_active(key) && !answer.trim().is_empty(),
            egui::Button::new("Submit answer").fill(theme::BLUE),
        )
        .clicked()
    {
        let pending = (
            surface.task_messages(key).len(),
            format!("My answer: {}", answer.trim()),
        );
        ui.ctx().data_mut(|data| {
            data.insert_temp(pending_id, pending);
            data.insert_temp(saved_id, false);
        });
        surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::SubmitReply {
                ticket: key.to_owned(),
                draft: format!("My answer: {}", answer.trim()),
                decision: false,
            },
        ));
    }
    if ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(saved_id))
        .unwrap_or(false)
    {
        ui.label("Answer saved.");
    }
}

pub(super) fn paint_context(ui: &mut egui::Ui, item: &crate::domain::item::OpenItem) {
    let Some(brief) = item.decision_brief.as_ref() else {
        return;
    };
    if let Some(recommendation) = &brief.recommendation {
        ui.label(format!(
            "Kool.ad/e recommends {}",
            option_label(brief, &recommendation.option_id)
        ));
        ui.label(crate::ui::theme::helper_text(&recommendation.rationale));
    }
    ui.collapsing("Decision details", |ui| {
        if let Some(confidence) = &brief.confidence {
            ui.label(format!("Confidence: {:?}", confidence.level));
            ui.label(&confidence.explanation);
        }
        for option in &brief.options {
            ui.collapsing(format!("{} · {}", option.label, option.summary), |ui| {
                for detail in option
                    .benefits
                    .iter()
                    .chain(&option.costs)
                    .chain(&option.risks)
                {
                    ui.label(detail);
                }
            });
        }
        for detail in &brief.ramifications {
            ui.label(detail);
        }
        ui.label(&brief.defer_consequence);
        for evidence in &brief.evidence {
            ui.label(evidence);
        }
    });
}

fn option_label<'a>(brief: &'a crate::domain::DecisionBrief, id: &str) -> &'a str {
    brief
        .options
        .iter()
        .find(|option| option.id == id)
        .map(|option| option.label.as_str())
        .unwrap_or("the selected option")
}
