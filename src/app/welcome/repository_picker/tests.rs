use super::paint;

fn frame(ctx: &egui::Context, root: &str, events: Vec<egui::Event>) -> egui::FullOutput {
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
                let mut path = root.to_owned();
                paint(ui, &mut path);
            });
        },
    );
    output.textures_delta.clear();
    output
}

fn frame_mut(ctx: &egui::Context, path: &mut String, events: Vec<egui::Event>) -> egui::FullOutput {
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
            egui::CentralPanel::default().show(ctx, |ui| paint(ui, path));
        },
    );
    output.textures_delta.clear();
    output
}

fn text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| {
        if let egui::Shape::Text(text) = &shape.shape
            && text.galley.text() == needle
        {
            Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
        } else {
            None
        }
    })
}

fn contains_text(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains(needle))
    })
}

#[test]
fn welcome_picker_shows_shared_name_and_role_fallback() {
    let root = std::env::temp_dir().join(format!(
        "packet_repository_picker_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let layout = crate::artifacts::layout::ArtifactLayout::new(&root);
    std::fs::create_dir_all(layout.project_manifest().parent().unwrap()).unwrap();
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![
            crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: String::new(),
                display_name: Some("Primary workspace".into()),
            },
            crate::core::project_repos::Repository {
                id: "worker".into(),
                role: "Worker checkout".into(),
                remote: "https://example.test/worker.git".into(),
                display_name: None,
            },
        ],
    };
    std::fs::write(
        layout.project_manifest(),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let ctx = egui::Context::default();
    let root_text = root.to_string_lossy().into_owned();
    frame(&ctx, &root_text, Vec::new());
    let output = frame(&ctx, &root_text, Vec::new());
    let pos = text_position(&output, "Registered repositories").unwrap();
    frame(
        &ctx,
        &root_text,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        &ctx,
        &root_text,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let output = frame(&ctx, &root_text, Vec::new());
    assert!(contains_text(&output, "Primary workspace"));
    assert!(contains_text(&output, "Worker checkout"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn welcome_picker_selects_the_mapped_checkout_by_repository_id() {
    use std::{ffi::OsString, path::Path, process::Command};

    struct PacketHome(Option<OsString>);
    impl Drop for PacketHome {
        fn drop(&mut self) {
            // SAFETY: the process-wide Git test shield serializes this restore.
            unsafe {
                if let Some(previous) = self.0.take() {
                    std::env::set_var("PACKET_HOME", previous);
                } else {
                    std::env::remove_var("PACKET_HOME");
                }
            }
        }
    }
    fn init(path: &Path, remote: &str) {
        std::fs::create_dir_all(path).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(path)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["remote", "add", "origin", remote])
                .current_dir(path)
                .status()
                .unwrap()
                .success()
        );
    }

    let _shield = crate::core::gitops::test_support::shield("welcome-repository-selection");
    let base = std::env::temp_dir().join(format!(
        "packet_repository_selection_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = base.join("planning");
    let worker = base.join("worker");
    init(&root, "https://example.test/planning.git");
    init(&worker, "https://example.test/worker.git");
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![
            crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: "https://example.test/planning.git".into(),
                display_name: Some("Planning".into()),
            },
            crate::core::project_repos::Repository {
                id: "worker".into(),
                role: "Worker checkout".into(),
                remote: "https://example.test/worker.git".into(),
                display_name: Some("API".into()),
            },
        ],
    };
    let manifest_path = crate::artifacts::layout::ArtifactLayout::new(&root).project_manifest();
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let packet_home = base.join("packet-home");
    let previous = std::env::var_os("PACKET_HOME");
    // SAFETY: `_shield` serializes all test access to PACKET_HOME.
    unsafe { std::env::set_var("PACKET_HOME", &packet_home) };
    let _restore = PacketHome(previous);
    crate::core::project_repos::map_local_checkout(&root, "worker", &worker).unwrap();

    let ctx = egui::Context::default();
    let mut path = root.to_string_lossy().into_owned();
    frame_mut(&ctx, &mut path, Vec::new());
    let output = frame_mut(&ctx, &mut path, Vec::new());
    let menu = text_position(&output, "Registered repositories").unwrap();
    frame_mut(
        &ctx,
        &mut path,
        vec![
            egui::Event::PointerMoved(menu),
            egui::Event::PointerButton {
                pos: menu,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame_mut(
        &ctx,
        &mut path,
        vec![egui::Event::PointerButton {
            pos: menu,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let output = frame_mut(&ctx, &mut path, Vec::new());
    let api = text_position(&output, "API").unwrap();
    frame_mut(
        &ctx,
        &mut path,
        vec![
            egui::Event::PointerMoved(api),
            egui::Event::PointerButton {
                pos: api,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame_mut(
        &ctx,
        &mut path,
        vec![egui::Event::PointerButton {
            pos: api,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert_eq!(Path::new(&path), worker.canonicalize().unwrap());
    let shared = std::fs::read_to_string(manifest_path).unwrap();
    assert!(shared.contains("API"));
    assert!(!shared.contains(&worker.to_string_lossy().to_string()));
    drop(_restore);
    let _ = std::fs::remove_dir_all(base);
}
