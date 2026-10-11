use crate::ui::{ApplicationCommand, PlanningRepositoryCommand, Surface};

pub(super) fn paint_modal(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    let id = egui::Id::new("planning_selected_conflict");
    let conflict = ui
        .ctx()
        .data_mut(|data| data.get_temp::<crate::core::planning_sync::PlanningSyncConflict>(id));
    let Some(conflict) = conflict else {
        return;
    };
    let mut local_value = conflict.local_value.clone().unwrap_or_default();
    let mut remote_value = conflict.remote_value.clone().unwrap_or_default();
    let manual_id = egui::Id::new((
        "planning_manual_conflict",
        conflict.operation_id,
        conflict.path.clone(),
    ));
    let mut manual = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(manual_id).unwrap_or_default());
    let action = std::cell::RefCell::new(None);
    let cancelled = std::cell::Cell::new(false);
    let closed =
        crate::ui::overlays::show_modal(ui, true, "Resolve planning conflict", 760.0, |ui| {
            ui.label(format!("Record {} · {}", conflict.record_id, conflict.path));
            ui.label(format!(
                "Conflicting fields: {}",
                conflict.fields.join(", ")
            ));
            ui.label(format!(
                "Base {} · local {} · remote {}",
                super::short_sha(&conflict.base_commit),
                conflict
                    .local_commit
                    .as_deref()
                    .map(super::short_sha)
                    .unwrap_or_else(|| "uncommitted".into()),
                super::short_sha(&conflict.remote_commit)
            ));
            ui.label("Local value");
            ui.add(
                egui::TextEdit::multiline(&mut local_value)
                    .desired_rows(5)
                    .interactive(false),
            );
            ui.label("Remote value");
            ui.add(
                egui::TextEdit::multiline(&mut remote_value)
                    .desired_rows(5)
                    .interactive(false),
            );
            ui.separator();
            ui.label("Manually merged value (JSON for state records, Markdown for item content)");
            ui.add(
                egui::TextEdit::multiline(&mut manual)
                    .desired_rows(7)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui.button("Use local").clicked() {
                    *action.borrow_mut() = Some((
                        crate::core::planning_sync::PlanningConflictChoice::Local,
                        None,
                    ));
                }
                if ui.button("Use remote").clicked() {
                    *action.borrow_mut() = Some((
                        crate::core::planning_sync::PlanningConflictChoice::Remote,
                        None,
                    ));
                }
                if ui.button("Use manual merge").clicked() {
                    *action.borrow_mut() = Some((
                        crate::core::planning_sync::PlanningConflictChoice::Manual,
                        Some(manual.clone()),
                    ));
                }
                if ui.button("Cancel").clicked() {
                    cancelled.set(true);
                }
            });
        });
    ui.ctx()
        .data_mut(|data| data.insert_temp(manual_id, manual));
    if cancelled.get() {
        ui.ctx().data_mut(|data| {
            data.remove_temp::<String>(manual_id);
            data.remove_temp::<crate::core::planning_sync::PlanningSyncConflict>(id);
        });
    } else if let Some((choice, value)) = action.into_inner() {
        surface.dispatch(ApplicationCommand::PlanningRepository(
            PlanningRepositoryCommand::ResolveConflict {
                operation_id: conflict.operation_id,
                path: conflict.path,
                choice,
                manual_value: value,
            },
        ));
        ui.ctx().data_mut(|data| {
            data.remove_temp::<crate::core::planning_sync::PlanningSyncConflict>(id)
        });
    } else if closed {
        ui.ctx().data_mut(|data| {
            data.remove_temp::<crate::core::planning_sync::PlanningSyncConflict>(id)
        });
    }
}
