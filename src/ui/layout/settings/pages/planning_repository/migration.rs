use crate::ui::{Surface, theme};

pub(super) fn paint_modal(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    let id = egui::Id::new("planning_migration_preview");
    let preview = ui
        .ctx()
        .data_mut(|data| data.get_temp::<crate::core::managed_workspace::MigrationPreview>(id));
    let Some(preview) = preview else {
        return;
    };
    let confirm = std::cell::Cell::new(false);
    let dismiss = std::cell::Cell::new(false);
    let mut record_ids = preview.record_ids.join("\n");
    let mut exclusions = preview.exclusions.join("\n");
    let error_id = egui::Id::new("planning_migration_confirm_error");
    let error = ui.ctx().data_mut(|data| data.get_temp::<String>(error_id));
    let closed = crate::ui::overlays::show_modal(
        ui,
        true,
        "Review legacy migration",
        760.0,
        |ui| {
            ui.label(format!(
                "Source: {} @ {}",
                preview.source_root.display(),
                super::short_sha(&preview.source_commit)
            ));
            ui.label(format!(
                "Files: {} · stable record IDs: {}",
                preview.file_count,
                preview.record_ids.len()
            ));
            ui.label(format!(
                "Workspace UUID: {}",
                preview
                    .workspace_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "new identity".into())
            ));
            ui.label(format!(
                "Preserved recovery path: {}",
                preview.backup_path.display()
            ));
            ui.label(theme::helper_text(
                "Rollback is available from Workspace > Return to embedded planning. Keep the original checkout and preserved recovery path until the migrated workspace is verified.",
            ));
            ui.label("Stable IDs");
            ui.add(
                egui::TextEdit::multiline(&mut record_ids)
                    .desired_rows(4)
                    .interactive(false),
            );
            ui.label(format!("Branch collisions: {}", preview.collisions.len()));
            for collision in &preview.collisions {
                ui.label(format!(
                    "{} · {} · {}",
                    collision.path,
                    collision.record_id.as_deref().unwrap_or("no record ID"),
                    collision.kind
                ));
            }
            ui.label("Excluded private artifacts");
            ui.add(
                egui::TextEdit::multiline(&mut exclusions)
                    .desired_rows(3)
                    .interactive(false),
            );
            if let Some(error) = error {
                ui.label(egui::RichText::new(error).color(theme::DANGER));
            }
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    dismiss.set(true);
                }
                if ui
                    .add_enabled(
                        preview.collisions.is_empty(),
                        egui::Button::new("Confirm migration"),
                    )
                    .clicked()
                {
                    confirm.set(true);
                }
            });
        },
    );
    if confirm.get() {
        match surface.confirm_planning_migration(&preview) {
            Ok(()) => {
                ui.ctx().data_mut(|data| {
                    data.remove_temp::<crate::core::managed_workspace::MigrationPreview>(id);
                    data.remove_temp::<String>(error_id);
                });
            }
            Err(error) => {
                ui.ctx().data_mut(|data| data.insert_temp(error_id, error));
            }
        }
    } else if closed || dismiss.get() {
        ui.ctx().data_mut(|data| {
            data.remove_temp::<crate::core::managed_workspace::MigrationPreview>(id)
        });
    }
}
