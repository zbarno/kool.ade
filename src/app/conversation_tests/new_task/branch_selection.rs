use super::*;

#[test]
fn new_task_can_override_and_persist_source_and_destination_branches() {
    let root = std::env::temp_dir().join(format!(
        "koolade-task-branches-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["config", "user.name", "Kool.ad/e Branch Test"],
        vec!["config", "user.email", "koolade-branch@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(root.join("README.md"), "branch fixture\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-qm", "fixture"]] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    for branch in ["release/2.1", "integration"] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["branch", branch])
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        project.refresh_git();
    }
    app.task_harness = Some(Box::new(StoppedHarness {
        wait_for_cancel: true,
    }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "+ New Task");
    let repo_label = crate::ui::Surface::repository_git_options(&app)[0]
        .label
        .clone();
    click_branch_picker(&mut app, &ctx, &format!("{repo_label} source branch"));
    click_text(&mut app, &ctx, "release/2.1");
    click_branch_picker(&mut app, &ctx, &format!("{repo_label} destination branch"));
    click_text(&mut app, &ctx, "integration");
    click_text(&mut app, &ctx, "Describe what you want to do…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Ship the feature branch".into())],
    );
    click_text(&mut app, &ctx, "Create Task");
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    let created = project.planning_work.last().unwrap();
    assert_eq!(created.source_branch.as_deref(), Some("release/2.1"));
    assert_eq!(created.destination_branch.as_deref(), Some("integration"));
    let intent = &created.git_intents["root"];
    assert_eq!(intent.source_ref, "release/2.1");
    assert_eq!(intent.destination_ref, "integration");
    assert!(intent.source_sha.len() == 40 || intent.source_sha.len() == 64);
    intent.validate().unwrap();
    let saved = crate::core::planning_work::load(&project.state.planning_store).unwrap();
    assert_eq!(
        saved.last().unwrap().source_branch.as_deref(),
        Some("release/2.1")
    );
    assert_eq!(
        saved.last().unwrap().destination_branch.as_deref(),
        Some("integration")
    );
    let key = created.key.clone();
    app.cancel_task_reply(&key);
    complete(&mut app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn origin_backed_destination_selection_rejects_local_only_branches() {
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.git.branches = vec!["main".into(), "local-only".into()];
        project.git.remote_branches = vec!["main".into()];
        project.git.has_origin = true;
    }
    assert_eq!(
        crate::ui::Surface::repository_destination_branches(&app),
        vec!["main"]
    );
    app.dispatch_ui_command(crate::ui::ApplicationCommand::CreatePlanningTask {
        kind: crate::core::planning_work::WorkKind::Feature,
        description: "Use a local-only destination".into(),
        parent_uid: None,
        git_intents: std::collections::BTreeMap::from([(
            "root".into(),
            crate::core::git_intent::GitIntentSelectionV1 {
                source_ref: "main".into(),
                destination_ref: "local-only".into(),
                publication_mode: crate::core::git_intent::PublicationModeV1::HoldForReview,
            },
        )]),
        routing_overrides: Default::default(),
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    assert!(project.planning_work.is_empty());
}

// Production boundary: the New Task repository selector must project branch
// options from every mapped code checkout. A single-checkout widget test
// cannot catch accidental reuse of the primary snapshot. Uses local Git only;
// setup cost is two temporary repositories and private manifest/map writes.
#[test]
fn repository_git_options_keep_branches_scoped_to_each_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let api = temp.path().join("api");
    for (path, branch, extra) in [
        (&root, "develop", "integration"),
        (&api, "release/2.0", "main"),
    ] {
        std::fs::create_dir_all(path).unwrap();
        for args in [
            vec!["init", "--quiet", "-b", branch],
            vec!["config", "user.name", "Kool.ad/e Repo Test"],
            vec!["config", "user.email", "koolade-repo@example.invalid"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(path.join("README.md"), "repo fixture\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(path)
                .args(["add", "."])
                .status()
                .unwrap()
                .success()
        );
        if path == &api {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(["remote", "add", "origin", "https://github.com/team/api.git",])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(path)
                .args(["commit", "-qm", "fixture"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(path)
                .args(["branch", extra])
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        let manifest = crate::core::project_repos::ProjectManifest {
            repositories: vec![
                crate::core::project_repos::Repository {
                    id: "root".into(),
                    role: "Primary code".into(),
                    remote: String::new(),
                    display_name: None,
                },
                crate::core::project_repos::Repository {
                    id: "api".into(),
                    role: "API".into(),
                    remote: "https://github.com/team/api.git".into(),
                    display_name: Some("API".into()),
                },
            ],
        };
        let checkouts = crate::core::project_repos::RepositoryCheckoutMapV1 {
            schema_version: 1,
            checkouts: std::collections::BTreeMap::from([("api".into(), api.clone())]),
        };
        project
            .state
            .planning_store
            .atomic_write(
                crate::artifacts::layout::store::PROJECT_MANIFEST,
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
        project
            .state
            .planning_store
            .atomic_write(
                "config/repository-checkouts.json",
                &serde_json::to_vec_pretty(&checkouts).unwrap(),
            )
            .unwrap();
        project.refresh_git();
    }
    let options = crate::ui::Surface::repository_git_options(&app);
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(
            crate::core::project_repos::ProjectManifest::load(&project.state.planning_store)
                .unwrap()
                .repositories
                .len(),
            2
        );
        assert_eq!(
            crate::core::gitops::origin_url(&api).as_deref(),
            Some("https://github.com/team/api.git")
        );
        assert_eq!(
            crate::core::gitops::worktree_root(&api).as_deref(),
            Some(api.as_path())
        );
        assert_eq!(project.state.repo_root, root);
    }
    let root_options = options.iter().find(|option| option.id == "root").unwrap();
    let api_options = options.iter().find(|option| option.id == "api").unwrap();
    assert_eq!(
        root_options.source_branches,
        crate::core::gitops::snapshot(&root).branches
    );
    assert_eq!(
        api_options.source_branches,
        crate::core::gitops::snapshot(&api).branches
    );
    assert_ne!(root_options.source_branches, api_options.source_branches);
}

// Branch labels and values are separate so long names can truncate without
// hiding the field's purpose. Target the value directly below its label.
fn click_branch_picker(app: &mut KooladeApp, ctx: &egui::Context, label: &str) {
    let output = frame(app, ctx, vec![]);
    let label = text_position(&output, label).expect("branch field label");
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "main" => {
                let pos = text.pos + text.galley.mesh_bounds.center().to_vec2();
                (pos.y > label.y && pos.y < label.y + 50.0).then_some(pos)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let visible = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            panic!("branch selector below its label; visible text: {visible:?}");
        });
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}
