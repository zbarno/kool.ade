use crate::core::project_repos::{ProjectManifest, display_labels};

#[cfg(test)]
mod tests;

pub(super) fn paint(ui: &mut egui::Ui, path: &mut String) {
    let candidate = std::path::Path::new(path.trim());
    let Ok(root) = candidate.canonicalize() else {
        return;
    };
    let manifest_path = crate::artifacts::layout::ArtifactLayout::new(&root).project_manifest();
    if !manifest_path.is_file() {
        return;
    }
    let Ok(manifest) = ProjectManifest::load(&root) else {
        return;
    };
    let labels = display_labels(&manifest.repositories);
    ui.menu_button("Registered repositories", |ui| {
        for (repository, label) in manifest.repositories.iter().zip(labels) {
            if ui.button(label.clone()).clicked() {
                match manifest.target_if_available(&root, &repository.id) {
                    Ok(Some(target)) => *path = target.to_string_lossy().into_owned(),
                    Ok(None) => {
                        ui.label(format!("No local checkout is mapped for {}.", label));
                    }
                    Err(error) => {
                        ui.label(format!("Could not open {}: {error}", label));
                    }
                }
            }
        }
    });
}
