use crate::{
    app::{dialogs::DlgProjectSettings, session::test_support},
    core::project_repos::ProjectManifest,
};
use std::path::PathBuf;

#[path = "tests/project_guidance.rs"]
mod project_guidance;

fn editor_frame(
    ctx: &egui::Context,
    dialog: &mut DlgProjectSettings,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::app::dialogs::paint_project_settings_card(ui, dialog);
            });
        },
    );
    output.textures_delta.clear();
    output
}

#[test]
fn repository_name_editor_renders_local_storage_and_duplicate_guidance() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-editor-ui");
    let root = fixture();
    let project = test_support::project_from(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    let ctx = egui::Context::default();
    editor_frame(&ctx, &mut dialog, Vec::new());
    let output = editor_frame(&ctx, &mut dialog, Vec::new());
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Registered repository names" => {
                Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
            }
            _ => None,
        })
        .unwrap();
    editor_frame(
        &ctx,
        &mut dialog,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    let output = editor_frame(
        &ctx,
        &mut dialog,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let rendered = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("Registered repository names"));
    assert!(rendered.contains("Optional local names; blank uses the stable ID"));
    let mut people = crate::app::dialogs::DlgSettings::from_project(&project);
    let mut people_output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::app::dialogs::paint_settings_card(ui, &mut people);
            });
        },
    );
    people_output.textures_delta.clear();
    assert!(!people_output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Text(text) if text.galley.text().contains("Registered repository names")
    )));
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "koolade-settings-repository-names-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .unwrap()
    };
    assert!(git(&["init", "-q", "-b", "main"]).success());
    assert!(git(&["config", "user.name", "Settings Test"]).success());
    assert!(git(&["config", "user.email", "settings@example.test"]).success());
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "// Settings fixture source.\n").unwrap();
    assert!(git(&["add", "src/lib.rs"]).success());
    assert!(git(&["commit", "-q", "-m", "source baseline"]).success());
    let store = test_support::planning_store_for(&root);
    let manifest = ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            crate::core::project_repos::PROJECT_FILE,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    root
}

fn git_head(root: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn settings_save_persists_trimmed_repository_name_locally_without_git_commit() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-save");
    let root = fixture();
    let mut project = test_support::project_from(&root);
    let initial_head = git_head(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    assert_eq!(dialog.repositories[0].name, "");
    dialog.repositories[0].name = "  Product API  ".into();
    assert!(dialog.apply(&mut project).unwrap().is_some());
    assert_eq!(
        project.state.repositories.repositories[0]
            .display_name
            .as_deref(),
        Some("Product API")
    );
    let manifest = ProjectManifest::load(&project.state.planning_store).unwrap();
    assert_eq!(
        manifest.repositories[0].display_name.as_deref(),
        Some("Product API")
    );
    assert_eq!(
        git_head(&root),
        initial_head,
        "private settings must not create Git history"
    );
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
// Production boundary: exercises the Project Settings apply path that writes
// repository registration and checkout mapping together. The core store test
// alone cannot prove dialog state reaches the transaction. Uses local Git and
// a temporary repository only; setup cost is low.
fn settings_registers_a_secondary_repository_only_in_private_storage() {
    let _shield = crate::core::gitops::test_support::shield("secondary-repository-settings");
    let root = fixture();
    let api = root.with_file_name(format!(
        "{}-api",
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&api).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&api)
            .status()
            .unwrap()
    };
    assert!(git(&["init", "-q", "-b", "main"]).success());
    assert!(
        git(&[
            "remote",
            "add",
            "origin",
            "https://example.test/team/api.git",
        ])
        .success()
    );
    let mut project = test_support::project_from(&root);
    let initial_head = git_head(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.new_repository_id = "api".into();
    dialog.new_repository_role = "Backend API".into();
    dialog.new_repository_remote = "https://example.test/team/api.git".into();
    dialog.new_repository_checkout = Some(api.clone());
    dialog.register_repository_requested = true;

    assert!(dialog.apply(&mut project).unwrap().is_some());
    let manifest = ProjectManifest::load(&project.state.planning_store).unwrap();
    assert!(manifest.repositories.iter().any(|repo| repo.id == "api"));
    assert_eq!(
        manifest
            .target(&project.state.planning_store, &root, "api")
            .unwrap(),
        api.canonicalize().unwrap()
    );
    assert_eq!(git_head(&root), initial_head);
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(status.stdout.is_empty());
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(api);
}

#[test]
fn project_harness_route_is_saved_locally_and_restored_without_git_commit() {
    let _shield = crate::core::gitops::test_support::shield("project-harness-route-save");
    let root = fixture();
    let mut project = test_support::project_from(&root);
    let initial_head = git_head(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.harness_routes.insert(
        crate::persistence::harness_settings::MANAGER.into(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "claude".into(),
            model: Some("synthetic-model".into()),
        },
    );

    assert!(dialog.apply(&mut project).unwrap().is_some());
    assert_eq!(
        project.state.harness_routes.routes[crate::persistence::harness_settings::MANAGER].harness,
        "claude"
    );
    let reloaded = test_support::project_from(&root);
    assert_eq!(reloaded.state.harness_routes, project.state.harness_routes);
    assert_eq!(
        git_head(&root),
        initial_head,
        "private settings must not create Git history"
    );
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn clearing_a_saved_repository_name_persists_none_and_preserves_identity() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-clear");
    let root = fixture();
    let mut project = test_support::project_from(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.repositories[0].name = "Product API".into();
    dialog.apply(&mut project).unwrap();

    let mut project = test_support::project_from(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    assert_eq!(dialog.repositories[0].name, "Product API");
    dialog.repositories[0].name.clear();
    dialog.apply(&mut project).unwrap();

    let manifest = ProjectManifest::load(&project.state.planning_store).unwrap();
    assert_eq!(manifest.repositories[0].display_name, None);
    assert_eq!(manifest.repositories[0].id, "root");
    assert_eq!(manifest.repositories[0].role, "Primary code");
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_edit_is_discarded_without_changing_the_manifest_or_identity() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-invalid");
    let root = fixture();
    let mut project = test_support::project_from(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.repositories[0].name = "Valid name".into();
    dialog.apply(&mut project).unwrap();
    let manifest_path = project.state.planning_store.layout().project_manifest();
    let before = std::fs::read(&manifest_path).unwrap();

    let mut project = test_support::project_from(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.repositories[0].name = "x".repeat(41);
    let error = dialog.apply(&mut project).unwrap_err();
    assert!(error.detail().contains("40 characters"));
    assert_eq!(dialog.repositories[0].name, "Valid name");
    assert_eq!(std::fs::read(&manifest_path).unwrap(), before);
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}
