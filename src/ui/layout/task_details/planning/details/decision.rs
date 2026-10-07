use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    item: &crate::domain::item::OpenItem,
) {
    let Some(brief) = item.decision_brief.as_ref() else {
        return;
    };
    ui.label(crate::ui::theme::section_heading("Decision guidance"));
    if let Some(recommendation) = &brief.recommendation {
        ui.label(format!(
            "Kool.ad/e recommends {}",
            option_label(brief, &recommendation.option_id)
        ));
        ui.label(crate::ui::theme::helper_text(&recommendation.rationale));
    }
    if !brief.options.is_empty() && item.status != crate::domain::ItemStatus::Resolved {
        ui.label(crate::ui::theme::section_heading("Choose an option"));
        for option in &brief.options {
            if ui.button(&option.label).clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    crate::ui::task_detail::Command::UpdateDraft {
                        ticket: item.conversation_key().to_owned(),
                        draft: format!("I choose option ({}): {}.", option.id, option.label),
                    },
                ));
            }
        }
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
