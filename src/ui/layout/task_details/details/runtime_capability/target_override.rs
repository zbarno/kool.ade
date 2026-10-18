use crate::{
    project_runtime::AppSurface,
    ui::{Surface, task_detail},
};

#[derive(Clone)]
struct Draft {
    surface: AppSurface,
    root: String,
    artifact_paths: String,
    required_dependency_ids: String,
    build: task_detail::RuntimeRecipeInput,
    test: task_detail::RuntimeRecipeInput,
    start: task_detail::RuntimeRecipeInput,
    readiness_port: String,
    readiness_path: String,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            surface: AppSurface::Web,
            root: ".".into(),
            artifact_paths: String::new(),
            required_dependency_ids: String::new(),
            build: Default::default(),
            test: Default::default(),
            start: Default::default(),
            readiness_port: String::new(),
            readiness_path: "/".into(),
        }
    }
}

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, ticket: &str, revision: u64) {
    egui::CollapsingHeader::new("Add a custom application target")
        .id_salt(("runtime_target_override", ticket))
        .show(ui, |ui| {
            ui.label("Use this for an unrecognized app root or an ambiguous monorepo target.");
            let id = egui::Id::new(("runtime_target_draft", ticket));
            let mut draft = ui
                .ctx()
                .data_mut(|data| data.get_temp::<Draft>(id))
                .unwrap_or_default();
            egui::ComboBox::from_id_salt(("runtime_target_surface", ticket))
                .selected_text(surface_label(draft.surface))
                .show_ui(ui, |ui| {
                    for choice in [
                        AppSurface::Web,
                        AppSurface::LinuxDesktop,
                        AppSurface::HeadlessService,
                    ] {
                        ui.selectable_value(&mut draft.surface, choice, surface_label(choice));
                    }
                });
            ui.horizontal(|ui| {
                ui.label("App root relative to repository");
                ui.text_edit_singleline(&mut draft.root);
            });
            let repository_root_error =
                crate::project_runtime::validate_custom_target_root(draft.root.trim()).err();
            if let Some(message) = repository_root_error {
                ui.colored_label(egui::Color32::LIGHT_RED, message);
            }
            recipe_fields(ui, "Build recipe", &mut draft.build);
            ui.label("Build artifact paths relative to the app root, one per line");
            ui.text_edit_multiline(&mut draft.artifact_paths);
            ui.label("Prepared dependency broker request IDs required by this target, one per line");
            ui.text_edit_multiline(&mut draft.required_dependency_ids);
            ui.label("Copy request IDs from Dependency activity. QA requires successful preparation for this task.");
            recipe_fields(ui, "Test recipe", &mut draft.test);
            recipe_fields(ui, "Start recipe", &mut draft.start);
            if draft.surface == AppSurface::Web || draft.surface == AppSurface::HeadlessService {
                ui.horizontal(|ui| {
                    ui.label("Approved readiness port");
                    ui.text_edit_singleline(&mut draft.readiness_port);
                });
                ui.horizontal(|ui| {
                    ui.label("Readiness path");
                    ui.text_edit_singleline(&mut draft.readiness_path);
                });
            }
            ui.label(
            "Arguments are separate argv items, one per line. Shell evaluation is not supported.",
        );
            let has_recipe = [&draft.build, &draft.test, &draft.start]
                .iter()
                .any(|recipe| {
                    !recipe.executable_id.trim().is_empty() && !recipe.argv_lines.trim().is_empty()
                });
            let has_build = !draft.build.executable_id.trim().is_empty()
                && !draft.build.argv_lines.trim().is_empty();
            let has_start = !draft.start.executable_id.trim().is_empty()
                && !draft.start.argv_lines.trim().is_empty();
            if has_start && !has_build {
                ui.label("Start requires a Build recipe so its outputs can be verified.");
            }
            let has_artifacts = !draft
                .artifact_paths
                .lines()
                .all(|line| line.trim().is_empty());
            if ui
                .add_enabled(
                    !draft.root.trim().is_empty()
                        && repository_root_error.is_none()
                        && has_recipe
                        && (!has_start || has_build)
                        && has_artifacts,
                    egui::Button::new("Approve and save custom target"),
                )
                .clicked()
            {
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    task_detail::Command::AddRuntimeTarget {
                        ticket: ticket.to_owned(),
                        app_surface: draft.surface,
                        project_relative_root: draft.root.trim().to_owned(),
                        artifact_paths: draft.artifact_paths.clone(),
                        required_dependency_ids: draft.required_dependency_ids.clone(),
                        build: draft.build.clone(),
                        test: draft.test.clone(),
                        start: draft.start.clone(),
                        approval_proof:
                            crate::ui::layout::RuntimeTargetApprovalProof::from_user_approval_click(
                            ),
                        readiness_port: draft.readiness_port.clone(),
                        readiness_path: draft.readiness_path.clone(),
                        expected_manifest_revision: revision,
                    },
                ));
            }
            ui.ctx().data_mut(|data| data.insert_temp(id, draft));
        });
}

#[cfg(test)]
pub(super) fn seed_test_repository_root_draft(
    context: &egui::Context,
    ticket: &str,
    surface: AppSurface,
) {
    let draft = Draft {
        surface,
        root: "./".into(),
        artifact_paths: "dist/index.html".into(),
        required_dependency_ids: "dependency-prepared-fixture".into(),
        build: task_detail::RuntimeRecipeInput {
            executable_id: "node".into(),
            argv_lines: "build.js\n--release".into(),
        },
        start: task_detail::RuntimeRecipeInput {
            executable_id: "node".into(),
            argv_lines: "server.js".into(),
        },
        readiness_port: "8080".into(),
        readiness_path: "/health".into(),
        ..Draft::default()
    };
    context.data_mut(|data| {
        data.insert_temp(egui::Id::new(("runtime_target_draft", ticket)), draft);
    });
}

fn recipe_fields(ui: &mut egui::Ui, label: &str, recipe: &mut task_detail::RuntimeRecipeInput) {
    ui.collapsing(label, |ui| {
        ui.horizontal(|ui| {
            ui.label("Executable ID");
            ui.text_edit_singleline(&mut recipe.executable_id);
        });
        ui.label("Arguments, one per line");
        ui.text_edit_multiline(&mut recipe.argv_lines);
    });
}

fn surface_label(surface: AppSurface) -> &'static str {
    match surface {
        AppSurface::Web => "Web",
        AppSurface::LinuxDesktop => "Linux desktop",
        AppSurface::HeadlessService => "Headless service",
        AppSurface::UnsupportedTarget => "Unsupported",
    }
}
