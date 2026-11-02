use super::navigate_to_planning;

pub(super) fn paint(
    ui: &mut egui::Ui,
    feature_uid: &str,
    relations: &[(String, String, String, String, Option<String>)],
) {
    egui::CollapsingHeader::new(format!("Planning work · {} records", relations.len()))
        .default_open(true)
        .id_salt(("task_planning_work", feature_uid))
        .show(ui, |ui| {
            ui.label(crate::ui::theme::helper_text(
                "Planning work edges follow recorded parent UIDs. Select a record to open its own details and conversation.",
            ));
            egui::ScrollArea::vertical()
                .id_salt(("task_planning_work_scroll", feature_uid))
                .max_height(200.0)
                .show(ui, |ui| {
                    for (uid, title, kind, status, parent_uid) in relations {
                        ui.group(|ui| {
                            let edge = parent_uid.as_ref().map_or_else(
                                || "Feature-linked planning work".to_owned(),
                                |parent| format!("Parent work → this record · {parent}"),
                            );
                            ui.small(edge);
                            let key_for_uid = ui.ctx().data_mut(|data| {
                                data.get_temp::<std::collections::HashMap<String, String>>(
                                    egui::Id::new("koolade_task_planning_work_keys"),
                                )
                                .and_then(|keys| keys.get(uid).cloned())
                            });
                            if let Some(key_for_uid) = key_for_uid {
                                if ui.link(format!("{kind} · {title}")).clicked() {
                                    navigate_to_planning(ui, key_for_uid);
                                }
                            } else {
                                ui.label(format!("{kind} · {title}"));
                                ui.small(format!(
                                    "Related planning work is unavailable or has an ambiguous UID · {uid}"
                                ));
                            }
                            super::node_status(ui, status);
                        });
                    }
                });
        });
}
