//! Compact, evidence-aware decision details and issue-specific reply choices.
use crate::{
    domain::{Authority, DecisionBrief, OpenItem},
    ui::{Surface, theme},
};
use egui::RichText;

pub(super) fn summary(ui: &mut egui::Ui, item: &OpenItem, expanded: bool) {
    let Some(brief) = &item.decision_brief else {
        return;
    };
    ui.label(RichText::new("Decision guidance").small().strong());
    ui.label(crate::core::context_build::clip(&brief.why_now, 260));
    if let Some((option, rationale)) = recommendation(brief) {
        egui::Frame::NONE
            .fill(theme::ACCENT_SOFT)
            .corner_radius(6)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Packet recommends {}", option.label))
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.add(egui::Label::new(rationale).wrap());
            });
    }
    if !expanded {
        if let Some(confidence) = &brief.confidence {
            ui.label(format!(
                "Confidence: {:?} — {}",
                confidence.level,
                crate::core::context_build::clip(&confidence.explanation, 150)
            ));
        } else {
            ui.label("Confidence is not established from the recorded evidence.");
        }
        if !brief.reversibility.trim().is_empty() {
            ui.label(format!(
                "Can this change later? {}",
                crate::core::context_build::clip(&brief.reversibility, 180)
            ));
        }
        if !brief.defer_consequence.trim().is_empty() {
            ui.label(format!(
                "If you wait: {}",
                crate::core::context_build::clip(&brief.defer_consequence, 180)
            ));
        }
    }
    if expanded {
        ui.collapsing("Decision details", |ui| details(ui, brief));
    }
}

pub(super) fn choices(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    key: &str,
    item: &OpenItem,
    expanded: bool,
    busy: bool,
) -> bool {
    if item.authority != Authority::Human {
        return false;
    }
    let Some(brief) = &item.decision_brief else {
        return false;
    };
    if brief.options.is_empty() {
        return false;
    }
    ui.label(
        RichText::new("Choose an option or write your own answer")
            .small()
            .strong(),
    );
    let mut fired = false;
    for option in &brief.options {
        let consequence = option.consequences.first();
        if crate::ui::decision_choice::paint(
            ui,
            &option.label,
            &option.summary,
            consequence.map(String::as_str),
            !busy,
        ) && let Some(draft) = surface.task_draft(key)
        {
            *draft = format!("I choose option ({}): {}.", option.id, option.label);
            fired = true;
        }
        ui.add_space(4.0);
    }
    if fired && expanded {
        ui.label(
            RichText::new("Review the choice or add detail before sending.")
                .small()
                .weak(),
        );
    }
    fired
}

fn details(ui: &mut egui::Ui, brief: &DecisionBrief) {
    section(ui, "Why now", std::slice::from_ref(&brief.why_now));
    if let Some(confidence) = &brief.confidence {
        ui.label(format!(
            "Confidence: {:?} — {}",
            confidence.level, confidence.explanation
        ));
    } else {
        ui.label("Confidence is not established from the recorded evidence.");
    }
    if !brief.options.is_empty() {
        ui.label(RichText::new("Options").strong());
        for option in &brief.options {
            ui.collapsing(format!("{} · {}", option.label, option.summary), |ui| {
                section(ui, "Benefits", &option.benefits);
                section(ui, "Costs", &option.costs);
                section(ui, "Risks", &option.risks);
                section(ui, "What changes", &option.consequences);
                if !option.reversibility.trim().is_empty() {
                    ui.label(format!("Reversibility: {}", option.reversibility));
                }
            });
        }
    }
    section(ui, "Overall benefits", &brief.benefits);
    section(ui, "Overall costs", &brief.costs);
    section(ui, "Overall risks", &brief.risks);
    section(ui, "Ramifications", &brief.ramifications);
    if !brief.reversibility.trim().is_empty() {
        ui.label(format!("Overall reversibility: {}", brief.reversibility));
    }
    if !brief.defer_consequence.trim().is_empty() {
        ui.label(format!("If deferred: {}", brief.defer_consequence));
    }
    section(ui, "Evidence", &brief.evidence);
}

fn recommendation(brief: &DecisionBrief) -> Option<(&crate::domain::DecisionOption, &str)> {
    let recommendation = brief.recommendation.as_ref()?;
    let option = brief
        .options
        .iter()
        .find(|option| option.id == recommendation.option_id)?;
    Some((option, &recommendation.rationale))
}

fn section(ui: &mut egui::Ui, heading: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    ui.label(RichText::new(heading).strong());
    for value in values {
        ui.add(egui::Label::new(value).wrap());
    }
}
