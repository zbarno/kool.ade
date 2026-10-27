use crate::persistence::harness_settings::{self, HarnessSettings, WorkRoute};
use std::collections::BTreeMap;

const CATEGORIES: [(&str, &str); 14] = [
    (harness_settings::MANAGER, "Manager"),
    (harness_settings::PLANNING, "General planning"),
    (harness_settings::SPECIFICATION, "Specification"),
    (harness_settings::FEATURE_PLANNING, "Feature planning"),
    (harness_settings::TASK_GENERATION, "Task generation"),
    (harness_settings::INVESTIGATION, "Investigation"),
    (harness_settings::PLAN_REVIEW, "Independent plan review"),
    (harness_settings::IMPLEMENTATION, "Implementation"),
    (harness_settings::QA, "QA / verification"),
    (harness_settings::RUNTIME_QA, "Runtime QA"),
    (harness_settings::DOCUMENTATION, "Documentation"),
    (harness_settings::RECONCILIATION, "Reconciliation"),
    (
        harness_settings::DECISION_EXPLANATION,
        "Decision explanation",
    ),
    (harness_settings::REPAIR, "Repair worker"),
];

pub(super) fn paint_routes(
    ui: &mut egui::Ui,
    routes: &mut BTreeMap<String, WorkRoute>,
    settings: &HarnessSettings,
) {
    for (category, label) in CATEGORIES {
        let selected = routes.get(category).cloned();
        let now = harness_settings::current_unix_ms();
        let role = crate::app::root::role_for_work_category(category);
        let candidates = settings
            .discovered
            .iter()
            .filter(|(id, entry)| {
                role.is_some_and(|role| {
                    entry.has_current_authenticated_capability(id, now)
                        && entry.has_any_verified_model_at(id, now)
                        && entry.supports_role_or_context_only_planning_at(role, now)
                        && (category != harness_settings::IMPLEMENTATION
                            || (entry.implementation_available
                                && crate::harness::implementation_route_available(id)))
                })
            })
            .flat_map(|(id, entry)| model_routes_for(id, entry, now))
            .collect::<Vec<_>>();
        let selected_text = selected.as_ref().map_or_else(
            || "Use Coding tools default".to_owned(),
            |route| route_label(route, settings),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(label).strong());
            egui::ComboBox::from_id_salt(("project-harness-route", category))
                .selected_text(selected_text)
                .width(ui.available_width())
                .wrap_mode(egui::TextWrapMode::Truncate)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(selected.is_none(), "Use Coding tools default")
                        .clicked()
                    {
                        routes.remove(category);
                    }
                    for candidate in &candidates {
                        if ui
                            .selectable_label(
                                selected.as_ref() == Some(candidate),
                                route_label(candidate, settings),
                            )
                            .clicked()
                        {
                            routes.insert(category.to_owned(), candidate.clone());
                        }
                    }
                });
        });
    }
}

fn model_routes_for(
    id: &str,
    entry: &crate::persistence::harness_settings::DetectedHarness,
    now: u64,
) -> Vec<WorkRoute> {
    let default = entry.verified_default_model_at(id, now).map(|_| WorkRoute {
        harness: id.to_owned(),
        model: None,
    });
    default
        .into_iter()
        .chain(
            entry
                .models
                .iter()
                .filter(|model| entry.has_verified_model_at(id, model, now))
                .map(|model| WorkRoute {
                    harness: id.to_owned(),
                    model: Some(model.clone()),
                }),
        )
        .collect()
}

#[cfg(test)]
#[path = "project_routes/model_tests.rs"]
mod model_tests;

fn route_label(route: &WorkRoute, settings: &HarnessSettings) -> String {
    let now = harness_settings::current_unix_ms();
    if let Some(model) = route
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
    {
        let verified = settings
            .discovered
            .get(&route.harness)
            .is_some_and(|entry| entry.has_verified_model_at(&route.harness, model, now));
        let qualifier = if verified { "" } else { " (unverified)" };
        return format!("{} / {model}{qualifier}", route.harness);
    }
    match settings
        .discovered
        .get(&route.harness)
        .and_then(|entry| entry.verified_default_model_at(&route.harness, now))
    {
        Some(model) => format!("{} / {model}", route.harness),
        None => format!("{} / CLI default (unverified)", route.harness),
    }
}
