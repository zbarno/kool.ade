use super::navigate_to_planning;

pub(super) fn paint(
    ui: &mut egui::Ui,
    relations: &[crate::ui::task_detail::OpportunityRelationView],
) {
    egui::CollapsingHeader::new(format!("Related proposals · {} records", relations.len()))
        .default_open(true)
        .id_salt("task_map_related_opportunities")
        .show(ui, |ui| {
            ui.label(crate::ui::theme::helper_text(
                "Proposals appear only when they record this feature UID or task UID.",
            ));
            egui::ScrollArea::vertical()
                .id_salt("task_map_related_opportunities_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for relation in relations {
                        ui.group(|ui| {
                            ui.small(&relation.relation);
                            if relation.navigable {
                                if ui
                                    .link(format!("{} · {}", relation.kind, relation.title))
                                    .clicked()
                                {
                                    navigate_to_planning(ui, relation.conversation_key.clone());
                                }
                            } else {
                                ui.label(format!("{} · {}", relation.kind, relation.title));
                                ui.small(format!(
                                    "Related proposal unavailable or has an ambiguous UID · {}",
                                    relation.opportunity_uid
                                ));
                            }
                            super::node_status(ui, &relation.status);
                        });
                    }
                });
        });
}
