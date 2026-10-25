use crate::ui::{Surface, task_detail};

mod recipe_adapter;
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
        target_override::paint(ui, surface, ticket, revision, &runtime.prepared_dependencies);
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
            let applicability = match target.qa_applicability {
                crate::project_runtime_capability::QaApplicability::Visual => {
                    "Visual QA surface; target readiness and driver availability are checked separately."
                }
                crate::project_runtime_capability::QaApplicability::ServiceEvidence => {
                    "Headless service: visual QA does not apply. A target-bound applicability assessment and Manager signoff can be recorded; this does not certify API or service tests."
                }
                crate::project_runtime_capability::QaApplicability::StaticReviewOnly => {
                    "Static review only; runtime QA is not currently applicable."
                }
                crate::project_runtime_capability::QaApplicability::Unsupported => {
                    "Unsupported surface: no runtime QA driver is available."
                }
            };
            ui.label(egui::RichText::new(applicability).small());
            if !target.diagnostics.is_empty() {
                ui.label(egui::RichText::new("Capability gaps and suggested next steps").strong());
                ui.label(egui::RichText::new(
                    "These suggestions do not grant recipe approval or prove runtime readiness.",
                ).small().weak());
                for diagnostic in &target.diagnostics {
                    ui.label(format!("• {diagnostic}"));
                }
            }
            ui.label(if target.recipes.is_empty() {
                "No approved build, test, or start recipes.".into()
            } else {
                target.recipes.clone()
            });
            if target.selected {
                if target.app_surface == crate::project_runtime::AppSurface::UnsupportedTarget {
                    ui.label("Recipes cannot run for an unsupported target surface.");
                } else {
                    for operation in [
                        task_detail::RuntimeRecipeOperation::Build,
                        task_detail::RuntimeRecipeOperation::Test,
                        task_detail::RuntimeRecipeOperation::Start,
                    ] {
                        recipe_form(ui, surface, ticket, target, operation, revision);
                    }
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
        if let Some(suggestion) = recipe_adapter::suggestion(
            target.detected_language.as_deref(),
            operation,
        ) && ui
            .button(format!("Fill {} adapter template", suggestion.label))
            .clicked()
        {
            draft.executable_id = suggestion.executable_id.into();
            draft.argv_lines = suggestion.argv.join("\n");
        }
        if operation == task_detail::RuntimeRecipeOperation::Test
            && target.recipes.is_empty()
            && ui
                .button("Apply and approve trusted test adapter")
                .clicked()
        {
            dispatch(
                surface,
                task_detail::Command::ApplyRuntimeAdapter {
                    ticket: ticket.to_owned(),
                    target_id: target.target_id.clone(),
                    operation,
                    expected_manifest_revision: revision,
                },
            );
        }
        if target.detected_language.is_some() {
            ui.label("Templates are hints. The trusted test adapter uses a fixed argv and requires a separate explicit action; it does not run the recipe.");
        }
        ui.horizontal(|ui| {
            ui.label("Executable ID");
            ui.text_edit_singleline(&mut draft.executable_id);
        });
        ui.label("Arguments, one per line (each line is a separate argv item)");
        ui.text_edit_multiline(&mut draft.argv_lines);
        if operation == task_detail::RuntimeRecipeOperation::Start {
            match target.app_surface {
                crate::project_runtime::AppSurface::Web => {
                    readiness_fields(ui, &mut draft, "Approved preview port", "HTTP readiness path");
                    ui.label("Web readiness is restricted to the approved preview port.");
                }
                crate::project_runtime::AppSurface::HeadlessService => {
                    readiness_fields(ui, &mut draft, "Local service health port", "Health path");
                    ui.label("Service readiness checks the approved local health endpoint; it does not count as visual QA.");
                }
                crate::project_runtime::AppSurface::LinuxDesktop => {
                    ui.label("Desktop readiness is checked by the launched process.");
                }
                crate::project_runtime::AppSurface::UnsupportedTarget => {
                    ui.label("Unsupported surfaces cannot start runtime recipes.");
                }
            }
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

fn readiness_fields(
    ui: &mut egui::Ui,
    draft: &mut RecipeDraft,
    port_label: &str,
    path_label: &str,
) {
    ui.horizontal(|ui| {
        ui.label(port_label);
        ui.text_edit_singleline(&mut draft.readiness_port);
    });
    ui.horizontal(|ui| {
        ui.label(path_label);
        ui.text_edit_singleline(&mut draft.readiness_path);
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
