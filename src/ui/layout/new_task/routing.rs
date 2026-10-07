use crate::{
    persistence::harness_settings::{self, HarnessSettings, WorkRoute},
    ui::theme,
};
use egui::{RichText, Ui};
use std::collections::BTreeMap;

const ROUTABLE_CATEGORIES: [(&str, &str); 3] = [
    (harness_settings::IMPLEMENTATION, "Implementation"),
    (harness_settings::QA, "QA / Verification"),
    (harness_settings::DOCUMENTATION, "Documentation"),
];

pub(super) fn routing_editor(
    ui: &mut Ui,
    settings: &HarnessSettings,
    overrides: &mut BTreeMap<String, WorkRoute>,
) {
    let label = if overrides.is_empty() {
        "Advanced routing · using application defaults"
    } else {
        "Advanced routing · task overrides selected"
    };
    egui::CollapsingHeader::new(label)
        .default_open(false)
        .show(ui, |ui| {
            ui.label(RichText::new("Optional overrides for this task. Each category inherits until you select a replacement.").color(theme::TEXT_MUTED));
            for (category, label) in ROUTABLE_CATEGORIES {
                route_row(ui, settings, overrides, category, label);
            }
        });
}

fn route_row(
    ui: &mut Ui,
    settings: &HarnessSettings,
    overrides: &mut BTreeMap<String, WorkRoute>,
    category: &str,
    label: &str,
) {
    let effective = effective_route(settings, category);
    let is_override = overrides.contains_key(category);
    let mut selected = overrides.get(category).cloned().or(effective);
    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).strong());
            ui.label(
                RichText::new(if is_override {
                    "Task override"
                } else {
                    "Application default"
                })
                .small()
                .color(if is_override {
                    theme::ACCENT
                } else {
                    theme::TEXT_MUTED
                }),
            );
            if is_override && ui.small_button("Use application default").clicked() {
                overrides.remove(category);
                selected = effective_route(settings, category);
            }
        });

        let ready: Vec<_> = settings
            .discovered
            .iter()
            .filter(|(_, harness)| harness.ready)
            .collect();
        let route_text = selected.as_ref().map_or_else(
            || "No application route configured".to_owned(),
            |route| route_label(settings, route),
        );
        egui::ComboBox::from_id_salt(("task-route-harness", category))
            .selected_text(route_text)
            .show_ui(ui, |ui| {
                for (id, harness) in ready {
                    let candidate = WorkRoute {
                        harness: id.clone(),
                        model: None,
                    };
                    if ui
                        .selectable_label(
                            selected
                                .as_ref()
                                .is_some_and(|r| r.harness == *id && r.model.is_none()),
                            route_label(settings, &candidate),
                        )
                        .clicked()
                    {
                        overrides.insert(category.to_owned(), candidate.clone());
                        selected = Some(candidate);
                    }
                    for model in &harness.models {
                        let candidate = WorkRoute {
                            harness: id.clone(),
                            model: Some(model.clone()),
                        };
                        if ui
                            .selectable_label(
                                selected.as_ref() == Some(&candidate),
                                format!("{} / {model}", id),
                            )
                            .clicked()
                        {
                            overrides.insert(category.to_owned(), candidate.clone());
                            selected = Some(candidate);
                        }
                    }
                }
            });
        if let Some(route) = selected.as_ref() {
            let model_hint = route
                .model
                .as_ref()
                .map(|model| format!("Model: {model}"))
                .or_else(|| {
                    settings
                        .discovered
                        .get(&route.harness)
                        .and_then(|h| h.default_model.as_ref())
                        .map(|m| format!("CLI default model: {m}"))
                })
                .unwrap_or_else(|| "Uses the selected harness's configured model.".into());
            ui.label(RichText::new(model_hint).small().color(theme::TEXT_MUTED));
        }
    });
}

fn effective_route(settings: &HarnessSettings, category: &str) -> Option<WorkRoute> {
    settings.work_routes.get(category).cloned().or_else(|| {
        settings.default_harness.clone().map(|harness| WorkRoute {
            harness,
            model: None,
        })
    })
}

fn route_label(settings: &HarnessSettings, route: &WorkRoute) -> String {
    let model = route.model.as_deref().or_else(|| {
        settings
            .discovered
            .get(&route.harness)
            .and_then(|h| h.default_model.as_deref())
    });
    match model {
        Some(model) => format!(
            "{} / {model}{}",
            route.harness,
            if route.model.is_some() {
                ""
            } else {
                " (CLI default)"
            }
        ),
        None => format!("{} / CLI default", route.harness),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(harness: &str, model: Option<&str>) -> WorkRoute {
        WorkRoute {
            harness: harness.into(),
            model: model.map(str::to_owned),
        }
    }

    #[test]
    fn task_routing_uses_category_and_application_defaults() {
        let mut settings = HarnessSettings {
            default_harness: Some("codex".into()),
            ..Default::default()
        };
        settings
            .work_routes
            .insert(harness_settings::QA.into(), route("claude", Some("sonnet")));

        assert_eq!(
            effective_route(&settings, harness_settings::IMPLEMENTATION),
            Some(route("codex", None))
        );
        assert_eq!(
            effective_route(&settings, harness_settings::QA),
            Some(route("claude", Some("sonnet")))
        );
        assert_eq!(
            effective_route(&settings, harness_settings::DOCUMENTATION),
            Some(route("codex", None))
        );
    }

    #[test]
    fn explicit_task_overrides_are_independent_and_can_be_reset() {
        let mut overrides: BTreeMap<String, WorkRoute> = BTreeMap::new();
        overrides.insert(
            harness_settings::IMPLEMENTATION.into(),
            route("pi", Some("openai/gpt-4.1")),
        );
        overrides.insert(harness_settings::QA.into(), route("claude", None));

        assert_eq!(
            overrides.get(harness_settings::IMPLEMENTATION),
            Some(&route("pi", Some("openai/gpt-4.1")))
        );
        assert_eq!(
            overrides.get(harness_settings::QA),
            Some(&route("claude", None))
        );
        assert!(!overrides.contains_key(harness_settings::DOCUMENTATION));

        overrides.remove(harness_settings::IMPLEMENTATION);
        assert!(!overrides.contains_key(harness_settings::IMPLEMENTATION));
        assert!(overrides.contains_key(harness_settings::QA));
    }

    #[test]
    fn manager_route_is_not_exposed_in_task_editor() {
        let categories: Vec<_> = ROUTABLE_CATEGORIES
            .iter()
            .map(|(category, _)| *category)
            .collect();
        assert_eq!(
            categories,
            [
                harness_settings::IMPLEMENTATION,
                harness_settings::QA,
                harness_settings::DOCUMENTATION
            ]
        );
        assert!(!categories.contains(&harness_settings::MANAGER));
    }
}
