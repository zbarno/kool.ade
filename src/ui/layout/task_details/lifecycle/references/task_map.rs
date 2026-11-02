use crate::ui::{Surface, task_detail::LifecycleView};

mod opportunities;
mod planning_items;
mod planning_work;
mod status;
pub(super) use status::node_status;
#[cfg(test)]
mod tests;

pub(crate) fn relationships(
    ui: &mut egui::Ui,
    _surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
    ticket: &str,
) {
    ui.label(crate::ui::theme::section_heading("Task Map · Related Work"));
    let previous = ui.ctx().data_mut(|data| {
        data.get_temp::<Vec<String>>(egui::Id::new("koolade_task_relationship_history"))
            .and_then(|history| history.last().cloned())
    });
    if let Some(previous) = previous
        && ui.button("← Back to previous task").clicked()
    {
        navigate_back(ui, previous);
    }
    let selected_title = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
            "koolade_task_titles_by_path",
        ))
        .and_then(|titles| titles.get(ticket).cloned())
    });
    ui.group(|ui| {
        ui.label(egui::RichText::new("Selected task · Task").strong());
        ui.label(selected_title.as_deref().unwrap_or_else(|| {
            std::path::Path::new(ticket)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(ticket)
        }));
        ui.label(crate::ui::theme::helper_text(
            crate::ui::task_detail::task_phase_label(lifecycle.task_phase),
        ));
    });
    let dependents = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, Vec<(String, String, String)>>>(
            egui::Id::new("koolade_task_dependents"),
        )
        .and_then(|dependents| dependents.get(ticket).cloned())
        .unwrap_or_default()
    });
    let planning_relations = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<
            String,
            Vec<(String, String, String, String, Option<String>)>,
        >>(egui::Id::new("koolade_task_planning_work_relations"))
            .and_then(|relations| relations.get(&lifecycle.feature_uid).cloned())
            .unwrap_or_default()
    });
    let decision_relations = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<
            String,
            Vec<crate::ui::task_detail::PlanningItemRelationView>,
        >>(egui::Id::new("koolade_task_planning_item_relations"))
            .and_then(|relations| relations.get(&lifecycle.feature_uid).cloned())
            .unwrap_or_default()
    });
    let task_uid = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
            "koolade_task_uids_by_path",
        ))
        .and_then(|uids| uids.get(ticket).cloned())
    });
    let opportunity_relations = ui.ctx().data_mut(|data| {
        let relations =
            data.get_temp::<std::collections::HashMap<
                String,
                Vec<crate::ui::task_detail::OpportunityRelationView>,
            >>(egui::Id::new("koolade_task_opportunity_relations"));
        let mut related = relations
            .as_ref()
            .and_then(|relations| relations.get(&lifecycle.feature_uid))
            .cloned()
            .unwrap_or_default();
        if let Some(task_uid) = &task_uid
            && let Some(task_related) = relations
                .as_ref()
                .and_then(|relations| relations.get(task_uid))
        {
            related.extend(task_related.iter().cloned());
        }
        let mut seen = std::collections::HashSet::new();
        related.retain(|item| seen.insert((item.opportunity_uid.clone(), item.relation.clone())));
        related
    });
    let opportunity_error = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_task_opportunity_error")));
    if lifecycle.parent_task.is_none()
        && lifecycle.dependencies.is_empty()
        && lifecycle.subtasks.is_empty()
        && dependents.is_empty()
        && planning_relations.is_empty()
        && decision_relations.is_empty()
        && opportunity_relations.is_empty()
        && opportunity_error.is_none()
    {
        ui.label(crate::ui::theme::helper_text(
            "No recorded task relationships.",
        ));
        return;
    }
    ui.label(crate::ui::theme::helper_text(
        "Arrows describe recorded task ownership and execution prerequisites.",
    ));
    if let Some(parent) = &lifecycle.parent_task {
        ui.label("Parent-child edge · parent → selected task");
        ui.group(|ui| {
            relation_link(ui, "Parent task", parent);
            node_status(ui, &parent.status);
            ui.label(crate::ui::theme::helper_text(
                "Changes are included in the parent's single pull request.",
            ));
        });
    }
    if !lifecycle.dependencies.is_empty() {
        egui::CollapsingHeader::new("Dependencies")
            .default_open(true)
            .id_salt(("task_dependencies", &lifecycle.feature_uid))
            .show(ui, |ui| {
                ui.small(format!("{} prerequisites", lifecycle.dependencies.len()));
                ui.label(crate::ui::theme::helper_text(
                    "Execution dependency edge · prerequisite → selected task.",
                ));
                egui::ScrollArea::vertical()
                    .id_salt(("task_dependencies_scroll", &lifecycle.feature_uid))
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for dependency in &lifecycle.dependencies {
                            ui.group(|ui| {
                                relation_link(ui, "← Dependency", dependency);
                                node_status(ui, &dependency.status);
                            });
                        }
                    });
            });
    }
    if !lifecycle.subtasks.is_empty() {
        egui::CollapsingHeader::new(format!("Children · {} subtasks", lifecycle.subtasks.len()))
            .default_open(true)
            .id_salt(("task_subtasks", &lifecycle.feature_uid))
            .show(ui, |ui| {
                ui.label(crate::ui::theme::helper_text(
                    "Parent-child edge · children join this parent's single pull request.",
                ));
                egui::ScrollArea::vertical()
                    .id_salt(("task_subtasks_scroll", &lifecycle.feature_uid))
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for subtask in &lifecycle.subtasks {
                            ui.group(|ui| {
                                relation_link(ui, "↓ Subtask", subtask);
                                node_status(ui, &subtask.status);
                                for criterion in &subtask.acceptance_criteria {
                                    ui.label(format!("  • {criterion}"));
                                }
                            });
                        }
                    });
            });
    }
    if !dependents.is_empty() {
        egui::CollapsingHeader::new(format!("Downstream · {} tasks", dependents.len()))
            .default_open(true)
            .id_salt(("task_dependents", ticket))
            .show(ui, |ui| {
                ui.label(crate::ui::theme::helper_text(
                    "Execution dependency edge · selected task → dependent task.",
                ));
                egui::ScrollArea::vertical()
                    .id_salt(("task_dependents_scroll", ticket))
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (path, title, status) in dependents {
                            ui.group(|ui| {
                                if ui.link(format!("→ Dependent task · {title}")).clicked() {
                                    navigate_to(ui, path);
                                }
                                node_status(ui, &status);
                            });
                        }
                    });
            });
    }
    if !planning_relations.is_empty() {
        planning_work::paint(ui, &lifecycle.feature_uid, &planning_relations);
    }
    if !decision_relations.is_empty() {
        planning_items::paint(ui, &lifecycle.feature_uid, &decision_relations);
    }
    if !opportunity_relations.is_empty() {
        opportunities::paint(ui, &opportunity_relations);
    }
    if opportunity_error.is_some() {
        ui.label(crate::ui::theme::helper_text(
            "Related opportunity records are unavailable because the planning store could not load them.",
        ));
    }
}

fn relation_link(
    ui: &mut egui::Ui,
    label: &str,
    relation: &crate::ui::task_detail::TaskRelationView,
) {
    let target_path = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
            "koolade_task_paths_by_uid",
        ))
        .and_then(|paths| paths.get(&relation.planning_item_uid).cloned())
    });
    if let Some(target_path) = target_path {
        if ui.link(format!("{label} {}", relation.title)).clicked() {
            navigate_to(ui, target_path);
        }
    } else {
        let reference = relation
            .lifecycle_task_uid
            .as_deref()
            .unwrap_or(&relation.planning_item_uid);
        ui.label(format!("{label}: {reference}"));
        ui.small(&relation.title);
        ui.small("Task details unavailable for this recorded relationship.");
    }
}

fn navigate_to(ui: &egui::Ui, target_path: String) {
    ui.ctx().data_mut(|data| {
        let selected = egui::Id::new("koolade_selected_task");
        if let Some(current) = data.get_temp::<String>(selected) {
            let history_id = egui::Id::new("koolade_task_relationship_history");
            let mut history = data.get_temp::<Vec<String>>(history_id).unwrap_or_default();
            if history.last() != Some(&current) {
                history.push(current);
            }
            data.insert_temp(history_id, history);
        }
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_navigation"),
            target_path,
        );
    });
}

fn navigate_back(ui: &egui::Ui, previous_path: String) {
    ui.ctx().data_mut(|data| {
        let id = egui::Id::new("koolade_task_relationship_history");
        let mut history = data.get_temp::<Vec<String>>(id).unwrap_or_default();
        history.pop();
        data.insert_temp(id, history);
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_navigation"),
            previous_path,
        );
    });
}

fn navigate_to_planning(ui: &egui::Ui, key: String) {
    ui.ctx().data_mut(|data| {
        if let Some(current) = data.get_temp::<String>(egui::Id::new("koolade_selected_task")) {
            data.insert_temp(
                egui::Id::new("koolade_task_relationship_work_return"),
                current,
            );
        }
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_planning_navigation"),
            key,
        );
    });
}
