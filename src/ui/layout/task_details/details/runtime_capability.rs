use crate::ui::{Surface, task_detail};

mod target_override;

#[derive(Clone, Default)]
struct RecipeDraft {
    executable_id: String,
    argv_lines: String,
    artifact_paths: String,
    readiness_port: String,
    readiness_path: String,
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    let Some(runtime) = view.runtime_capability.as_ref() else {
        return;
    };
    ui.group(|ui| {
        ui.label(egui::RichText::new("Project runtime").strong());
        if !runtime.languages.is_empty() || !runtime.frameworks.is_empty() {
            ui.label(format!(
                "Detected: {}{}",
                runtime.languages.join(", "),
                if runtime.frameworks.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", runtime.frameworks.join(", "))
                }
            ));
        }
        if let Some(source) = runtime.source_sha.as_deref() {
            ui.label(format!("Source: {}", &source[..source.len().min(12)]));
        }
        if let Some(message) = runtime.message.as_deref() {
            ui.label(egui::RichText::new(message).small().weak());
        }
        if ui
            .add_enabled(runtime.source_sha.is_some(), egui::Button::new("Discover project targets"))
            .on_hover_text("Reads project metadata only. It never runs manifest scripts.")
            .clicked()
        {
            dispatch(surface, task_detail::Command::DiscoverRuntimeTargets {
                ticket: ticket.to_owned(),
            });
        }
        let Some(revision) = runtime.manifest_revision else {
            return;
        };
        target_override::paint(ui, surface, ticket, revision);
        if runtime.targets.is_empty() {
            ui.label("No targets are recorded for this task.");
            return;
        }
        let current = runtime
            .targets
            .iter()
            .find(|target| target.selected)
            .or_else(|| (runtime.targets.len() == 1).then(|| &runtime.targets[0]));
        let mut selected_id = current.map(|target| target.target_id.clone());
        egui::ComboBox::from_id_salt(("project_runtime_target", ticket))
            .selected_text(
                current
                    .map(|target| target.label.as_str())
                    .unwrap_or("Select a project target"),
            )
            .show_ui(ui, |ui| {
                for target in &runtime.targets {
                    if ui
                        .selectable_value(
                            &mut selected_id,
                            Some(target.target_id.clone()),
                            &target.label,
                        )
                        .clicked()
                    {
                        dispatch(
                            surface,
                            task_detail::Command::SelectRuntimeTarget {
                                ticket: ticket.to_owned(),
                                target_id: target.target_id.clone(),
                                expected_manifest_revision: revision,
                            },
                        );
                    }
                }
            });
        if let Some(target) = selected_id
            .as_deref()
            .and_then(|id| runtime.targets.iter().find(|target| target.target_id == id))
        {
            ui.label(format!("Capability: {}", target.state));
            for diagnostic in &target.diagnostics {
                ui.label(egui::RichText::new(diagnostic).small().weak());
            }
            if let Some(diagnostic) = target_surface_diagnostic(target.surface) {
                ui.label(egui::RichText::new(diagnostic).small().weak());
            }
            ui.label(if target.recipes.is_empty() {
                "No approved build, test, or start recipes.".into()
            } else {
                target.recipes.clone()
            });
            if target.selected {
                if target.recipes.is_empty()
                    && runtime.languages.iter().any(|language| {
                        crate::project_runtime::trusted_adapter_template(
                            Some(language),
                            crate::project_runtime::RuntimeOperation::Test,
                        )
                        .is_some()
                    })
                    && ui.button("Apply trusted test adapter").clicked()
                {
                    dispatch(surface, task_detail::Command::ApplyRuntimeTestAdapter {
                        ticket: ticket.to_owned(),
                        target_id: target.target_id.clone(),
                        expected_manifest_revision: revision,
                    });
                }
                for operation in [
                    task_detail::RuntimeRecipeOperation::Build,
                    task_detail::RuntimeRecipeOperation::Test,
                    task_detail::RuntimeRecipeOperation::Start,
                ] {
                    recipe_form(ui, surface, ticket, target, operation, revision);
                }
            } else if runtime.targets.len() > 1 {
                ui.label("Select this target explicitly before approving recipes.");
            }
        }
        ui.label(egui::RichText::new(
            "Approved recipes run as bounded argv inside the task's Docker execution session. Discovery never executes project scripts.",
        ).small().weak());
    });
}

fn target_surface_diagnostic(surface: crate::project_runtime::AppSurface) -> Option<&'static str> {
    match surface {
        crate::project_runtime::AppSurface::HeadlessService => Some(
            "Headless service: browser and desktop visual QA do not apply. API and integration evidence is not yet connected.",
        ),
        crate::project_runtime::AppSurface::UnsupportedTarget => Some(
            "This target is unsupported by the current Linux web and desktop drivers. Planning and static review remain available.",
        ),
        crate::project_runtime::AppSurface::Web
        | crate::project_runtime::AppSurface::LinuxDesktop => None,
    }
}

fn recipe_form(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    target: &task_detail::RuntimeTargetChoice,
    operation: task_detail::RuntimeRecipeOperation,
    revision: u64,
) {
    let label = match operation {
        task_detail::RuntimeRecipeOperation::Build => "build",
        task_detail::RuntimeRecipeOperation::Test => "test",
        task_detail::RuntimeRecipeOperation::Start => "start/readiness",
    };
    ui.collapsing(format!("Approve {label} recipe"), |ui| {
        let id = egui::Id::new(("runtime_recipe_draft", ticket, &target.target_id, label));
        let mut draft = ui
            .ctx()
            .data_mut(|data| data.get_temp::<RecipeDraft>(id))
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Executable ID");
            ui.text_edit_singleline(&mut draft.executable_id);
        });
        ui.label("Arguments, one per line (each line is a separate argv item)");
        ui.text_edit_multiline(&mut draft.argv_lines);
        if operation == task_detail::RuntimeRecipeOperation::Start {
            ui.horizontal(|ui| {
                ui.label("Web readiness port");
                ui.text_edit_singleline(&mut draft.readiness_port);
            });
            ui.horizontal(|ui| {
                ui.label("HTTP readiness path");
                ui.text_edit_singleline(&mut draft.readiness_path);
            });
            ui.label("Web targets require an approved local port; Linux desktop targets use process readiness.");
        }
        if operation == task_detail::RuntimeRecipeOperation::Build {
            ui.label("Expected output paths relative to the app root, one per line");
            ui.text_edit_multiline(&mut draft.artifact_paths);
        }
        if ui
            .add_enabled(
                !draft.executable_id.trim().is_empty() && !draft.argv_lines.trim().is_empty(),
                egui::Button::new(format!("Approve {label} recipe")),
            )
            .clicked()
        {
            dispatch(
                surface,
                task_detail::Command::ApproveRuntimeRecipe {
                    ticket: ticket.to_owned(),
                    target_id: target.target_id.clone(),
                    operation,
                    approval_proof:
                        crate::ui::layout::RuntimeTargetApprovalProof::from_user_approval_click(),
                    executable_id: draft.executable_id.trim().to_owned(),
                    argv_lines: draft.argv_lines.clone(),
                    artifact_paths: draft.artifact_paths.clone(),
                    readiness_port: draft.readiness_port.clone(),
                    readiness_path: draft.readiness_path.clone(),
                    expected_manifest_revision: revision,
                },
            );
        }
        ui.ctx().data_mut(|data| data.insert_temp(id, draft));
    });
}

fn dispatch(surface: &mut dyn Surface, command: task_detail::Command) {
    surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(command));
}

#[cfg(test)]
pub(in crate::ui::layout::task_details) fn seed_custom_target_repository_root_draft(
    context: &egui::Context,
    ticket: &str,
    app_surface: crate::project_runtime::AppSurface,
) {
    target_override::seed_test_repository_root_draft(context, ticket, app_surface);
}

#[cfg(test)]
mod surface_diagnostic_tests {
    use super::target_surface_diagnostic;
    use crate::project_runtime::AppSurface;

    #[test]
    fn headless_and_unsupported_targets_explain_qa_applicability() {
        let headless = target_surface_diagnostic(AppSurface::HeadlessService).unwrap();
        assert!(headless.contains("API and integration evidence is not yet connected"));
        assert!(
            target_surface_diagnostic(AppSurface::UnsupportedTarget)
                .unwrap()
                .contains("Planning and static review remain available")
        );
        assert_eq!(target_surface_diagnostic(AppSurface::Web), None);
        assert_eq!(target_surface_diagnostic(AppSurface::LinuxDesktop), None);
    }
}
