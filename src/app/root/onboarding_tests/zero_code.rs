use super::super::HangingTurnHarness;
use super::support::*;
use crate::app::root::{KooladeApp, Screen};

#[test]
fn welcome_ui_creates_plans_joins_and_reopens_one_zero_code_workspace() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "workspace-onboarding-ui",
    );
    let base = std::env::temp_dir().join(format!(
        "koolade_onboarding_ui_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home_a = base.join("operator-a");
    let home_b = base.join("operator-b");
    std::fs::create_dir_all(&home_a).unwrap();
    std::fs::create_dir_all(&home_b).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home_a);
    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();

    type_into(&mut app, &ctx, "Project name", "Zero code workspace");
    click(&mut app, &ctx, "Create workspace");
    let Screen::Connected(project) = &app.screen else {
        let visible = visible_text(&frame(&mut app, &ctx, Vec::new()));
        panic!(
            "Create workspace should open a planning board; error: {:?}; visible: {visible:?}",
            app.conn_error
        )
    };
    let workspace_id = project.state.planning_store.project_id;
    assert!(project.state.repositories.repositories.is_empty());
    assert!(!project.state.repo_root.join(".koolade-packet").exists());
    assert!(!crate::ui::Surface::is_git_repo(&app));

    app.task_harness = Some(Box::new(HangingTurnHarness));
    click(&mut app, &ctx, "+ New Task");
    type_into(
        &mut app,
        &ctx,
        "Describe what you want to do",
        "Plan a zero-code workspace task through the board UI",
    );
    click(&mut app, &ctx, "Create Task");
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let work = project.planning_work.last().unwrap();
    let task_uid = work.uid.clone();
    assert_eq!(work.source_branch, None);
    assert_eq!(work.destination_branch, None);
    assert!(work.request.contains("zero-code workspace task"));
    wait_for_cancelled_turn(&mut app);

    let manager_a = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    let workspace_a = manager_a.open(workspace_id).unwrap();
    let work_path = workspace_a.store.layout().work_record(&task_uid).unwrap();
    let work_relative = work_path
        .strip_prefix(&workspace_a.planning_repo)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    crate::core::gitops::commit(
        &workspace_a.planning_repo,
        "Kool.ad/e: save onboarding fixture task",
        &[work_relative],
    )
    .unwrap();
    let remote = base.join("remotes/planning.git");
    clone_bare_remote(&workspace_a.planning_repo, &remote);
    manager_a
        .set_planning_remote(workspace_id, &remote.to_string_lossy())
        .unwrap();

    state_home.set(&home_b);
    app.screen = Screen::Welcome;
    app.conn_join_remote.clear();
    let join_url = url::Url::from_file_path(&remote).unwrap().to_string();
    type_into(
        &mut app,
        &ctx,
        "Planning Git remote URL or local checkout path",
        &join_url,
    );
    click(&mut app, &ctx, "Join workspace");
    let Screen::Connected(joined) = &app.screen else {
        panic!("Join workspace should open the shared planning board")
    };
    assert_eq!(joined.state.planning_store.project_id, workspace_id);
    assert_ne!(joined.state.repo_root, workspace_a.planning_repo);
    assert!(joined.state.repositories.repositories.is_empty());
    let joined_work = joined
        .planning_work
        .iter()
        .find(|work| work.uid == task_uid)
        .unwrap();
    assert!(joined_work.request.contains("zero-code workspace task"));

    app.screen = Screen::Welcome;
    app.conn_workspace_id = Some(workspace_id);
    click(&mut app, &ctx, "Open local workspace");
    let Screen::Connected(reopened) = &app.screen else {
        panic!(
            "Open local workspace should reopen the joined planning board; conn_error: {:?}",
            app.conn_error
        )
    };
    assert_eq!(reopened.state.planning_store.project_id, workspace_id);
    assert_eq!(
        reopened
            .planning_work
            .iter()
            .map(|work| &work.uid)
            .collect::<Vec<_>>(),
        vec![&task_uid]
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

// Production boundary: welcome wizard UI → application connect path → core
// managed-workspace sibling-pair creation (no in-test shortcuts). Invariants
// asserted: the operator's typed project name drives TWO sibling repositories
// (code + planning) under the chosen parent; the connected board is rooted at
// the freshly created CHECKOUT; exactly one code repository (id "root") is
// registered; the planning sibling carries managed-workspace scaffolding.
// Expected setup cost: in-process egui drive plus several local Git
// operations against a temp tree; no network, no Docker, no SSH.
#[test]
fn welcome_ui_wizard_creates_a_connected_sibling_pair() {
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("wizard-onboarding-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_wizard_ui_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let parent = base.join("projects");
    std::fs::create_dir_all(&parent).unwrap();

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    // Land on the door, enter the BLANK PROJECT wizard, and fill step 1.
    app.wizard_open(crate::app::root::WelcomeEntry::BlankProject);
    type_into(
        &mut app,
        &ctx,
        "Shown on the board and stamped on the planning repository",
        "Acme Portal",
    );
    app.wiz_apply_field(
        crate::app::root::WizField::Parent,
        parent.to_string_lossy().into_owned(),
    );
    app.wizard_continue();
    // An ordinary spaced title must ADVANCE, not trip the naming gate: the
    // derived planning name is slugged (`acme-portal.packet`), not taken
    // from the raw title.
    let advanced = app.wiz.as_ref().expect("wizard survives continue");
    assert_eq!(
        advanced.step, 1,
        "spaced project titles pass the naming gate"
    );
    assert!(
        advanced.error.is_none(),
        "no naming error for a spaced title: {:?}",
        advanced.error
    );
    app.wizard_commit();

    let Screen::Connected(project) = &app.screen else {
        let wiz_err = app.wiz.as_ref().and_then(|draft| draft.error.clone());
        let visible = visible_text(&frame(&mut app, &ctx, Vec::new()));
        panic!(
            "wizard create-and-connect should land on a planning board; wiz_error: {wiz_err:?}; conn_error: {:?}; visible: {visible:?}",
            app.conn_error
        )
    };
    let code = parent.join("acme-portal");
    let planning = parent.join("acme-portal.packet");
    assert!(
        code.is_dir(),
        "code sibling missing under the chosen parent"
    );
    assert!(
        planning.is_dir(),
        "planning sibling (.packet) missing under the chosen parent"
    );
    assert!(
        planning.join("manifest.json").is_file(),
        "planning sibling carries the managed workspace manifest"
    );
    assert_eq!(
        project.state.repo_root.canonicalize().unwrap(),
        code.canonicalize().unwrap(),
        "the connected surface is rooted at the code checkout"
    );
    assert_eq!(
        project.state.repositories.repositories.len(),
        1,
        "exactly the paired code repository is registered"
    );
    let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    assert_eq!(manager.list_local().unwrap().len(), 1);
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

// Production boundary: welcome wizard UI (existing-code track) → application
// connect path → core sibling-pair creation over an ALREADY INITIALIZED code
// repository. Invariants asserted: the operator's repository history and
// files survive byte-for-byte (no init, no commits, no README); a planning
// sibling is created BESIDE the existing folder; the connected board is
// rooted at the existing checkout with exactly one registered repository.
// Expected setup cost: in-process egui drive plus local Git ops on a temp
// tree; no network, no Docker, no SSH.
#[test]
fn welcome_ui_wizard_pairs_an_existing_code_repository() {
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("wizard-existing-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_wizard_existing_ui_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let parent = base.join("repos");
    std::fs::create_dir_all(&parent).unwrap();

    // The operator's pre-existing repository: two unrelated commits.
    let code = parent.join("ext-portal");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::write(code.join("notes.txt"), "operator baseline\n").unwrap();
    let git_here = |args: &[&str]| -> std::process::Output {
        let output = std::process::Command::new("git")
            .current_dir(&code)
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    git_here(&["init", "-q", "--initial-branch=main", "."]);
    git_here(&["add", "-A"]);
    git_here(&[
        "-c",
        "user.name=Wiz Fixture",
        "-c",
        "user.email=w@example.test",
        "commit",
        "-q",
        "-m",
        "operator commit one",
    ]);
    std::fs::write(code.join("more.txt"), "more\n").unwrap();
    git_here(&["add", "-A"]);
    git_here(&[
        "-c",
        "user.name=Wiz Fixture",
        "-c",
        "user.email=w@example.test",
        "commit",
        "-q",
        "-m",
        "operator commit two",
    ]);

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    // Land on the door, enter the EXISTING CODE wizard, and fill step 1:
    // the project name types into the LIVE panel; the folder arrives the
    // way the picker delivers it (wiz_apply_field).
    app.wizard_open(crate::app::root::WelcomeEntry::ExistingCode);
    type_into(
        &mut app,
        &ctx,
        "Shown on the board and stamped on the planning repository",
        "Existing Portal",
    );
    app.wiz_apply_field(
        crate::app::root::WizField::CodePath,
        code.to_string_lossy().into_owned(),
    );
    app.wizard_continue();
    app.wizard_commit();

    let Screen::Connected(project) = &app.screen else {
        let wiz_err = app.wiz.as_ref().and_then(|draft| draft.error.clone());
        let visible = visible_text(&frame(&mut app, &ctx, Vec::new()));
        panic!(
            "existing-code create-and-connect should land on a planning board; wiz_error: {wiz_err:?}; conn_error: {:?}; visible: {visible:?}",
            app.conn_error
        )
    };
    let planning = parent.join("ext-portal.packet");
    assert!(
        planning.join("manifest.json").is_file(),
        "the planning sibling is created beside the existing repository"
    );
    let count_out = git_here(&["rev-list", "--count", "HEAD"]);
    let count = String::from_utf8_lossy(&count_out.stdout).into_owned();
    assert_eq!(
        count.trim(),
        "2",
        "the operator's history is untouched by pairing"
    );
    assert_eq!(
        std::fs::read_to_string(code.join("notes.txt")).unwrap(),
        "operator baseline\n",
        "existing files are never rewritten"
    );
    assert_eq!(
        project.state.repo_root.canonicalize().unwrap(),
        code.canonicalize().unwrap(),
        "the connected surface is rooted at the EXISTING checkout"
    );
    assert_eq!(project.state.repositories.repositories.len(), 1);
    let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    assert_eq!(manager.list_local().unwrap().len(), 1);
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

// Production boundary: welcome wizard CLONE track → deferred pairing
// hand-off crossing UI commit → background worker thread → tick drain →
// sibling-pair creation → connect. Unit tests cannot prove the intent
// survives the real JoinHandle round trip and pairs BEFORE any connect;
// only driving this path end-to-end can. The hermetic
// `clone_computation_override` seam stands in for the network fetch and
// hands back a locally initialized repository that plays the role of the
// completed clone. Invariants asserted: exactly one planning sibling
// `<repo>.packet` is created beside the clone before the board connects;
// the board roots at the clone destination; exactly one code repository
// registers; the pairing intent is consumed, leaving none dangling.
// Expected setup cost: in-process egui drive plus a few local Git ops in a
// temp tree; no network, no Docker, no SSH.
#[test]
fn welcome_ui_wizard_clone_completion_pairs_before_connecting() {
    use std::sync::{Arc, Mutex};
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("wizard-clone-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_wiz_clone_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let downloads = base.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    // The stand-in "completed clone": a folder named for the repo segment,
    // holding an initialized repository.
    let dest = downloads.join("payroll-service");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("service.toml"), "[server]\n").unwrap();
    let git_here = |args: &[&str]| -> std::process::Output {
        let output = std::process::Command::new("git")
            .current_dir(&dest)
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    git_here(&["init", "-q", "--initial-branch=main", "."]);
    git_here(&["add", "-A"]);
    git_here(&[
        "-c",
        "user.name=Wiz Fixture",
        "-c",
        "user.email=w@example.test",
        "commit",
        "-q",
        "-m",
        "landed clone commit",
    ]);

    let captured: Arc<Mutex<(String, String)>> = Arc::new(Mutex::default());
    let cap2 = captured.clone();
    let dest_seam = dest.clone();
    let ctx = egui::Context::default();
    let mut app = KooladeApp {
        clone_computation_override: Some(Arc::new(move |source: String, repo: String| {
            *cap2.lock().unwrap() = (source, repo);
            Ok(dest_seam.clone())
        })),
        ..Default::default()
    };

    app.wizard_open(crate::app::root::WelcomeEntry::CloneRepository);
    type_into(
        &mut app,
        &ctx,
        "Example: https://github.com/zorgco/payroll-service",
        "https://github.com/ZorgCo/PayrollService",
    );
    app.wizard_continue();
    app.wizard_commit();
    assert!(
        app.clone_job.is_some(),
        "commit spawns the clone worker (conn_error: {:?})",
        app.conn_error
    );
    assert!(
        app.pending_clone_pairing.is_some(),
        "the pairing intent rides alongside the worker"
    );

    // Let the worker settle through the production tick path.
    for _ in 0..500 {
        if app.clone_job.is_none() {
            break;
        }
        app.tick(0.016, &ctx);
    }
    let Screen::Connected(project) = &app.screen else {
        let wiz_err = app.wiz.as_ref().and_then(|d| d.error.clone());
        panic!(
            "paired clone should connect the board; wiz_error: {wiz_err:?}; conn_error: {:?}",
            app.conn_error
        );
    };
    let planning = downloads.join("payroll-service.packet");
    assert!(
        planning.join("manifest.json").is_file(),
        "the planning sibling was created beside the clone BEFORE connecting"
    );
    assert_eq!(
        project.state.repo_root.canonicalize().unwrap(),
        dest.canonicalize().unwrap(),
        "the connected board roots at the clone destination"
    );
    assert_eq!(
        project.state.repositories.repositories.len(),
        1,
        "exactly one code repository is registered"
    );
    assert!(
        app.pending_clone_pairing.is_none(),
        "the intent was consumed"
    );
    assert!(app.wiz.is_none(), "a successful connect retires the wizard");
    let (source, repo) = captured.lock().unwrap().clone();
    assert_eq!(
        source, "https://github.com/ZorgCo/PayrollService.git",
        "the worker receives the canonical rebuilt url (raw paste never reaches git argv), got {source}"
    );
    assert_eq!(
        repo, "PayrollService",
        "repo segment case preserved, got {repo}"
    );
    let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    assert_eq!(manager.list_local().unwrap().len(), 1);
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

// Companion of the deferral test: when the worker FAILS, the wizard keeps
// the operator on the panel WITH the diagnosis surfaced on the wizard
// itself (not hidden in the classic card banner), keeps the intent armed
// for a corrected retry, and no planning sibling appears.
#[test]
fn welcome_ui_wizard_clone_worker_failure_surfaces_on_the_wizard() {
    use std::sync::Arc;
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("wizard-clonefail-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_wiz_clonefail_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let downloads = base.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();

    // The seam panics: a stand-in for a killed/unstable fetch.
    let ctx = egui::Context::default();
    let mut app = KooladeApp {
        clone_computation_override: Some(Arc::new(
            |_source: String,
             _repo: String|
             -> std::result::Result<std::path::PathBuf, crate::error::AppError> {
                panic!("simulated unstable fetch")
            },
        )),
        ..Default::default()
    };

    app.wizard_open(crate::app::root::WelcomeEntry::CloneRepository);
    type_into(
        &mut app,
        &ctx,
        "Example: https://github.com/zorgco/payroll-service",
        "https://github.com/ZorgCo/Fragile",
    );
    app.wizard_continue();
    app.wizard_commit();
    assert!(app.clone_job.is_some(), "commit spawns the clone worker");

    // The worker is a REAL thread that panics; under heavy parallel-test
    // contention it may starve for wall time. Tick patiently (breaking as
    // soon as the job settles) while steadily yielding real time so the
    // panic is guaranteed a scheduling slot. Common cost stays trivial.
    for round in 0..20_000 {
        if app.clone_job.is_none() {
            break;
        }
        app.tick(0.016, &ctx);
        if round % 50 == 49 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    assert!(app.clone_job.is_none(), "the dead worker is settled");
    assert!(
        app.pending_clone_pairing.is_some(),
        "the intent stays armed so a corrected retry pairs on completion"
    );
    let wiz = app.wiz.as_ref().expect("the wizard STAYS OPEN on failure");
    let message = wiz.error.as_deref().unwrap_or("<none>");
    assert!(
        message.starts_with("The clone did not complete:"),
        "the wizard panel carries the failure, got: {message}"
    );
    assert!(
        wiz.error
            .as_deref()
            .unwrap_or("")
            .contains("stopped unexpectedly"),
        "the worker diagnosis is mirrored verbatim: {message}"
    );
    assert_eq!(wiz.step, 0, "back on the inputs step for the fix");
    assert!(matches!(app.screen, Screen::Welcome), "no phantom connect");
    assert!(
        !downloads.join("fragile.packet").exists() && !downloads.join("Fragile.packet").exists(),
        "no planning sibling materializes from a dead clone"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

/// The gate is directional: a typed ILLEGAL packet name is refused at
/// continue (before any folder work), while a legal/derived name passes.
#[test]
fn welcome_ui_wizard_blank_refuses_an_illegal_typed_packet_name() {
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("wizard-namedeny-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_wiz_namedeny_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let parent = base.join("projects");
    std::fs::create_dir_all(&parent).unwrap();

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    app.wizard_open(crate::app::root::WelcomeEntry::BlankProject);
    type_into(
        &mut app,
        &ctx,
        "Shown on the board and stamped on the planning repository",
        "Fine Report",
    );
    app.wiz_apply_field(
        crate::app::root::WizField::Parent,
        parent.to_string_lossy().into_owned(),
    );
    type_into(
        &mut app,
        &ctx,
        "Leave empty for the suggested <name>.packet",
        "fine report!.packet",
    );
    app.wizard_continue();

    let wiz = app.wiz.as_ref().expect("the wizard stays open on refusal");
    assert_eq!(wiz.step, 0, "refusal bounces back to the inputs step");
    let message = wiz.error.as_deref().expect("the refusal explains itself");
    assert!(
        message.contains("fine report!.packet"),
        "names the offending name: {message}"
    );
    assert!(
        message.contains("unusable"),
        "states the name is unusable: {message}"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

/// Dismissal channel 1: `Esc` while the modal is open retires the wizard.
#[test]
fn welcome_ui_wizard_modal_esc_dismisses_it() {
    let base = std::env::temp_dir().join(format!(
        "koolade_wiz_modalesc_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    app.wizard_open(crate::app::root::WelcomeEntry::ExistingCode);
    // Two paint passes: the modal Area settles into the output shapes on
    // the second frame in some interleavings.
    let _ = frame(&mut app, &ctx, Vec::new());
    let output = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&output)
            .iter()
            .any(|line| line.contains("Step 1 of 2")),
        "the modal panel is up before Esc is tried"
    );
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(egui::pos2(900.0, 450.0)),
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            },
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            },
        ],
    );
    assert!(app.wiz.is_none(), "Esc must retire the modal wizard");
    let _ = std::fs::remove_dir_all(base);
}

/// Dismissal channel 2: a click on the backdrop (outside the panel) retires
/// the wizard — and, proving the input sequestration, lands on NOTHING of
/// the canvas beneath.
#[test]
fn welcome_ui_wizard_modal_backdrop_click_dismisses_it() {
    let base = std::env::temp_dir().join(format!(
        "koolade_wiz_modalbackdrop_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    app.wizard_open(crate::app::root::WelcomeEntry::CloneRepository);
    // Two paint passes: the modal Area settles into the output shapes on
    // the second frame in some interleavings.
    let _ = frame(&mut app, &ctx, Vec::new());
    let output = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&output)
            .iter()
            .any(|line| line.contains("Step 1 of 2")),
        "the modal panel is up before the backdrop click is tried"
    );
    // Far left of the centered panel: wherever the canvas control there is,
    // the backdrop must swallow the click.
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(egui::pos2(180.0, 880.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(180.0, 880.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(180.0, 880.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(
        app.wiz.is_none(),
        "a backdrop click must retire the modal wizard"
    );
    assert!(
        matches!(app.screen, Screen::Welcome),
        "the swallowed click must not have submitted anything below"
    );
    let _ = std::fs::remove_dir_all(base);
}

/// Sequestration from the other side: while the modal is open, a click on a
/// journey door beneath the backdrop must NOT start a new journey (or
/// re-route this one).
#[test]
fn welcome_ui_wizard_modal_veils_the_canvas_below_while_open() {
    let base = std::env::temp_dir().join(format!(
        "koolade_wiz_modalveil_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    app.wizard_open(crate::app::root::WelcomeEntry::CloneRepository);
    let _ = frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    // The blank-project door's call-to-action, painted under the backdrop:
    // a leaking click would REPLACE the draft with the blank journey (or,
    // worse, the backdrop close and the journey swap could fight).
    click(&mut app, &ctx, "Create a brand-new project");
    frame(&mut app, &ctx, Vec::new());
    assert!(
        matches!(app.screen, Screen::Welcome),
        "a veiled canvas click must not navigate"
    );
    // The backdrop may legitimately consume the click as a dismissal; the
    // invariant under test is that no veiled action altered the draft.
    if let Some(wiz) = app.wiz.as_ref() {
        assert!(
            matches!(wiz.entry, crate::app::root::WelcomeEntry::CloneRepository),
            "a veiled click must not switch journeys (entry still clone)"
        );
        assert_eq!(wiz.step, 0, "a veiled click must not disturb the draft");
    }
    let _ = std::fs::remove_dir_all(base);
}
