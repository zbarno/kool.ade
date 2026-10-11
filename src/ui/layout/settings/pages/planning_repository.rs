use crate::ui::{ApplicationCommand, PlanningRepositoryCommand, Surface, theme};

mod conflicts;
mod migration;
mod pending;
#[cfg(test)]
mod tests;

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface) -> Option<String> {
    ui.label(theme::page_title("Planning Repository"));
    ui.label(theme::helper_text(
        "Workspace planning has one stable identity and repository, independent of code branches.",
    ));
    let Some(view) = surface.planning_repository() else {
        ui.label("Planning repository details are unavailable.");
        return None;
    };
    ui.add_space(theme::spacing::M);
    ui.label(theme::section_heading("Workspace identity"));
    ui.monospace(format!("Workspace UUID  {}", view.workspace_id));
    ui.monospace(format!(
        "Local planning path  {}",
        view.local_path.display()
    ));
    ui.monospace(format!(
        "Mode  {}",
        if view.shared { "Shared" } else { "Local" }
    ));
    if !view.canonical_ref.is_empty() {
        ui.monospace(format!("Canonical ref  {}", view.canonical_ref));
    }
    sync_status(ui, &view.sync);
    if let Some(detail) = &view.sync.detail {
        ui.label(egui::RichText::new(detail).color(theme::WARNING));
    }
    ui.add_space(theme::spacing::M);

    let remote_id = egui::Id::new("planning_repository_remote_draft");
    let mut remote = ui.ctx().data_mut(|data| {
        data.get_temp::<String>(remote_id)
            .unwrap_or_else(|| view.remote_url.clone().unwrap_or_default())
    });
    ui.label(theme::section_heading("Shared planning remote"));
    ui.label("Use your normal Git authentication. Credentials embedded in the URL are rejected.");
    ui.horizontal(|ui| {
        ui.add_sized(
            [ui.available_width() - 100.0, 30.0],
            egui::TextEdit::singleline(&mut remote)
                .hint_text("https://host/org/planning.git or git@host:org/planning.git"),
        );
        if ui.button("Save remote").clicked() {
            match surface.save_planning_remote(&remote) {
                Ok(()) => {
                    ui.ctx()
                        .data_mut(|data| data.remove_temp::<String>(remote_id));
                    surface
                        .toasts()
                        .success("Planning remote saved. Run Sync Now to confirm it.");
                }
                Err(error) => surface
                    .toasts()
                    .warning(format!("Remote was not saved: {error}")),
            }
        }
    });
    ui.ctx()
        .data_mut(|data| data.insert_temp(remote_id, remote));
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !view.sync_running && view.remote_url.is_some(),
                egui::Button::new("Sync Now"),
            )
            .clicked()
        {
            surface.dispatch(ApplicationCommand::PlanningRepository(
                PlanningRepositoryCommand::SyncNow,
            ));
        }
        if view.sync_running {
            ui.label(theme::helper_text("Syncing planning repository…"));
        }
        if matches!(
            view.sync.state,
            crate::core::planning_sync::PlanningSyncState::AuthRequired
                | crate::core::planning_sync::PlanningSyncState::ReadOnly
                | crate::core::planning_sync::PlanningSyncState::Offline
                | crate::core::planning_sync::PlanningSyncState::Pending
        ) && ui.button("Export unsynced changes…").clicked()
        {
            surface.dispatch(ApplicationCommand::PlanningRepository(
                PlanningRepositoryCommand::ExportPending,
            ));
        }
    });

    ui.add_space(theme::spacing::M);
    ui.label(theme::section_heading("Code repositories"));
    if view.repositories.is_empty() {
        ui.label("No code repositories are registered. Planning can continue in this workspace.");
    }
    for repository in &view.repositories {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong(&repository.id);
                if repository.checkout.is_some() {
                    ui.label(theme::helper_text("Configured on this machine"));
                } else {
                    ui.label(theme::helper_text("Not configured on this machine"));
                }
            });
            ui.label(theme::helper_text(if repository.remote.is_empty() {
                "No code remote registered"
            } else {
                &repository.remote
            }));
            if let Some(path) = &repository.checkout {
                ui.monospace(path.display().to_string());
                if ui.small_button("Forget local checkout mapping").clicked() {
                    surface.dispatch(ApplicationCommand::PlanningRepository(
                        PlanningRepositoryCommand::ForgetCodeCheckout {
                            repository_id: repository.id.clone(),
                        },
                    ));
                }
            } else if ui
                .small_button("Register checkout on this machine…")
                .clicked()
            {
                surface.dispatch(ApplicationCommand::PlanningRepository(
                    PlanningRepositoryCommand::MapCodeCheckout {
                        repository_id: repository.id.clone(),
                    },
                ));
            }
        });
    }
    if ui.button("Register another code repository…").clicked() {
        surface.dispatch(ApplicationCommand::HeaderAction(
            crate::ui::HeaderAction::AddCodeRepository,
        ));
    }

    if !view.sync.pending.is_empty() {
        ui.add_space(theme::spacing::M);
        ui.label(theme::section_heading("Pending changes"));
        for operation in &view.sync.pending {
            ui.group(|ui| {
                let created_at = format_timestamp_ms(operation.created_at_ms);
                ui.label(format!(
                    "Operation {} · {created_at}",
                    operation.operation_id
                ));
                ui.label(operation.summary.as_str());
                ui.label(theme::helper_text(format!(
                    "Targets: {} · {}",
                    operation.target_ids.join(", "),
                    if operation.has_local_commit {
                        "committed locally"
                    } else {
                        "not committed"
                    }
                )));
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !view.sync_running && view.remote_url.is_some(),
                            egui::Button::new("Retry sync"),
                        )
                        .clicked()
                    {
                        surface.dispatch(ApplicationCommand::PlanningRepository(
                            PlanningRepositoryCommand::SyncNow,
                        ));
                    }
                    if !operation.has_local_commit && ui.small_button("Cancel…").clicked() {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new("planning_cancel_pending"),
                                operation.operation_id,
                            );
                        });
                    }
                });
            });
        }
    }
    for conflict in &view.sync.conflicts {
        ui.group(|ui| {
            ui.label(format!(
                "Conflict · {} · {}",
                conflict.record_id, conflict.path
            ));
            ui.label(theme::helper_text(format!(
                "Fields: {} · base {} · local {} · remote {}",
                conflict.fields.join(", "),
                short_sha(&conflict.base_commit),
                conflict
                    .local_commit
                    .as_deref()
                    .map(short_sha)
                    .unwrap_or_else(|| "uncommitted".into()),
                short_sha(&conflict.remote_commit),
            )));
            if ui.button("Review conflict…").clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("planning_selected_conflict"),
                        conflict.clone(),
                    );
                });
            }
        });
    }

    if view.migration_available {
        ui.add_space(theme::spacing::M);
        ui.label(theme::section_heading("Migrate legacy project"));
        ui.label("Preview the files, identity collisions, preserved source path, and exclusions before confirming.");
        if ui.button("Preview migration…").clicked() {
            match surface.migration_preview() {
                Ok(preview) => ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("planning_migration_preview"), preview);
                }),
                Err(error) => surface
                    .toasts()
                    .warning(format!("Migration preview failed: {error}")),
            }
        }
    }

    pending::paint_cancel_modal(ui, surface);
    conflicts::paint_modal(ui, surface);
    migration::paint_modal(ui, surface);
    None
}

fn sync_status(ui: &mut egui::Ui, status: &crate::core::planning_sync::PlanningSyncStatus) {
    let (label, fill, text) = match status.state {
        crate::core::planning_sync::PlanningSyncState::Local => {
            ("Local".into(), theme::PANEL_ALT, theme::TEXT_DIM)
        }
        crate::core::planning_sync::PlanningSyncState::Synced => {
            ("Synced".into(), theme::SUCCESS, theme::BG)
        }
        crate::core::planning_sync::PlanningSyncState::Pending => (
            format!("Pending {}", status.pending_operations),
            theme::WARNING,
            theme::BG,
        ),
        crate::core::planning_sync::PlanningSyncState::Offline => {
            ("Offline".into(), theme::WARNING, theme::BG)
        }
        crate::core::planning_sync::PlanningSyncState::AuthRequired => {
            ("Auth required".into(), theme::DANGER, theme::BG)
        }
        crate::core::planning_sync::PlanningSyncState::Conflicted => (
            format!("Conflict {}", status.conflicts.len()),
            theme::DANGER,
            theme::BG,
        ),
        crate::core::planning_sync::PlanningSyncState::ReadOnly => {
            ("Permission required".into(), theme::WARNING, theme::BG)
        }
        crate::core::planning_sync::PlanningSyncState::IncompatibleSchema => {
            ("Incompatible schema".into(), theme::DANGER, theme::BG)
        }
    };
    theme::badge(ui, &label, fill, text);
    if let Some(timestamp) = status.confirmed_at_ms {
        let confirmed_at = format_timestamp_ms(timestamp);
        ui.label(theme::helper_text(format!(
            "Last confirmed sync: {confirmed_at}"
        )));
    }
}

pub(super) fn short_sha(value: &str) -> String {
    value.chars().take(10).collect()
}

fn format_timestamp_ms(timestamp_ms: u64) -> String {
    i64::try_from(timestamp_ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown time".into())
}
