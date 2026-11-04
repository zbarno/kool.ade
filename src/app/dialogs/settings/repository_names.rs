use super::Project;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryNameRow {
    pub id: String,
    pub role: String,
    pub name: String,
    pub original_name: String,
    pub checkout: Option<std::path::PathBuf>,
    pub original_checkout: Option<std::path::PathBuf>,
}

pub(super) fn rows(
    manifest: &crate::core::project_repos::ProjectManifest,
    checkouts: &crate::core::project_repos::RepositoryCheckoutMapV1,
) -> Vec<RepositoryNameRow> {
    manifest
        .repositories
        .iter()
        .map(|repository| RepositoryNameRow {
            id: repository.id.clone(),
            role: repository.role.clone(),
            name: repository.display_name.clone().unwrap_or_default(),
            original_name: repository.display_name.clone().unwrap_or_default(),
            checkout: checkouts.checkouts.get(&repository.id).cloned(),
            original_checkout: checkouts.checkouts.get(&repository.id).cloned(),
        })
        .collect()
}

pub(super) fn persist(
    project: &mut Project,
    rows: &mut [RepositoryNameRow],
) -> Result<bool, crate::AppError> {
    let current = &project.state.repositories;
    if rows.len() != current.repositories.len() {
        return Err(crate::AppError::Other(
            "Repository list changed while settings were open; reopen settings and retry.".into(),
        ));
    }
    let mut updated = current.clone();
    for repository in &mut updated.repositories {
        let Some(row) = rows.iter_mut().find(|row| row.id == repository.id) else {
            return Err(crate::AppError::Other(format!(
                "Repository {} is no longer available; reopen settings and retry.",
                repository.id
            )));
        };
        let normalized = match crate::core::project_repos::normalize_display_name(&row.name) {
            Ok(name) => name,
            Err(error) => {
                row.name.clone_from(&row.original_name);
                return Err(crate::AppError::Other(error.to_string()));
            }
        };
        row.name = normalized.clone().unwrap_or_default();
        repository.display_name = normalized;
    }
    updated.validate().map_err(crate::AppError::from)?;
    let names_changed = &updated != current;
    if names_changed {
        let revision = crate::core::project_repos::save_display_names_expected(
            &project.state.planning_store,
            updated.clone(),
            &project.state.baseline_planning_revision,
        )
        .map_err(crate::AppError::from)?;
        project.state.repositories = updated.clone();
        project.state.baseline_planning_revision = revision;
    }
    let mut changed = names_changed;
    for row in rows.iter_mut().filter(|row| row.id != "root") {
        if row.checkout != row.original_checkout
            && let Some(path) = row.checkout.as_deref()
        {
            let revision = crate::core::project_repos::connect_checkout(
                &project.state.planning_store,
                &updated,
                &project.state.repo_root,
                &row.id,
                path,
                &project.state.baseline_planning_revision,
            )
            .map_err(crate::AppError::from)?;
            project.state.baseline_planning_revision = revision;
            row.original_checkout = row.checkout.clone();
            changed = true;
        }
    }
    if !changed {
        for row in rows {
            row.original_name.clone_from(&row.name);
        }
        return Ok(false);
    }
    project.state.repositories = updated;
    for row in rows {
        row.name = row.name.trim().to_owned();
        row.original_name.clone_from(&row.name);
    }
    Ok(true)
}

pub(super) fn paint(ui: &mut egui::Ui, rows: &mut [RepositoryNameRow]) {
    ui.collapsing("Registered repository names", |ui| {
        ui.label(
            egui::RichText::new("Optional local names; blank uses the stable ID. Duplicate names are allowed and disambiguated.")
                .size(11.0)
                .weak(),
        );
        for row in rows {
            ui.vertical(|ui| {
                ui.label(format!("{} · {}", row.id, row.role));
                if row.id != "root" {
                    if let Some(path) = &row.checkout {
                        ui.label(path.display().to_string());
                    } else {
                        ui.label("No local checkout connected");
                    }
                    if ui.button(format!("Choose checkout for {}…", row.id)).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        row.checkout = Some(path);
                    }
                }
                ui.add_sized(
                    [ui.available_width(), 26.0],
                    egui::TextEdit::singleline(&mut row.name)
                        .hint_text("Optional display name (blank clears)")
                        .char_limit(120),
                );
            });
        }
    });
}

#[cfg(test)]
mod tests;
