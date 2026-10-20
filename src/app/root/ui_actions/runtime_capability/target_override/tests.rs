use super::super::Screen;
use crate::project_runtime::{
    AppSurface, ProjectCapabilityManifestV1, ProjectRuntimeStore, TargetCapabilityState,
};

#[path = "tests/helpers.rs"]
mod helpers;
#[path = "tests/root_guard.rs"]
mod root_guard;
use helpers::{frame, pointer_button, text_position};

#[test]
// Production boundary: rendered Task Details dispatches through AddRuntimeTarget
// into file-backed state; store tests cannot prove UI wiring or visible rejection.
// Uses egui and a synthetic git-backed fixture only (no Docker, SSH, network, or
// credentials); setup is three in-process click/save flows.
fn task_details_custom_web_and_headless_targets_persist_matching_readiness() {
    assert_custom_target_persists(AppSurface::Web);
    assert_custom_target_persists(AppSurface::HeadlessService);
    assert_custom_target_persists(AppSurface::LinuxDesktop);
}

#[test]
// Production boundary: a Task Details adapter command must re-inspect the
// isolated task checkout, persist a locally fingerprinted recipe, and route it
// through the app's normal UI dispatcher. Store tests alone cannot prove that
// only the app-owned adapter action creates this provenance. Uses a synthetic
// package.json, a synthetic local Git commit, and in-process stores. Git is
// the only external executable; no Docker or network is used, with low setup cost.
fn task_detail_applies_only_the_metadata_matched_trusted_test_adapter() {
    let mut app = crate::app::root::board_tests::fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/002-task.md";
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("package.json"), "{}\n").unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Fixture User"],
        vec!["config", "user.email", "fixture@example.invalid"],
        vec!["add", "package.json"],
        vec!["commit", "--quiet", "-m", "fixture source"],
    ] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(root.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(head.status.success());
    let source_sha = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    let repository_id = "fixture-repository".to_owned();
    let identity = crate::domain::ArtifactIdentity {
        uid: task_uid.clone(),
        display_id: "TASK-002".into(),
        title: "Fixture task".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, &repository_id, Vec::new())
            .unwrap();
    let (planning, manifest_revision) = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture is connected")
        };
        let document = project
            .task_documents
            .iter_mut()
            .find(|document| document.path == ticket)
            .unwrap();
        document.identity = Some(identity);
        document.metadata = Some(metadata);
        let implementation = project.implementation_states.get_mut(ticket).unwrap();
        implementation.task_uid = Some(task_uid.clone());
        implementation.workspace_uid = Some(workspace_uid.clone());
        implementation.verified_head = Some(source_sha.clone());
        implementation.task_repository = root.path().to_path_buf();
        let planning = project.state.planning_store.clone();
        let store = ProjectRuntimeStore::new(&planning);
        let manifest = store
            .discover_and_save(
                root.path(),
                &crate::project_runtime::DiscoveryContext {
                    workspace_id: workspace_uid,
                    repository_id,
                    owning_task_id: task_uid.clone(),
                    source_sha,
                },
                0,
            )
            .unwrap();
        let target_id = manifest.candidate_targets[0].target_id.clone();
        let selected = store
            .select_target_and_save(&task_uid, &target_id, manifest.revision)
            .unwrap();
        (planning, (target_id, selected.revision))
    };
    let (target_id, revision) = manifest_revision;
    crate::ui::Surface::dispatch(
        &mut app,
        crate::ui::ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::ApplyRuntimeTestAdapter {
                ticket: ticket.into(),
                target_id: target_id.clone(),
                expected_manifest_revision: revision,
            },
        ),
    );
    let manifest = ProjectRuntimeStore::new(&planning)
        .load_manifest()
        .unwrap()
        .unwrap();
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == target_id)
        .unwrap();
    let recipe = manifest
        .recipes
        .iter()
        .find(|recipe| recipe.recipe_id == target.test_recipe_refs[0])
        .unwrap();
    assert_eq!(
        recipe.source,
        crate::project_runtime::RecipeSource::TrustedAdapter
    );
    assert_eq!(recipe.executable_id, "npm");
    assert_eq!(recipe.argv, ["test"]);
    let store = ProjectRuntimeStore::new(&planning);
    assert!(store.recipe_is_locally_approved(&manifest, target, recipe));
    let mut changed_recipe = recipe.clone();
    changed_recipe.argv.push("--unreviewed".into());
    assert!(!store.recipe_is_locally_approved(&manifest, target, &changed_recipe));
    let mut changed_target = target.clone();
    changed_target.project_relative_root = "other-app".into();
    assert!(!store.recipe_is_locally_approved(&manifest, &changed_target, recipe));
}

fn assert_custom_target_persists(app_surface: AppSurface) {
    let mut app = crate::app::root::board_tests::fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/002-task.md";
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_uid);
    let repository_id = "fixture-repository".to_owned();
    let source_sha = "a".repeat(40);
    let identity = crate::domain::ArtifactIdentity {
        uid: task_uid.clone(),
        display_id: "TASK-002".into(),
        title: "Fixture task".into(),
        parent_uid: Some(batch_uid),
    };
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, &repository_id, Vec::new())
            .unwrap();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture is connected")
    };
    let document = project
        .task_documents
        .iter_mut()
        .find(|document| document.path == ticket)
        .unwrap();
    document.identity = Some(identity);
    document.metadata = Some(metadata);
    let implementation = project.implementation_states.get_mut(ticket).unwrap();
    implementation.task_uid = Some(task_uid.clone());
    implementation.workspace_uid = Some(workspace_uid.clone());
    implementation.verified_head = Some(source_sha.clone());

    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: workspace_uid,
        repository_id,
        source_sha,
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: Vec::new(),
        selected_target_id: None,
        target_capabilities: Vec::<TargetCapabilityState>::new(),
        recipes: Vec::new(),
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 0,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 0,
    };
    ProjectRuntimeStore::new(&project.state.planning_store)
        .save_manifest(&manifest, 0)
        .unwrap();
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .unwrap()
        .clone();
    let Screen::Connected(_) = &app.screen else {
        unreachable!()
    };
    let view = crate::ui::Surface::task_detail_view(&mut app, ticket)
        .expect("Task Details exposes a view model");
    let runtime = view
        .runtime_capability
        .as_ref()
        .expect("runtime capability is exposed to Task Details");
    assert_eq!(runtime.manifest_revision, Some(1));

    let context = egui::Context::default();
    crate::ui::layout::seed_custom_runtime_target_repository_root_draft(
        &context,
        ticket,
        app_surface,
    );
    let mut output = frame(&context, &mut app, &document, Vec::new());
    if text_position(&output, "Approve and save custom target").is_none() {
        let position = text_position(&output, "Add a custom application target")
            .expect("custom target section is rendered in Task Details");
        frame(
            &context,
            &mut app,
            &document,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
            ],
        );
        output = frame(
            &context,
            &mut app,
            &document,
            vec![pointer_button(position, false)],
        );
    }
    let position = text_position(&output, "Approve and save custom target")
        .expect("custom target action is rendered after opening the section");
    frame(
        &context,
        &mut app,
        &document,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    frame(
        &context,
        &mut app,
        &document,
        vec![pointer_button(position, false)],
    );

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let saved = ProjectRuntimeStore::new(&project.state.planning_store)
        .load_manifest()
        .unwrap()
        .expect("custom target manifest is persisted");
    assert_eq!(saved.revision, 2);
    assert_eq!(saved.candidate_targets.len(), 1);
    let target = &saved.candidate_targets[0];
    ProjectRuntimeStore::new(&project.state.planning_store)
        .verify_user_target_approval(&saved, target)
        .expect("the click path persists a verifiable user approval receipt");
    assert_eq!(target.owning_task_id, task_uid);
    assert_eq!(target.project_relative_root, ".");
    let artifact_spec = saved
        .artifact_specs
        .get(target.artifact_spec_ref.as_ref().unwrap())
        .expect("the user approved artifact list is persisted");
    assert_eq!(artifact_spec, &["dist/index.html"]);
    assert_eq!(target.app_surface, app_surface);
    assert_eq!(
        target.capability_state,
        TargetCapabilityState::ReadyButUntested
    );
    if matches!(app_surface, AppSurface::Web | AppSurface::HeadlessService) {
        assert_eq!(target.port_needs, [8080]);
    } else {
        assert!(target.port_needs.is_empty());
    }
    assert!(
        saved
            .evidence_refs
            .iter()
            .any(|reference| reference.starts_with("user-approval:"))
    );
    assert_eq!(saved.user_overrides.len(), 2);
    assert!(saved.user_overrides.iter().all(|recipe| {
        recipe.source == crate::project_runtime::RecipeSource::UserApproved
            && recipe
                .evidence_refs
                .iter()
                .any(|reference| reference.starts_with("user-approval:"))
    }));
    let build_id = target.build_recipe_ref.as_deref().unwrap();
    let build = saved
        .user_overrides
        .iter()
        .find(|recipe| recipe.recipe_id == build_id)
        .expect("target build recipe reference resolves");
    assert_eq!(build.argv, ["build.js", "--release"]);
    assert_eq!(build.allowed_output_paths, ["dist/index.html"]);
    let start_id = target.start_recipe_ref.as_deref().unwrap();
    let start = saved
        .user_overrides
        .iter()
        .find(|recipe| recipe.recipe_id == start_id)
        .expect("target start recipe reference resolves");
    assert_eq!(start.argv, ["server.js"]);
    let readiness_id = target.readiness_probe_ref.as_deref().unwrap();
    let expected_readiness = match app_surface {
        AppSurface::Web => crate::project_runtime::ReadinessProbeV1::HttpOnApprovedPort {
            port: 8080,
            path: "/health".into(),
            expected_status: 200,
        },
        AppSurface::HeadlessService => {
            crate::project_runtime::ReadinessProbeV1::LocalServiceHealth {
                port: 8080,
                path: "/health".into(),
            }
        }
        AppSurface::LinuxDesktop => crate::project_runtime::ReadinessProbeV1::ProcessAlive {
            executable_id: "node".into(),
        },
        AppSurface::UnsupportedTarget => panic!("unsupported targets cannot be approved"),
    };
    assert_eq!(
        saved.readiness_probes.get(readiness_id),
        Some(&expected_readiness)
    );
    let receipt_id = build
        .evidence_refs
        .iter()
        .find_map(|reference| reference.strip_prefix("user-approval:"))
        .expect("approved recipe carries its receipt ID");
    let proposal_sha256 = ProjectRuntimeStore::user_target_proposal_sha256(
        &saved.workspace_id,
        &saved.repository_id,
        &saved.source_sha,
        target,
        &saved.user_overrides,
        saved.readiness_probes.get(readiness_id),
        saved
            .artifact_specs
            .get(target.artifact_spec_ref.as_ref().unwrap()),
    )
    .unwrap();
    crate::persistence::runtime_target_approvals::revoke(
        &crate::persistence::runtime_target_approvals::Binding {
            workspace_id: saved.workspace_id.clone(),
            task_id: target.owning_task_id.clone(),
            repository_id: saved.repository_id.clone(),
            source_sha: saved.source_sha.clone(),
            target_id: target.target_id.clone(),
            proposal_sha256,
            receipt_id: receipt_id.to_owned(),
        },
    )
    .unwrap();
    let refreshed = crate::ui::Surface::task_detail_view(&mut app, ticket)
        .expect("Task Details refreshes the saved custom target");
    assert_eq!(
        refreshed
            .runtime_capability
            .as_ref()
            .and_then(|runtime| runtime.targets.first())
            .map(|target| target.surface),
        Some(app_surface),
        "the Task Details read model preserves the target surface"
    );
}
