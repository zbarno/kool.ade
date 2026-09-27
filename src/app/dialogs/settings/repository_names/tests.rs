use crate::{
    app::{dialogs::DlgSettings, session::test_support},
    core::project_repos::ProjectManifest,
};
use std::path::PathBuf;

fn editor_frame(
    ctx: &egui::Context,
    dialog: &mut DlgSettings,
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
            egui::CentralPanel::default().show(ctx, |ui| super::paint(ui, dialog));
        },
    );
    output.textures_delta.clear();
    output
}

#[test]
fn repository_name_editor_renders_its_shared_storage_and_duplicate_guidance() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-editor-ui");
    let root = fixture();
    let project = test_support::project_from(&root);
    let mut dialog = DlgSettings::from_project(&project);
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
    assert!(rendered.contains("Optional shared names; blank uses the stable ID"));
    let _ = std::fs::remove_dir_all(root);
}

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "packet-settings-repository-names-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(root.join(".kool-ade-packet/config")).unwrap();
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
    std::fs::write(
        root.join(".kool-ade-packet/config/project.md"),
        "# Planner Configuration\n\n## Current User\nName: Settings Test\nGroups:\n\n## Stakeholders\n",
    )
    .unwrap();
    let manifest = ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Planning root".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    std::fs::write(
        root.join(crate::core::project_repos::PROJECT_FILE),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    root
}

#[test]
fn settings_save_commits_trimmed_repository_name_and_reload_restores_it() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-save");
    let root = fixture();
    let mut project = test_support::project_from(&root);
    let mut dialog = DlgSettings::from_project(&project);
    assert_eq!(dialog.repositories[0].name, "");
    dialog.repositories[0].name = "  Product API  ".into();
    let commit = dialog.apply(&mut project).unwrap();
    assert_eq!(commit.len(), 7);
    let manifest = ProjectManifest::load(&root).unwrap();
    assert_eq!(
        manifest.repositories[0].display_name.as_deref(),
        Some("Product API")
    );
    let paths = std::process::Command::new("git")
        .args(["show", "--format=", "--name-only", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    let changed = String::from_utf8(paths.stdout).unwrap();
    assert!(changed.contains(crate::core::project_repos::PROJECT_FILE));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_edit_is_discarded_without_changing_the_manifest_or_identity() {
    let _shield = crate::core::gitops::test_support::shield("repository-name-invalid");
    let root = fixture();
    let mut project = test_support::project_from(&root);
    let mut dialog = DlgSettings::from_project(&project);
    dialog.repositories[0].name = "Valid name".into();
    dialog.apply(&mut project).unwrap();
    let before = std::fs::read(root.join(crate::core::project_repos::PROJECT_FILE)).unwrap();

    let mut project = test_support::project_from(&root);
    let mut dialog = DlgSettings::from_project(&project);
    dialog.repositories[0].name = "x".repeat(41);
    let error = dialog.apply(&mut project).unwrap_err();
    assert!(error.detail().contains("40 characters"));
    assert_eq!(dialog.repositories[0].name, "Valid name");
    assert_eq!(
        std::fs::read(root.join(crate::core::project_repos::PROJECT_FILE)).unwrap(),
        before
    );
    let _ = std::fs::remove_dir_all(root);
}
