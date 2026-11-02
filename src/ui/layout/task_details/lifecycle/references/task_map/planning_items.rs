use super::navigate_to_planning;

pub(super) fn paint(
    ui: &mut egui::Ui,
    feature_uid: &str,
    relations: &[crate::ui::task_detail::PlanningItemRelationView],
) {
    let available = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
            "koolade_task_planning_item_keys",
        ))
        .unwrap_or_default()
    });
    egui::CollapsingHeader::new(format!("Questions and decisions · {} items", relations.len()))
        .default_open(true)
        .id_salt(("task_planning_items", feature_uid))
        .show(ui, |ui| {
            ui.label(crate::ui::theme::helper_text(
                "These items link to this feature UID; prerequisite arrows follow their recorded blocked-by IDs.",
            ));
            let titles = relations
                .iter()
                .map(|item| (item.planning_item_id.as_str(), item.title.as_str()))
                .collect::<std::collections::HashMap<_, _>>();
            egui::ScrollArea::vertical()
                .id_salt(("task_planning_items_scroll", feature_uid))
                .max_height(200.0)
                .show(ui, |ui| {
                    for item in relations {
                        ui.group(|ui| {
                            if item.legacy_feature_link {
                                ui.small("Relationship data incomplete · linked by legacy feature ID");
                            }
                            if item.feature_reference_conflict {
                                ui.colored_label(
                                    crate::ui::theme::WARNING,
                                    "Relationship conflict · feature UID disagrees with feature ID",
                                );
                            }
                            if item.blocked_by.is_empty() {
                                ui.small("Feature-linked decision or attention item");
                            } else {
                                for prerequisite in &item.blocked_by {
                                    if available.contains_key(prerequisite)
                                        && let Some(title) = titles.get(prerequisite.as_str())
                                    {
                                        ui.small(format!(
                                            "Prerequisite → this item · {title}"
                                        ));
                                    } else {
                                        ui.small(format!(
                                            "Prerequisite unavailable · {prerequisite}"
                                        ));
                                    }
                                }
                            }
                            let key = available.get(&item.planning_item_id).cloned();
                            if item.feature_reference_conflict {
                                ui.label(format!("{} · {}", item.kind, item.title));
                                ui.small("Navigation disabled while feature references conflict.");
                            } else if let Some(key) = key {
                                if ui.link(format!("{} · {}", item.kind, item.title)).clicked() {
                                    navigate_to_planning(ui, key);
                                }
                            } else {
                                ui.label(format!("{} · {}", item.kind, item.title));
                                ui.small(format!(
                                    "Planning item is unavailable or has an ambiguous ID · {}",
                                    item.planning_item_id
                                ));
                            }
                            super::node_status(ui, &item.status);
                        });
                    }
                });
        });
}
