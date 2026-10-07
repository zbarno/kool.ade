use super::Project;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryNameRow {
    pub id: String,
    pub role: String,
    pub name: String,
    pub original_name: String,
}

pub(super) fn rows(
    manifest: &crate::core::project_repos::ProjectManifest,
) -> Vec<RepositoryNameRow> {
    manifest
        .repositories
        .iter()
        .map(|repository| RepositoryNameRow {
            id: repository.id.clone(),
            role: repository.role.clone(),
            name: repository.display_name.clone().unwrap_or_default(),
            original_name: repository.display_name.clone().unwrap_or_default(),
        })
        .collect()
}

pub(super) fn persist(
    project: &Project,
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
    if &updated == current {
        for row in rows {
            row.original_name.clone_from(&row.name);
        }
        return Ok(false);
    }
    crate::core::project_repos::save_display_names(&project.state.repo_root, updated)
        .map_err(crate::AppError::from)?;
    for row in rows {
        row.name = row.name.trim().to_owned();
        row.original_name.clone_from(&row.name);
    }
    Ok(true)
}

pub(super) fn paint(ui: &mut egui::Ui, rows: &mut [RepositoryNameRow]) {
    ui.collapsing("Registered repository names", |ui| {
        ui.label(
            egui::RichText::new("Optional shared names; blank uses the stable ID. Duplicate names are allowed and disambiguated.")
                .size(11.0)
                .weak(),
        );
        for row in rows {
            ui.horizontal(|ui| {
                ui.label(format!("{} · {}", row.id, row.role));
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
